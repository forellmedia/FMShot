//! macOS: ScreenCaptureKit-Stream (Bildschirm, Systemton) und AVFoundation
//! (Mikrofon) schreiben über AVAssetWriter direkt in Dateien. Alle Quellen
//! liefern Zeitstempel der Systemuhr (Host-Zeit); so passen die Spuren ohne
//! Abgleich zusammen.
//!
//! Ablauf: Der Stream läuft schon während des Countdowns, damit das erste
//! Bild sofort da ist. Erst [`Recording::begin`] legt den Nullpunkt fest; das
//! letzte Bild davor wird auf ihn gelegt (ein ruhiger Bildschirm liefert sonst
//! lange kein neues Bild). Pausen werden aus der Zeitachse herausgerechnet.

// Die synchronen AVAsset-Aufrufe laufen ohnehin im Hintergrund-Thread.
#![allow(deprecated)]

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex, MutexGuard, mpsc};
use std::time::Duration;

use block2::RcBlock;
use dispatch2::{DispatchQueue, DispatchRetained};
use fmshot_core::RgbaImage;
use objc2::rc::Retained;
use objc2::runtime::{AnyObject, Bool, NSObject, NSObjectProtocol, ProtocolObject};
use objc2::{AllocAnyThread, DefinedClass, define_class, msg_send};
use objc2_av_foundation::{
    AVAssetExportPresetHighestQuality, AVAssetExportSession, AVAssetExportSessionStatus,
    AVAssetImageGenerator, AVAssetWriter, AVAssetWriterInput, AVAssetWriterStatus,
    AVAuthorizationStatus, AVCaptureAudioDataOutput, AVCaptureAudioDataOutputSampleBufferDelegate,
    AVCaptureConnection, AVCaptureDevice, AVCaptureDeviceInput, AVCaptureOutput, AVCaptureSession,
    AVFileTypeAppleM4A, AVFileTypeMPEG4, AVMediaTypeAudio, AVMediaTypeVideo, AVMutableComposition,
    AVURLAsset, AVVideoAllowFrameReorderingKey, AVVideoAverageBitRateKey, AVVideoCodecKey,
    AVVideoCodecTypeHEVC, AVVideoCompressionPropertiesKey, AVVideoExpectedSourceFrameRateKey,
    AVVideoHeightKey, AVVideoMaxKeyFrameIntervalKey, AVVideoWidthKey,
};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGDisplayCopyDisplayMode, CGDisplayMode,
    CGImage, CGImageAlphaInfo, CGImageByteOrderInfo, CGPreflightScreenCaptureAccess,
    kCGColorSpaceSRGB,
};
use objc2_core_media::{
    CMAudioFormatDescriptionGetStreamBasicDescription, CMClock, CMSampleBuffer, CMSampleTimingInfo,
    CMTime, CMTimeRange, kCMTimeInvalid, kCMTimeZero,
};
use objc2_foundation::{NSArray, NSDictionary, NSError, NSNumber, NSString, NSURL};
use objc2_screen_capture_kit::{
    SCContentFilter, SCDisplay, SCFrameStatus, SCRunningApplication, SCShareableContent, SCStream,
    SCStreamConfiguration, SCStreamDelegate, SCStreamFrameInfoStatus, SCStreamOutput,
    SCStreamOutputType,
};

use crate::{
    AudioDevice, Levels, MediaError, Permission, RecordFiles, RecordOptions, RecordSummary,
    RecordTarget, Recording, Result, even, screen_bitrate,
};

const TIMEOUT: Duration = Duration::from_secs(5);
/// Abschliessen der Dateien kann bei langen Videos dauern.
const FINISH_TIMEOUT: Duration = Duration::from_secs(60);
/// `420v`: YCbCr 4:2:0 – das Format, das der Encoder ohnehin braucht.
const PIXEL_FORMAT_420V: u32 = 0x3432_3076;
/// `aac `
const AUDIO_FORMAT_AAC: u32 = 0x6161_6320;
const SAMPLE_RATE: u32 = 48_000;
const AUDIO_FORMAT_FLAG_IS_FLOAT: u32 = 1;

/// Objective-C-Objekte, die nur unter dem Mutex benutzt werden.
struct SendRetained<T>(Retained<T>);
// SAFETY: Zugriff nur unter dem Mutex von `Shared` bzw. unveränderlich.
unsafe impl<T> Send for SendRetained<T> {}
// SAFETY: wie oben.
unsafe impl<T> Sync for SendRetained<T> {}

struct SendCf<T: 'static>(CFRetained<T>);
// SAFETY: CMSampleBuffer ist nach dem Erzeugen unveränderlich.
unsafe impl<T> Send for SendCf<T> {}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Phase {
    /// Stream läuft, es wird noch nicht geschrieben (Countdown).
    Warmup,
    Recording,
    Paused,
    Stopped,
}

/// Eine Datei mit genau einer Spur.
struct Track {
    writer: SendRetained<AVAssetWriter>,
    input: SendRetained<AVAssetWriterInput>,
    path: PathBuf,
    samples: u64,
}

impl Track {
    fn append(&self, sample: &CMSampleBuffer) -> bool {
        unsafe {
            if !self.input.0.isReadyForMoreMediaData() {
                // Echtzeit: lieber ein Bild auslassen als blockieren.
                return false;
            }
            self.input.0.appendSampleBuffer(sample)
        }
    }

    /// Schliesst die Datei ab. Ohne eine einzige Probe ist sie wertlos und
    /// wird gelöscht.
    fn finish(&self, end: CMTime) -> std::result::Result<bool, String> {
        let writer = &self.writer.0;
        unsafe {
            if writer.status() != AVAssetWriterStatus::Writing || self.samples == 0 {
                writer.cancelWriting();
                let _ = std::fs::remove_file(&self.path);
                return Ok(false);
            }
            writer.endSessionAtSourceTime(end);
            self.input.0.markAsFinished();
        }
        let (tx, rx) = mpsc::sync_channel(1);
        let block = RcBlock::new(move || {
            let _ = tx.send(());
        });
        unsafe { writer.finishWritingWithCompletionHandler(&block) };
        rx.recv_timeout(FINISH_TIMEOUT)
            .map_err(|_| "Datei abschliessen dauert zu lange".to_string())?;
        unsafe {
            if writer.status() == AVAssetWriterStatus::Completed {
                Ok(true)
            } else {
                Err(writer
                    .error()
                    .map(|e| e.localizedDescription().to_string())
                    .unwrap_or_else(|| "Datei konnte nicht geschrieben werden".into()))
            }
        }
    }

    fn cancel(&self) {
        unsafe { self.writer.0.cancelWriting() };
        let _ = std::fs::remove_file(&self.path);
    }
}

struct State {
    phase: Phase,
    /// Nullpunkt (Host-Zeit) – gesetzt mit `begin`.
    t0: CMTime,
    pause_started: CMTime,
    /// Summe aller Pausen.
    offset: CMTime,
    last_frame: Option<SendCf<CMSampleBuffer>>,
    screen: Track,
    system: Option<Track>,
    mic: Option<Track>,
    failure: Option<String>,
}

struct Shared {
    state: Mutex<State>,
    /// Bits eines f32 – bei positiven Zahlen ist die Bit-Reihenfolge die der Werte.
    system_peak: AtomicU32,
    mic_peak: AtomicU32,
}

impl Shared {
    fn lock(&self) -> MutexGuard<'_, State> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }
}

#[derive(Clone, Copy)]
enum Source {
    Screen,
    System,
    Mic,
}

fn host_now() -> CMTime {
    unsafe { CMClock::host_time_clock().time() }
}

fn is_before(a: CMTime, b: CMTime) -> bool {
    unsafe { a.compare(b) < 0 }
}

/// Kopie mit verschobenen Zeitstempeln (Pausen herausgerechnet).
fn shifted(sample: &CMSampleBuffer, offset: CMTime) -> Option<CFRetained<CMSampleBuffer>> {
    unsafe {
        let mut needed = 0;
        sample.sample_timing_info_array(0, std::ptr::null_mut(), &mut needed);
        let mut timing = vec![
            CMSampleTimingInfo {
                duration: kCMTimeInvalid,
                presentationTimeStamp: kCMTimeInvalid,
                decodeTimeStamp: kCMTimeInvalid,
            };
            needed.max(1) as usize
        ];
        if needed > 0 {
            sample.sample_timing_info_array(needed, timing.as_mut_ptr(), &mut needed);
        } else {
            timing[0].presentationTimeStamp = sample.presentation_time_stamp();
        }
        for t in &mut timing {
            if t.presentationTimeStamp.flags.0 & 1 != 0 {
                t.presentationTimeStamp = t.presentationTimeStamp.subtract(offset);
            }
            if t.decodeTimeStamp.flags.0 & 1 != 0 {
                t.decodeTimeStamp = t.decodeTimeStamp.subtract(offset);
            }
        }
        let mut out: *mut CMSampleBuffer = std::ptr::null_mut();
        let status = CMSampleBuffer::create_copy_with_new_timing(
            None,
            sample,
            timing.len() as _,
            timing.as_ptr(),
            NonNull::from(&mut out),
        );
        if status != 0 {
            return None;
        }
        NonNull::new(out).map(|p| CFRetained::from_raw(p))
    }
}

/// Nur vollständige Bilder (nicht „Bildschirm unverändert“) schreiben.
fn is_complete_frame(sample: &CMSampleBuffer) -> bool {
    unsafe {
        if sample.image_buffer().is_none() {
            return false;
        }
        let Some(array) = sample.sample_attachments_array(false) else {
            return true;
        };
        // SAFETY: CFArray und NSArray sind gebührenfrei austauschbar.
        let array: &NSArray<NSDictionary<NSString, AnyObject>> =
            &*(CFRetained::as_ptr(&array).as_ptr() as *const NSArray<_>);
        let Some(info) = array.firstObject() else {
            return true;
        };
        let Some(status) = info.objectForKey(SCStreamFrameInfoStatus) else {
            return true;
        };
        let Ok(status) = status.downcast::<NSNumber>() else {
            return true;
        };
        let status = SCFrameStatus(status.integerValue());
        status == SCFrameStatus::Complete || status == SCFrameStatus::Started
    }
}

/// Lauteste Stelle eines Tonblocks (0.0 bis 1.0).
fn peak(sample: &CMSampleBuffer) -> f32 {
    unsafe {
        let Some(desc) = sample.format_description() else {
            return 0.0;
        };
        let asbd = CMAudioFormatDescriptionGetStreamBasicDescription(&desc);
        let Some(asbd) = asbd.as_ref() else {
            return 0.0;
        };
        let Some(block) = sample.data_buffer() else {
            return 0.0;
        };
        let mut len = 0usize;
        let mut ptr: *mut std::ffi::c_char = std::ptr::null_mut();
        if block.data_pointer(0, &mut len, std::ptr::null_mut(), &mut ptr) != 0 || ptr.is_null() {
            return 0.0;
        }
        let bytes = std::slice::from_raw_parts(ptr as *const u8, len);
        let is_float = asbd.mFormatFlags & AUDIO_FORMAT_FLAG_IS_FLOAT != 0;
        match (is_float, asbd.mBitsPerChannel) {
            (true, 32) => bytes
                .as_chunks::<4>()
                .0
                .iter()
                .map(|b| f32::from_ne_bytes(*b).abs())
                .fold(0.0, f32::max)
                .min(1.0),
            (false, 16) => {
                bytes
                    .as_chunks::<2>()
                    .0
                    .iter()
                    .map(|b| i16::from_ne_bytes(*b).unsigned_abs())
                    .max()
                    .unwrap_or(0) as f32
                    / 32768.0
            }
            (false, 32) => {
                bytes
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .map(|b| i32::from_ne_bytes(*b).unsigned_abs())
                    .max()
                    .unwrap_or(0) as f32
                    / 2_147_483_648.0
            }
            _ => 0.0,
        }
    }
}

fn on_sample(shared: &Shared, source: Source, sample: &CMSampleBuffer) {
    match source {
        Source::System => {
            shared
                .system_peak
                .fetch_max(peak(sample).to_bits(), Ordering::Relaxed);
        }
        Source::Mic => {
            shared
                .mic_peak
                .fetch_max(peak(sample).to_bits(), Ordering::Relaxed);
        }
        Source::Screen => {
            if !is_complete_frame(sample) {
                return;
            }
        }
    }
    let mut st = shared.lock();
    if matches!(source, Source::Screen) {
        // SAFETY: gültiger Puffer; wir behalten eine eigene Referenz.
        st.last_frame = Some(SendCf(unsafe { CFRetained::retain(NonNull::from(sample)) }));
    }
    if st.phase != Phase::Recording {
        return;
    }
    let pts = unsafe { sample.presentation_time_stamp() };
    if is_before(pts, st.t0) {
        return;
    }
    let offset = st.offset;
    let track = match source {
        Source::Screen => Some(&mut st.screen),
        Source::System => st.system.as_mut(),
        Source::Mic => st.mic.as_mut(),
    };
    let Some(track) = track else { return };
    let ok = if offset.value == 0 {
        track.append(sample)
    } else {
        shifted(sample, offset).is_some_and(|s| track.append(&s))
    };
    if ok {
        track.samples += 1;
    }
}

// --- Objective-C-Empfänger ------------------------------------------------------

struct OutputIvars {
    shared: Arc<Shared>,
}

define_class!(
    #[unsafe(super(NSObject))]
    #[name = "FMShotStreamOutput"]
    #[ivars = OutputIvars]
    struct StreamOutput;

    unsafe impl NSObjectProtocol for StreamOutput {}

    unsafe impl SCStreamOutput for StreamOutput {
        #[unsafe(method(stream:didOutputSampleBuffer:ofType:))]
        fn did_output(
            &self,
            _stream: &SCStream,
            sample: &CMSampleBuffer,
            kind: SCStreamOutputType,
        ) {
            let source = if kind == SCStreamOutputType::Screen {
                Source::Screen
            } else if kind == SCStreamOutputType::Audio {
                Source::System
            } else {
                return;
            };
            on_sample(&self.ivars().shared, source, sample);
        }
    }

    unsafe impl SCStreamDelegate for StreamOutput {
        #[unsafe(method(stream:didStopWithError:))]
        fn did_stop(&self, _stream: &SCStream, error: &NSError) {
            let mut st = self.ivars().shared.lock();
            if st.phase != Phase::Stopped {
                st.failure = Some(error.localizedDescription().to_string());
            }
        }
    }

    unsafe impl AVCaptureAudioDataOutputSampleBufferDelegate for StreamOutput {
        #[unsafe(method(captureOutput:didOutputSampleBuffer:fromConnection:))]
        fn did_capture(
            &self,
            _output: &AVCaptureOutput,
            sample: &CMSampleBuffer,
            _connection: &AVCaptureConnection,
        ) {
            on_sample(&self.ivars().shared, Source::Mic, sample);
        }
    }
);

impl StreamOutput {
    fn new(shared: Arc<Shared>) -> Retained<Self> {
        let this = Self::alloc().set_ivars(OutputIvars { shared });
        unsafe { msg_send![super(this), init] }
    }
}

// --- Aufnahme ---------------------------------------------------------------------

pub(crate) struct MacRecording {
    shared: Arc<Shared>,
    stream: SendRetained<SCStream>,
    mic_session: Option<SendRetained<AVCaptureSession>>,
    // Am Leben halten, solange der Stream läuft.
    _output: SendRetained<StreamOutput>,
    _queues: Vec<SendQueue>,
    size: (u32, u32),
}

struct SendQueue(#[allow(dead_code)] DispatchRetained<DispatchQueue>);
// SAFETY: Dispatch-Warteschlangen sind threadsicher.
unsafe impl Send for SendQueue {}
// SAFETY: wie oben.
unsafe impl Sync for SendQueue {}

pub(crate) fn start_recording(options: RecordOptions) -> Result<MacRecording> {
    if !CGPreflightScreenCaptureAccess() {
        return Err(MediaError::PermissionDenied);
    }
    let content = fetch_shareable_content()?;
    let own = own_app(&content);
    let (filter, width, height, source_rect) =
        make_filter(&content, own.as_deref(), options.target)?;
    let (width, height) = (even(width), even(height));
    let fps = options.fps.clamp(1, 60);

    let screen = video_track(&options.files.screen, width, height, fps)?;
    let system = match &options.files.system_audio {
        Some(p) => Some(audio_track(p, 2)?),
        None => None,
    };
    let mic = match &options.files.microphone {
        Some(p) => Some(audio_track(p, 1)?),
        None => None,
    };
    let shared = Arc::new(Shared {
        state: Mutex::new(State {
            phase: Phase::Warmup,
            t0: unsafe { kCMTimeZero },
            pause_started: unsafe { kCMTimeZero },
            offset: unsafe { kCMTimeZero },
            last_frame: None,
            screen,
            system,
            mic,
            failure: None,
        }),
        system_peak: AtomicU32::new(0),
        mic_peak: AtomicU32::new(0),
    });

    let config = unsafe { SCStreamConfiguration::new() };
    unsafe {
        config.setWidth(width as usize);
        config.setHeight(height as usize);
        config.setMinimumFrameInterval(CMTime::new(1, fps as i32));
        config.setPixelFormat(PIXEL_FORMAT_420V);
        config.setShowsCursor(options.show_cursor);
        config.setQueueDepth(6);
        if let Some(rect) = source_rect {
            config.setSourceRect(rect);
        }
        if options.files.system_audio.is_some() {
            config.setCapturesAudio(true);
            config.setExcludesCurrentProcessAudio(true);
            config.setSampleRate(SAMPLE_RATE as isize);
            config.setChannelCount(2);
        }
    }

    let output = StreamOutput::new(shared.clone());
    let stream = unsafe {
        SCStream::initWithFilter_configuration_delegate(
            SCStream::alloc(),
            &filter,
            &config,
            Some(ProtocolObject::from_ref(&*output)),
        )
    };
    let screen_queue = DispatchQueue::new("ch.forellmedia.fmshot.video", None);
    let audio_queue = DispatchQueue::new("ch.forellmedia.fmshot.audio", None);
    unsafe {
        stream
            .addStreamOutput_type_sampleHandlerQueue_error(
                ProtocolObject::from_ref(&*output),
                SCStreamOutputType::Screen,
                Some(&screen_queue),
            )
            .map_err(|e| backend(&e))?;
        if options.files.system_audio.is_some() {
            stream
                .addStreamOutput_type_sampleHandlerQueue_error(
                    ProtocolObject::from_ref(&*output),
                    SCStreamOutputType::Audio,
                    Some(&audio_queue),
                )
                .map_err(|e| backend(&e))?;
        }
    }

    let mic_queue = DispatchQueue::new("ch.forellmedia.fmshot.mic", None);
    let mic_session = if options.files.microphone.is_some() {
        Some(start_microphone(
            options.microphone_id.as_deref(),
            &output,
            &mic_queue,
        )?)
    } else {
        None
    };

    let (tx, rx) = mpsc::sync_channel(1);
    let block = RcBlock::new(move |error: *mut NSError| {
        let _ = tx.send(error_text_opt(error));
    });
    unsafe { stream.startCaptureWithCompletionHandler(Some(&block)) };
    match rx.recv_timeout(TIMEOUT) {
        Ok(None) => {}
        Ok(Some(e)) => {
            cancel_tracks(&shared);
            return Err(MediaError::Backend(e));
        }
        Err(_) => {
            cancel_tracks(&shared);
            return Err(MediaError::Backend(
                "ScreenCaptureKit antwortet nicht".into(),
            ));
        }
    }

    Ok(MacRecording {
        shared,
        stream: SendRetained(stream),
        mic_session: mic_session.map(SendRetained),
        _output: SendRetained(output),
        _queues: vec![
            SendQueue(screen_queue),
            SendQueue(audio_queue),
            SendQueue(mic_queue),
        ],
        size: (width, height),
    })
}

fn cancel_tracks(shared: &Shared) {
    let st = shared.lock();
    st.screen.cancel();
    if let Some(t) = &st.system {
        t.cancel();
    }
    if let Some(t) = &st.mic {
        t.cancel();
    }
}

impl MacRecording {
    /// Stoppt Stream und Mikrofon; danach kommen keine Proben mehr.
    fn stop_sources(&self) {
        let (tx, rx) = mpsc::sync_channel(1);
        let block = RcBlock::new(move |_error: *mut NSError| {
            let _ = tx.send(());
        });
        unsafe { self.stream.0.stopCaptureWithCompletionHandler(Some(&block)) };
        let _ = rx.recv_timeout(TIMEOUT);
        if let Some(session) = &self.mic_session {
            unsafe { session.0.stopRunning() };
        }
    }
}

impl Recording for MacRecording {
    fn begin(&self) {
        let mut st = self.shared.lock();
        if st.phase != Phase::Warmup {
            return;
        }
        let t0 = host_now();
        st.t0 = t0;
        st.phase = Phase::Recording;
        let tracks = [Some(&st.screen), st.system.as_ref(), st.mic.as_ref()];
        for track in tracks.into_iter().flatten() {
            unsafe { track.writer.0.startSessionAtSourceTime(t0) };
        }
        // Das zuletzt gesehene Bild wird das erste Bild des Videos.
        if let Some(frame) = st.last_frame.take() {
            let delta = unsafe { frame.0.presentation_time_stamp().subtract(t0) };
            if let Some(first) = shifted(&frame.0, delta)
                && st.screen.append(&first)
            {
                st.screen.samples += 1;
            }
            st.last_frame = Some(frame);
        }
    }

    fn pause(&self) {
        let mut st = self.shared.lock();
        if st.phase == Phase::Recording {
            st.phase = Phase::Paused;
            st.pause_started = host_now();
        }
    }

    fn resume(&self) {
        let mut st = self.shared.lock();
        if st.phase == Phase::Paused {
            let paused = unsafe { host_now().subtract(st.pause_started) };
            st.offset = unsafe { st.offset.add(paused) };
            st.phase = Phase::Recording;
        }
    }

    fn is_paused(&self) -> bool {
        self.shared.lock().phase == Phase::Paused
    }

    fn elapsed(&self) -> Duration {
        let st = self.shared.lock();
        let end = match st.phase {
            Phase::Warmup => return Duration::ZERO,
            Phase::Paused => st.pause_started,
            _ => host_now(),
        };
        let secs = unsafe { end.subtract(st.t0).subtract(st.offset).seconds() };
        Duration::from_secs_f64(secs.max(0.0))
    }

    fn levels(&self) -> Levels {
        Levels {
            system: f32::from_bits(self.shared.system_peak.swap(0, Ordering::Relaxed)),
            microphone: f32::from_bits(self.shared.mic_peak.swap(0, Ordering::Relaxed)),
        }
    }

    fn failure(&self) -> Option<String> {
        self.shared.lock().failure.clone()
    }

    fn stop(&self) -> Result<RecordSummary> {
        let duration = self.elapsed();
        let end = {
            let mut st = self.shared.lock();
            let end = match st.phase {
                Phase::Paused => st.pause_started,
                _ => host_now(),
            };
            let started = st.phase != Phase::Warmup;
            st.phase = Phase::Stopped;
            if !started {
                drop(st);
                self.stop_sources();
                cancel_tracks(&self.shared);
                return Err(MediaError::Empty);
            }
            end
        };
        self.stop_sources();
        let st = self.shared.lock();
        let end = unsafe { end.subtract(st.offset) };
        if !st.screen.finish(end).map_err(MediaError::Backend)? {
            return Err(MediaError::Empty);
        }
        for track in [st.system.as_ref(), st.mic.as_ref()].into_iter().flatten() {
            if let Err(e) = track.finish(end) {
                log::warn!("Tonspur {}: {e}", track.path.display());
            }
        }
        Ok(RecordSummary {
            duration,
            width: self.size.0,
            height: self.size.1,
        })
    }

    fn cancel(&self) {
        self.shared.lock().phase = Phase::Stopped;
        self.stop_sources();
        cancel_tracks(&self.shared);
    }
}

// --- Dateien ------------------------------------------------------------------------

fn file_url(path: &Path) -> Retained<NSURL> {
    NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()))
}

fn backend(e: &NSError) -> MediaError {
    MediaError::Backend(e.localizedDescription().to_string())
}

fn number(v: u32) -> Retained<NSNumber> {
    NSNumber::new_u32(v)
}

fn dict(pairs: &[(&NSString, &AnyObject)]) -> Retained<NSDictionary<NSString, AnyObject>> {
    let keys: Vec<&NSString> = pairs.iter().map(|(k, _)| *k).collect();
    let values: Vec<&AnyObject> = pairs.iter().map(|(_, v)| *v).collect();
    NSDictionary::from_slices(&keys, &values)
}

fn new_writer(path: &Path, file_type: &NSString) -> Result<Retained<AVAssetWriter>> {
    let _ = std::fs::remove_file(path);
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| MediaError::Backend(e.to_string()))?;
    }
    unsafe {
        AVAssetWriter::initWithURL_fileType_error(
            AVAssetWriter::alloc(),
            &file_url(path),
            file_type,
        )
        .map_err(|e| backend(&e))
    }
}

fn add_and_start(writer: &AVAssetWriter, input: &AVAssetWriterInput) -> Result<()> {
    unsafe {
        input.setExpectsMediaDataInRealTime(true);
        if !writer.canAddInput(input) {
            return Err(MediaError::Backend(
                "Spur kann nicht angelegt werden".into(),
            ));
        }
        writer.addInput(input);
        if !writer.startWriting() {
            return Err(writer.error().map(|e| backend(&e)).unwrap_or_else(|| {
                MediaError::Backend("Datei kann nicht geschrieben werden".into())
            }));
        }
    }
    Ok(())
}

fn video_track(path: &Path, width: u32, height: u32, fps: u32) -> Result<Track> {
    let file_type = unsafe { AVFileTypeMPEG4 }.ok_or(MediaError::NotImplemented)?;
    let writer = new_writer(path, file_type)?;
    // Alle 2 s ein Fragment: Nach einem Absturz bleibt das Video bis dahin lesbar.
    unsafe { writer.setMovieFragmentInterval(CMTime::new(2, 1)) };

    let key = |k: Option<&'static NSString>| k.ok_or(MediaError::NotImplemented);
    let (bitrate, fps_n, key_interval, no_reorder) = (
        number(screen_bitrate(width, height, fps)),
        number(fps),
        number(fps * 2),
        NSNumber::new_bool(false),
    );
    let compression = unsafe {
        dict(&[
            (key(AVVideoAverageBitRateKey)?, &bitrate),
            (key(AVVideoExpectedSourceFrameRateKey)?, &fps_n),
            (key(AVVideoMaxKeyFrameIntervalKey)?, &key_interval),
            (key(AVVideoAllowFrameReorderingKey)?, &no_reorder),
        ])
    };
    let codec = unsafe { AVVideoCodecTypeHEVC }.ok_or(MediaError::NotImplemented)?;
    let (w, h) = (number(width), number(height));
    let settings = unsafe {
        dict(&[
            (key(AVVideoCodecKey)?, codec),
            (key(AVVideoWidthKey)?, &w),
            (key(AVVideoHeightKey)?, &h),
            (key(AVVideoCompressionPropertiesKey)?, &compression),
        ])
    };
    let media = unsafe { AVMediaTypeVideo }.ok_or(MediaError::NotImplemented)?;
    let input = unsafe {
        AVAssetWriterInput::initWithMediaType_outputSettings(
            AVAssetWriterInput::alloc(),
            media,
            Some(&settings),
        )
    };
    add_and_start(&writer, &input)?;
    Ok(Track {
        writer: SendRetained(writer),
        input: SendRetained(input),
        path: path.to_path_buf(),
        samples: 0,
    })
}

fn audio_track(path: &Path, channels: u32) -> Result<Track> {
    let file_type = unsafe { AVFileTypeAppleM4A }.ok_or(MediaError::NotImplemented)?;
    let writer = new_writer(path, file_type)?;
    let (format, rate, ch, bitrate) = (
        number(AUDIO_FORMAT_AAC),
        number(SAMPLE_RATE),
        number(channels),
        number(if channels > 1 { 160_000 } else { 96_000 }),
    );
    // Die Schlüssel aus AVAudioSettings.h haben ihren Namen als Wert.
    let settings = dict(&[
        (&NSString::from_str("AVFormatIDKey"), &format),
        (&NSString::from_str("AVSampleRateKey"), &rate),
        (&NSString::from_str("AVNumberOfChannelsKey"), &ch),
        (&NSString::from_str("AVEncoderBitRateKey"), &bitrate),
    ]);
    let media = unsafe { AVMediaTypeAudio }.ok_or(MediaError::NotImplemented)?;
    let input = unsafe {
        AVAssetWriterInput::initWithMediaType_outputSettings(
            AVAssetWriterInput::alloc(),
            media,
            Some(&settings),
        )
    };
    add_and_start(&writer, &input)?;
    Ok(Track {
        writer: SendRetained(writer),
        input: SendRetained(input),
        path: path.to_path_buf(),
        samples: 0,
    })
}

// --- Mikrofon ---------------------------------------------------------------------

fn audio_media_type() -> Result<&'static NSString> {
    unsafe { AVMediaTypeAudio }.ok_or(MediaError::NotImplemented)
}

pub(crate) fn microphones() -> Vec<AudioDevice> {
    let Ok(media) = audio_media_type() else {
        return Vec::new();
    };
    unsafe { AVCaptureDevice::devicesWithMediaType(media) }
        .iter()
        .map(|d| AudioDevice {
            id: unsafe { d.uniqueID() }.to_string(),
            name: unsafe { d.localizedName() }.to_string(),
        })
        .collect()
}

pub(crate) fn microphone_permission() -> Permission {
    let Ok(media) = audio_media_type() else {
        return Permission::Denied;
    };
    match unsafe { AVCaptureDevice::authorizationStatusForMediaType(media) } {
        AVAuthorizationStatus::Authorized => Permission::Granted,
        AVAuthorizationStatus::NotDetermined => Permission::NotDetermined,
        _ => Permission::Denied,
    }
}

pub(crate) fn request_microphone_permission() -> bool {
    let Ok(media) = audio_media_type() else {
        return false;
    };
    let (tx, rx) = mpsc::sync_channel(1);
    let block = RcBlock::new(move |granted: Bool| {
        let _ = tx.send(granted.as_bool());
    });
    unsafe { AVCaptureDevice::requestAccessForMediaType_completionHandler(media, &block) };
    // Der Nutzer darf sich Zeit lassen.
    rx.recv_timeout(Duration::from_secs(300)).unwrap_or(false)
}

fn start_microphone(
    id: Option<&str>,
    output: &StreamOutput,
    queue: &DispatchQueue,
) -> Result<Retained<AVCaptureSession>> {
    let media = audio_media_type()?;
    let device = id
        .and_then(|id| unsafe { AVCaptureDevice::deviceWithUniqueID(&NSString::from_str(id)) })
        .or_else(|| unsafe { AVCaptureDevice::defaultDeviceWithMediaType(media) })
        .ok_or_else(|| MediaError::Backend("Kein Mikrofon gefunden".into()))?;
    let input = unsafe { AVCaptureDeviceInput::deviceInputWithDevice_error(&device) }
        .map_err(|e| backend(&e))?;
    let session = unsafe { AVCaptureSession::new() };
    let data = unsafe { AVCaptureAudioDataOutput::new() };
    unsafe {
        session.beginConfiguration();
        if !session.canAddInput(&input) || !session.canAddOutput(&data) {
            session.commitConfiguration();
            return Err(MediaError::Backend(
                "Mikrofon kann nicht verwendet werden".into(),
            ));
        }
        session.addInput(&input);
        data.setSampleBufferDelegate_queue(Some(ProtocolObject::from_ref(output)), Some(queue));
        session.addOutput(&data);
        session.commitConfiguration();
        session.startRunning();
    }
    Ok(session)
}

// --- Ziel -------------------------------------------------------------------------

type Filter = (Retained<SCContentFilter>, u32, u32, Option<CGRect>);

fn make_filter(
    content: &SCShareableContent,
    own: Option<&SCRunningApplication>,
    target: RecordTarget,
) -> Result<Filter> {
    let excluded = match own {
        Some(app) => NSArray::from_slice(&[app]),
        None => NSArray::new(),
    };
    let display_filter = |display: &SCDisplay| unsafe {
        SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(
            SCContentFilter::alloc(),
            display,
            &excluded,
            &NSArray::new(),
        )
    };
    let find_display = |id: u32| {
        unsafe { content.displays() }
            .iter()
            .find(|d| unsafe { d.displayID() } == id)
            .ok_or(MediaError::TargetNotFound)
    };
    match target {
        RecordTarget::Display { display_id } => {
            let display = find_display(display_id)?;
            let (w, h) = pixel_size(&display);
            Ok((display_filter(&display), w, h, None))
        }
        RecordTarget::Region {
            display_id,
            x,
            y,
            width,
            height,
        } => {
            let display = find_display(display_id)?;
            let scale = scale_of(&display);
            let rect = CGRect::new(
                CGPoint::new(x as f64 / scale, y as f64 / scale),
                CGSize::new(width as f64 / scale, height as f64 / scale),
            );
            Ok((display_filter(&display), width, height, Some(rect)))
        }
        RecordTarget::Window { window_id } => {
            let window = unsafe { content.windows() }
                .iter()
                .find(|w| unsafe { w.windowID() } == window_id)
                .ok_or(MediaError::TargetNotFound)?;
            let frame = unsafe { window.frame() };
            let center = (
                frame.origin.x + frame.size.width / 2.0,
                frame.origin.y + frame.size.height / 2.0,
            );
            let scale = unsafe { content.displays() }
                .iter()
                .find(|d| {
                    let f = unsafe { d.frame() };
                    center.0 >= f.origin.x
                        && center.1 >= f.origin.y
                        && center.0 < f.origin.x + f.size.width
                        && center.1 < f.origin.y + f.size.height
                })
                .map(|d| scale_of(&d))
                .unwrap_or(2.0);
            let filter = unsafe {
                SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), &window)
            };
            Ok((
                filter,
                (frame.size.width * scale).round() as u32,
                (frame.size.height * scale).round() as u32,
                None,
            ))
        }
    }
}

fn pixel_size(d: &SCDisplay) -> (u32, u32) {
    let id = unsafe { d.displayID() };
    let mode = CGDisplayCopyDisplayMode(id);
    let w = CGDisplayMode::pixel_width(mode.as_deref());
    let h = CGDisplayMode::pixel_height(mode.as_deref());
    if w > 0 && h > 0 {
        (w as u32, h as u32)
    } else {
        (unsafe { d.width() } as u32, unsafe { d.height() } as u32)
    }
}

fn scale_of(d: &SCDisplay) -> f64 {
    let frame = unsafe { d.frame() };
    pixel_size(d).0 as f64 / frame.size.width.max(1.0)
}

fn fetch_shareable_content() -> Result<Retained<SCShareableContent>> {
    let (tx, rx) = mpsc::sync_channel::<std::result::Result<SendRetained<_>, String>>(1);
    let block = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            // SAFETY: ScreenCaptureKit übergibt gültige Zeiger oder null.
            let result = match unsafe { Retained::retain(content) } {
                Some(content) => Ok(SendRetained(content)),
                None => Err(error_text_opt(error).unwrap_or_default()),
            };
            let _ = tx.send(result);
        },
    );
    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&block) };
    match rx.recv_timeout(TIMEOUT) {
        Ok(Ok(content)) => Ok(content.0),
        Ok(Err(e)) => Err(MediaError::Backend(e)),
        Err(_) => Err(MediaError::Backend(
            "ScreenCaptureKit antwortet nicht".into(),
        )),
    }
}

/// FMShot selbst (Aufnahmeleiste, Rahmen) gehört nie ins Video.
fn own_app(content: &SCShareableContent) -> Option<Retained<SCRunningApplication>> {
    let pid = std::process::id() as i32;
    unsafe { content.applications() }
        .iter()
        .find(|a| unsafe { a.processID() } == pid)
}

fn error_text_opt(error: *mut NSError) -> Option<String> {
    // SAFETY: null oder gültiges NSError.
    unsafe { Retained::retain(error) }.map(|e| e.localizedDescription().to_string())
}

// --- Zusammenführen und Einzelbilder -------------------------------------------------

pub(crate) fn export_mp4(files: &RecordFiles, output: &Path) -> Result<()> {
    let video = unsafe { AVMediaTypeVideo }.ok_or(MediaError::NotImplemented)?;
    let audio = audio_media_type()?;
    let composition = unsafe { AVMutableComposition::composition() };

    let screen = unsafe { AVURLAsset::URLAssetWithURL_options(&file_url(&files.screen), None) };
    let source = unsafe { screen.tracksWithMediaType(video) }
        .firstObject()
        .ok_or(MediaError::Empty)?;
    let range = unsafe { source.timeRange() };
    let track = unsafe { composition.addMutableTrackWithMediaType_preferredTrackID(video, 0) }
        .ok_or_else(|| MediaError::Backend("Videospur".into()))?;
    unsafe { track.insertTimeRange_ofTrack_atTime_error(range, &source, range.start) }
        .map_err(|e| backend(&e))?;

    for path in [&files.system_audio, &files.microphone]
        .into_iter()
        .flatten()
    {
        if !path.exists() {
            continue;
        }
        let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&file_url(path), None) };
        let Some(source) = unsafe { asset.tracksWithMediaType(audio) }.firstObject() else {
            continue;
        };
        let range = unsafe { source.timeRange() };
        let Some(track) =
            (unsafe { composition.addMutableTrackWithMediaType_preferredTrackID(audio, 0) })
        else {
            continue;
        };
        if let Err(e) =
            unsafe { track.insertTimeRange_ofTrack_atTime_error(range, &source, range.start) }
        {
            log::warn!("Tonspur {}: {}", path.display(), e.localizedDescription());
        }
    }

    let _ = std::fs::remove_file(output);
    let session = unsafe {
        AVAssetExportSession::initWithAsset_presetName(
            AVAssetExportSession::alloc(),
            &composition,
            AVAssetExportPresetHighestQuality,
        )
    }
    .ok_or_else(|| MediaError::Backend("Export nicht möglich".into()))?;
    let file_type = unsafe { AVFileTypeMPEG4 }.ok_or(MediaError::NotImplemented)?;
    unsafe {
        session.setOutputURL(Some(&file_url(output)));
        session.setOutputFileType(Some(file_type));
        session.setShouldOptimizeForNetworkUse(true);
        // Länge des Bildes – Ton darüber hinaus wird abgeschnitten.
        session.setTimeRange(CMTimeRange {
            start: kCMTimeZero,
            duration: range.start.add(range.duration),
        });
    }
    let (tx, rx) = mpsc::sync_channel(1);
    let block = RcBlock::new(move || {
        let _ = tx.send(());
    });
    unsafe { session.exportAsynchronouslyWithCompletionHandler(&block) };
    rx.recv()
        .map_err(|_| MediaError::Backend("Export abgebrochen".into()))?;
    match unsafe { session.status() } {
        AVAssetExportSessionStatus::Completed => Ok(()),
        _ => Err(unsafe { session.error() }
            .map(|e| backend(&e))
            .unwrap_or_else(|| MediaError::Backend("Export fehlgeschlagen".into()))),
    }
}

pub(crate) fn frame_at(video: &Path, at: Duration) -> Result<RgbaImage> {
    let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&file_url(video), None) };
    let generator =
        unsafe { AVAssetImageGenerator::initWithAsset(AVAssetImageGenerator::alloc(), &asset) };
    unsafe { generator.setAppliesPreferredTrackTransform(true) };
    let time = unsafe { CMTime::with_seconds(at.as_secs_f64(), 600) };
    let image = unsafe { generator.copyCGImageAtTime_actualTime_error(time, std::ptr::null_mut()) }
        .map_err(|e| backend(&e))?;
    cgimage_to_rgba(&image).map_err(MediaError::Backend)
}

fn cgimage_to_rgba(image: &CGImage) -> std::result::Result<RgbaImage, String> {
    let width = CGImage::width(Some(image));
    let height = CGImage::height(Some(image));
    let bytes_per_row = width * 4;
    let mut data = vec![0u8; bytes_per_row * height];
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))
        .ok_or("sRGB-Farbraum nicht verfügbar")?;
    let info = CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0;
    // SAFETY: `data` lebt länger als der Kontext und ist gross genug.
    let ctx = unsafe {
        CGBitmapContextCreate(
            data.as_mut_ptr() as *mut c_void,
            width,
            height,
            8,
            bytes_per_row,
            Some(&space),
            info,
        )
    }
    .ok_or("Bitmap-Kontext konnte nicht angelegt werden")?;
    CGContext::draw_image(
        Some(&ctx),
        CGRect::new(
            CGPoint::new(0.0, 0.0),
            CGSize::new(width as f64, height as f64),
        ),
        Some(image),
    );
    drop(ctx);
    // Videobilder sind deckend – kein Zurückrechnen der Vormultiplikation nötig.
    RgbaImage::new(width as u32, height as u32, data).map_err(|e| e.to_string())
}
