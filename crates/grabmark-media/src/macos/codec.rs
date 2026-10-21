//! Export unter macOS: Spuren dekodieren (AVAssetReader, BGRA) und das
//! zusammengesetzte Video kodieren (AVAssetWriter, H.264 in Hardware). Die
//! Tonspuren mischt AVAssetReaderAudioMixOutput; sie laufen auf einer
//! eigenen Warteschlange neben den Bildern her, damit der Writer beide
//! verzahnen kann.

use std::path::{Path, PathBuf};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use block2::RcBlock;
use dispatch2::DispatchQueue;
use grabmark_core::{TimeRange, VideoCodec, VideoEncoding};
use grabmark_video::{FrameSink, FrameSource, VideoFrame};
use objc2::AllocAnyThread;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_av_foundation::{
    AVAssetReader, AVAssetReaderAudioMixOutput, AVAssetReaderStatus, AVAssetReaderTrackOutput,
    AVAssetTrack, AVAssetWriter, AVAssetWriterInput, AVAssetWriterInputPixelBufferAdaptor,
    AVAssetWriterStatus, AVAudioMixInputParameters, AVFileTypeMPEG4, AVMutableAudioMix,
    AVMutableAudioMixInputParameters, AVMutableComposition, AVURLAsset, AVVideoAverageBitRateKey,
    AVVideoCodecKey, AVVideoCodecTypeH264, AVVideoCodecTypeHEVC, AVVideoCompressionPropertiesKey,
    AVVideoExpectedSourceFrameRateKey, AVVideoHeightKey, AVVideoMaxKeyFrameIntervalKey,
    AVVideoProfileLevelH264HighAutoLevel, AVVideoProfileLevelKey, AVVideoWidthKey,
};
use objc2_core_foundation::CFRetained;
use objc2_core_media::{CMTime, CMTimeRange, kCMTimeZero};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
    CVPixelBufferGetHeight, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
    CVPixelBufferLockFlags, CVPixelBufferPool, CVPixelBufferUnlockBaseAddress,
    kCVPixelBufferHeightKey, kCVPixelBufferPixelFormatTypeKey, kCVPixelBufferWidthKey,
    kCVPixelFormatType_32BGRA,
};
use objc2_foundation::{NSArray, NSNumber, NSString};

use super::{
    AUDIO_FORMAT_AAC, FINISH_TIMEOUT, SAMPLE_RATE, SendRetained, audio_media_type, backend, cf_key,
    dict, file_url, number, video_media_type,
};
use crate::{AudioInput, MediaError, Result, export_bitrate};

/// `lpcm`
const AUDIO_FORMAT_LPCM: u32 = 0x6C70_636D;
/// Zeitbasis der geschriebenen Bilder (teilbar durch 24, 25, 30, 60).
const TIMESCALE: i32 = 90_000;
/// So lange darf der Encoder brauchen, bis er ein Bild annimmt.
const ENCODER_TIMEOUT: Duration = Duration::from_secs(20);

fn read_only() -> CVPixelBufferLockFlags {
    CVPixelBufferLockFlags(1)
}

// --- Lesen -------------------------------------------------------------------------

/// Liest eine Videospur Bild für Bild als BGRA.
pub(crate) struct MacFrames {
    reader: SendRetained<AVAssetReader>,
    output: SendRetained<AVAssetReaderTrackOutput>,
    size: (u32, u32),
}

pub(crate) fn open_video(path: &Path) -> Result<MacFrames> {
    let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&file_url(path), None) };
    let track = unsafe { asset.tracksWithMediaType(video_media_type()?) }
        .firstObject()
        .ok_or(MediaError::Empty)?;
    let natural = unsafe { track.naturalSize() };
    let reader =
        unsafe { AVAssetReader::assetReaderWithAsset_error(&asset) }.map_err(|e| backend(&e))?;
    let format = NSNumber::new_u32(kCVPixelFormatType_32BGRA);
    let settings = unsafe { dict(&[(cf_key(kCVPixelBufferPixelFormatTypeKey), &format)]) };
    let output = unsafe {
        AVAssetReaderTrackOutput::assetReaderTrackOutputWithTrack_outputSettings(
            &track,
            Some(&settings),
        )
    };
    unsafe {
        // Die Bilder werden ohnehin kopiert.
        output.setAlwaysCopiesSampleData(false);
        if !reader.canAddOutput(&output) {
            return Err(MediaError::Backend(
                "Video kann nicht gelesen werden".into(),
            ));
        }
        reader.addOutput(&output);
        if !reader.startReading() {
            return Err(reader
                .error()
                .map(|e| backend(&e))
                .unwrap_or(MediaError::Empty));
        }
    }
    Ok(MacFrames {
        reader: SendRetained(reader),
        output: SendRetained(output),
        size: (
            natural.width.round().max(1.0) as u32,
            natural.height.round().max(1.0) as u32,
        ),
    })
}

impl FrameSource for MacFrames {
    fn size(&self) -> (u32, u32) {
        self.size
    }

    fn next_frame(&mut self) -> std::result::Result<Option<VideoFrame>, String> {
        loop {
            let Some(sample) = (unsafe { self.output.0.copyNextSampleBuffer() }) else {
                return match unsafe { self.reader.0.status() } {
                    AVAssetReaderStatus::Failed => Err(unsafe { self.reader.0.error() }
                        .map(|e| e.localizedDescription().to_string())
                        .unwrap_or_else(|| "Video kann nicht gelesen werden".into())),
                    _ => Ok(None),
                };
            };
            // Proben ohne Bild (z. B. Leerstellen) überspringen.
            let Some(image) = (unsafe { sample.image_buffer() }) else {
                continue;
            };
            let pts = unsafe { sample.presentation_time_stamp().seconds() };
            return copy_frame(&image, Duration::from_secs_f64(pts.max(0.0))).map(Some);
        }
    }
}

fn copy_frame(image: &CVPixelBuffer, pts: Duration) -> std::result::Result<VideoFrame, String> {
    if unsafe { CVPixelBufferLockBaseAddress(image, read_only()) } != 0 {
        return Err("Bild kann nicht gelesen werden".into());
    }
    let width = CVPixelBufferGetWidth(image);
    let height = CVPixelBufferGetHeight(image);
    let stride = CVPixelBufferGetBytesPerRow(image);
    let base = CVPixelBufferGetBaseAddress(image) as *const u8;
    let data = if base.is_null() {
        Vec::new()
    } else {
        // SAFETY: gesperrter Puffer mit `height` Zeilen à `stride` Bytes.
        unsafe { std::slice::from_raw_parts(base, stride * height) }.to_vec()
    };
    unsafe { CVPixelBufferUnlockBaseAddress(image, read_only()) };
    if data.is_empty() {
        return Err("Bild ohne Inhalt".into());
    }
    Ok(VideoFrame {
        pts,
        width: width as u32,
        height: height as u32,
        stride,
        data,
    })
}

// --- Schreiben -----------------------------------------------------------------------

/// Gemeinsamer Zustand der Ton-Pumpe (läuft auf ihrer Warteschlange).
struct Pump {
    reader: SendRetained<AVAssetReader>,
    output: SendRetained<AVAssetReaderAudioMixOutput>,
    input: SendRetained<AVAssetWriterInput>,
    finished: AtomicBool,
    done: Mutex<Option<mpsc::SyncSender<()>>>,
}

impl Pump {
    fn finish(&self) {
        if self.finished.swap(true, Ordering::AcqRel) {
            return;
        }
        unsafe { self.input.0.markAsFinished() };
        if let Some(tx) = self.done.lock().unwrap_or_else(|e| e.into_inner()).take() {
            let _ = tx.send(());
        }
    }

    /// Gibt Ton weiter, solange der Writer ihn annimmt.
    fn run(&self) {
        if self.finished.load(Ordering::Acquire) {
            return;
        }
        while unsafe { self.input.0.isReadyForMoreMediaData() } {
            match unsafe { self.output.0.copyNextSampleBuffer() } {
                Some(sample) => {
                    if !unsafe { self.input.0.appendSampleBuffer(&sample) } {
                        self.finish();
                        return;
                    }
                }
                None => {
                    if unsafe { self.reader.0.status() } == AVAssetReaderStatus::Failed {
                        log::warn!("Ton lesen fehlgeschlagen");
                    }
                    self.finish();
                    return;
                }
            }
        }
    }
}

/// Schreibt das fertige Video (H.264 + AAC).
pub(crate) struct MacMp4 {
    writer: SendRetained<AVAssetWriter>,
    input: SendRetained<AVAssetWriterInput>,
    adaptor: SendRetained<AVAssetWriterInputPixelBufferAdaptor>,
    pump: Option<(Arc<Pump>, mpsc::Receiver<()>)>,
    _queue: super::SendQueue,
    path: PathBuf,
    size: (u32, u32),
    duration: Duration,
    finished: bool,
}

fn cm_time(t: Duration) -> CMTime {
    let value = (t.as_nanos() * TIMESCALE as u128 / 1_000_000_000) as i64;
    unsafe { CMTime::new(value, TIMESCALE) }
}

pub(crate) fn create_mp4(
    path: &Path,
    size: (u32, u32),
    fps: u32,
    encoding: &VideoEncoding,
    audio: &[AudioInput],
    kept: &[TimeRange],
    duration: Duration,
) -> Result<MacMp4> {
    let file_type = unsafe { AVFileTypeMPEG4 }.ok_or(MediaError::NotImplemented)?;
    let writer = super::new_writer(path, file_type)?;
    unsafe { writer.setShouldOptimizeForNetworkUse(true) };
    let (width, height) = size;
    let fps = fps.clamp(1, 120);

    let key = |k: Option<&'static NSString>| k.ok_or(MediaError::NotImplemented);
    let (bitrate, fps_n, key_interval) = (
        number(export_bitrate(width, height, fps, encoding)),
        number(fps),
        number(fps * 2),
    );
    let mut properties: Vec<(&NSString, &AnyObject)> = vec![
        (key(unsafe { AVVideoAverageBitRateKey })?, &bitrate),
        (key(unsafe { AVVideoExpectedSourceFrameRateKey })?, &fps_n),
        (
            key(unsafe { AVVideoMaxKeyFrameIntervalKey })?,
            &key_interval,
        ),
    ];
    // HEVC wählt sein Profil selbst.
    let codec = match encoding.codec {
        VideoCodec::H264 => {
            let profile = unsafe { AVVideoProfileLevelH264HighAutoLevel }
                .ok_or(MediaError::NotImplemented)?;
            properties.push((key(unsafe { AVVideoProfileLevelKey })?, profile));
            unsafe { AVVideoCodecTypeH264 }
        }
        VideoCodec::Hevc => unsafe { AVVideoCodecTypeHEVC },
    }
    .ok_or(MediaError::NotImplemented)?;
    let compression = dict(&properties);
    let (w, h) = (number(width), number(height));
    let settings = unsafe {
        dict(&[
            (key(AVVideoCodecKey)?, codec),
            (key(AVVideoWidthKey)?, &w),
            (key(AVVideoHeightKey)?, &h),
            (key(AVVideoCompressionPropertiesKey)?, &compression),
        ])
    };
    let input = unsafe {
        AVAssetWriterInput::initWithMediaType_outputSettings(
            AVAssetWriterInput::alloc(),
            video_media_type()?,
            Some(&settings),
        )
    };
    let format = NSNumber::new_u32(kCVPixelFormatType_32BGRA);
    let attributes = unsafe {
        dict(&[
            (cf_key(kCVPixelBufferPixelFormatTypeKey), &format),
            (cf_key(kCVPixelBufferWidthKey), &w),
            (cf_key(kCVPixelBufferHeightKey), &h),
        ])
    };
    let adaptor = unsafe {
        AVAssetWriterInputPixelBufferAdaptor::assetWriterInputPixelBufferAdaptorWithAssetWriterInput_sourcePixelBufferAttributes(
            &input,
            Some(&attributes),
        )
    };
    unsafe {
        input.setExpectsMediaDataInRealTime(false);
        if !writer.canAddInput(&input) {
            return Err(MediaError::Backend(
                "Videospur kann nicht angelegt werden".into(),
            ));
        }
        writer.addInput(&input);
    }

    let audio_parts = audio_reader(audio, kept, duration)?;
    let audio_input = match &audio_parts {
        Some(_) => {
            let input = audio_input()?;
            unsafe {
                input.setExpectsMediaDataInRealTime(false);
                if writer.canAddInput(&input) {
                    writer.addInput(&input);
                    Some(input)
                } else {
                    None
                }
            }
        }
        None => None,
    };

    unsafe {
        if !writer.startWriting() {
            return Err(writer.error().map(|e| backend(&e)).unwrap_or_else(|| {
                MediaError::Backend("Datei kann nicht geschrieben werden".into())
            }));
        }
        writer.startSessionAtSourceTime(kCMTimeZero);
    }

    let queue = DispatchQueue::new("ch.forellmedia.fmshot.export-audio", None);
    let pump = match (audio_parts, audio_input) {
        (Some((reader, output)), Some(input)) if unsafe { reader.startReading() } => {
            let (tx, rx) = mpsc::sync_channel(1);
            let pump = Arc::new(Pump {
                reader: SendRetained(reader),
                output: SendRetained(output),
                input: SendRetained(input),
                finished: AtomicBool::new(false),
                done: Mutex::new(Some(tx)),
            });
            let p = pump.clone();
            let block = RcBlock::new(move || p.run());
            unsafe {
                pump.input
                    .0
                    .requestMediaDataWhenReadyOnQueue_usingBlock(&queue, &block)
            };
            Some((pump, rx))
        }
        (_, Some(input)) => {
            // Ton liess sich nicht lesen: leere Spur abschliessen.
            log::warn!("Ton für den Export nicht lesbar");
            unsafe { input.markAsFinished() };
            None
        }
        _ => None,
    };

    Ok(MacMp4 {
        writer: SendRetained(writer),
        input: SendRetained(input),
        adaptor: SendRetained(adaptor),
        pump,
        _queue: super::SendQueue(queue),
        path: path.to_path_buf(),
        size,
        duration,
        finished: false,
    })
}

type AudioParts = (
    Retained<AVAssetReader>,
    Retained<AVAssetReaderAudioMixOutput>,
);

fn ms(t: CMTime) -> f64 {
    unsafe { t.seconds() * 1000.0 }
}

fn cm_ms(v: f64) -> CMTime {
    cm_time(Duration::from_secs_f64(v.max(0.0) / 1000.0))
}

/// Alle Tonspuren in eine Komposition und gemischt lesen: die bleibenden
/// Stücke hintereinander, jede Spur mit ihrer Lautstärke.
fn audio_reader(
    files: &[AudioInput],
    kept: &[TimeRange],
    duration: Duration,
) -> Result<Option<AudioParts>> {
    let media = audio_media_type()?;
    let composition = unsafe { AVMutableComposition::composition() };
    let whole = [TimeRange {
        start_ms: 0,
        end_ms: duration.as_millis().min(u32::MAX as u128) as u32,
    }];
    let pieces = if kept.is_empty() { &whole[..] } else { kept };
    let mut volumes = Vec::new();
    for input in files.iter().filter(|i| i.path.exists() && i.volume > 0.0) {
        let asset = unsafe { AVURLAsset::URLAssetWithURL_options(&file_url(&input.path), None) };
        let Some(source) = unsafe { asset.tracksWithMediaType(media) }.firstObject() else {
            continue;
        };
        let range = unsafe { source.timeRange() };
        let (src_start, src_end) = (ms(range.start), ms(range.start) + ms(range.duration));
        let Some(track) =
            (unsafe { composition.addMutableTrackWithMediaType_preferredTrackID(media, 0) })
        else {
            continue;
        };
        let mut out = 0.0;
        let mut inserted = false;
        for piece in pieces {
            let (a, b) = (piece.start_ms as f64, piece.end_ms as f64);
            let (s, e) = (a.max(src_start), b.min(src_end));
            if e > s {
                let r = CMTimeRange {
                    start: cm_ms(s),
                    duration: cm_ms(e - s),
                };
                match unsafe {
                    track.insertTimeRange_ofTrack_atTime_error(r, &source, cm_ms(out + s - a))
                } {
                    Ok(()) => inserted = true,
                    Err(e) => log::warn!(
                        "Tonspur {}: {}",
                        input.path.display(),
                        e.localizedDescription()
                    ),
                }
            }
            out += b - a;
        }
        if inserted {
            volumes.push((unsafe { track.trackID() }, input.volume.clamp(0.0, 1.0)));
        }
    }
    if volumes.is_empty() {
        return Ok(None);
    }
    let tracks: Vec<Retained<AVAssetTrack>> = unsafe { composition.tracksWithMediaType(media) }
        .iter()
        .map(|t| Retained::into_super(Retained::into_super(t)))
        .collect();
    let tracks = NSArray::from_retained_slice(&tracks);
    let (format, rate, channels, bits, no) = (
        number(AUDIO_FORMAT_LPCM),
        number(SAMPLE_RATE),
        number(2),
        number(16),
        NSNumber::new_bool(false),
    );
    let s = NSString::from_str;
    let settings = dict(&[
        (&s("AVFormatIDKey"), &format),
        (&s("AVSampleRateKey"), &rate),
        (&s("AVNumberOfChannelsKey"), &channels),
        (&s("AVLinearPCMBitDepthKey"), &bits),
        (&s("AVLinearPCMIsFloatKey"), &no),
        (&s("AVLinearPCMIsBigEndianKey"), &no),
        (&s("AVLinearPCMIsNonInterleaved"), &no),
    ]);
    let reader = unsafe { AVAssetReader::assetReaderWithAsset_error(&composition) }
        .map_err(|e| backend(&e))?;
    let output = unsafe {
        AVAssetReaderAudioMixOutput::assetReaderAudioMixOutputWithAudioTracks_audioSettings(
            &tracks,
            Some(&settings),
        )
    };
    unsafe {
        output.setAlwaysCopiesSampleData(false);
        if !reader.canAddOutput(&output) {
            return Ok(None);
        }
        reader.addOutput(&output);
        reader.setTimeRange(CMTimeRange {
            start: kCMTimeZero,
            duration: cm_time(duration),
        });
        // Lautstärke je Spur.
        let parameters: Vec<Retained<AVAudioMixInputParameters>> = volumes
            .iter()
            .map(|&(id, volume)| {
                let p = AVMutableAudioMixInputParameters::audioMixInputParameters();
                p.setTrackID(id);
                p.setVolume_atTime(volume, kCMTimeZero);
                Retained::into_super(p)
            })
            .collect();
        let mix = AVMutableAudioMix::audioMix();
        mix.setInputParameters(&NSArray::from_retained_slice(&parameters));
        output.setAudioMix(Some(&mix));
    }
    Ok(Some((reader, output)))
}

fn audio_input() -> Result<Retained<AVAssetWriterInput>> {
    let (format, rate, ch, bitrate) = (
        number(AUDIO_FORMAT_AAC),
        number(SAMPLE_RATE),
        number(2),
        number(160_000),
    );
    let s = NSString::from_str;
    let settings = dict(&[
        (&s("AVFormatIDKey"), &format),
        (&s("AVSampleRateKey"), &rate),
        (&s("AVNumberOfChannelsKey"), &ch),
        (&s("AVEncoderBitRateKey"), &bitrate),
    ]);
    Ok(unsafe {
        AVAssetWriterInput::initWithMediaType_outputSettings(
            AVAssetWriterInput::alloc(),
            audio_media_type()?,
            Some(&settings),
        )
    })
}

impl MacMp4 {
    fn writer_error(&self) -> String {
        unsafe { self.writer.0.error() }
            .map(|e| e.localizedDescription().to_string())
            .unwrap_or_else(|| "Video kann nicht geschrieben werden".into())
    }

    fn pixel_buffer(&self) -> std::result::Result<CFRetained<CVPixelBuffer>, String> {
        let pool =
            unsafe { self.adaptor.0.pixelBufferPool() }.ok_or_else(|| self.writer_error())?;
        let mut out: *mut CVPixelBuffer = std::ptr::null_mut();
        let status =
            unsafe { CVPixelBufferPool::create_pixel_buffer(None, &pool, NonNull::from(&mut out)) };
        match NonNull::new(out) {
            // SAFETY: „Create“ übergibt eine eigene Referenz.
            Some(p) if status == 0 => Ok(unsafe { CFRetained::from_raw(p) }),
            _ => Err(format!("Bildpuffer fehlt ({status})")),
        }
    }
}

impl FrameSink for MacMp4 {
    fn push(
        &mut self,
        bgra: &[u8],
        stride: usize,
        pts: Duration,
    ) -> std::result::Result<(), String> {
        let started = Instant::now();
        while !unsafe { self.input.0.isReadyForMoreMediaData() } {
            if unsafe { self.writer.0.status() } == AVAssetWriterStatus::Failed {
                return Err(self.writer_error());
            }
            if started.elapsed() > ENCODER_TIMEOUT {
                return Err("Encoder antwortet nicht".into());
            }
            std::thread::sleep(Duration::from_micros(300));
        }
        let buffer = self.pixel_buffer()?;
        if unsafe { CVPixelBufferLockBaseAddress(&buffer, CVPixelBufferLockFlags(0)) } != 0 {
            return Err("Bildpuffer gesperrt".into());
        }
        let base = CVPixelBufferGetBaseAddress(&buffer) as *mut u8;
        let row = CVPixelBufferGetBytesPerRow(&buffer);
        let (w, h) = (self.size.0 as usize, self.size.1 as usize);
        if !base.is_null() {
            for y in 0..h.min(bgra.len() / stride.max(1)) {
                let src = &bgra[y * stride..y * stride + w * 4];
                // SAFETY: Der Puffer hat `h` Zeilen à `row` ≥ `w * 4` Bytes.
                unsafe {
                    std::ptr::copy_nonoverlapping(src.as_ptr(), base.add(y * row), w * 4);
                }
            }
        }
        unsafe { CVPixelBufferUnlockBaseAddress(&buffer, CVPixelBufferLockFlags(0)) };
        if unsafe {
            self.adaptor
                .0
                .appendPixelBuffer_withPresentationTime(&buffer, cm_time(pts))
        } {
            Ok(())
        } else {
            Err(self.writer_error())
        }
    }

    fn finish(&mut self) -> std::result::Result<(), String> {
        self.finished = true;
        unsafe { self.input.0.markAsFinished() };
        if let Some((pump, rx)) = &self.pump
            && rx.recv_timeout(FINISH_TIMEOUT).is_err()
        {
            log::warn!("Ton wurde nicht fertig");
            pump.finish();
        }
        let (tx, rx) = mpsc::sync_channel(1);
        let block = RcBlock::new(move || {
            let _ = tx.send(());
        });
        unsafe {
            self.writer.0.endSessionAtSourceTime(cm_time(self.duration));
            self.writer.0.finishWritingWithCompletionHandler(&block);
        }
        rx.recv_timeout(FINISH_TIMEOUT)
            .map_err(|_| "Datei abschliessen dauert zu lange".to_string())?;
        if unsafe { self.writer.0.status() } == AVAssetWriterStatus::Completed {
            Ok(())
        } else {
            Err(self.writer_error())
        }
    }

    fn cancel(&mut self) {
        if self.finished {
            return;
        }
        self.finished = true;
        if let Some((pump, _)) = &self.pump {
            pump.finish();
            unsafe { pump.reader.0.cancelReading() };
        }
        unsafe { self.writer.0.cancelWriting() };
        let _ = std::fs::remove_file(&self.path);
    }
}

impl Drop for MacMp4 {
    fn drop(&mut self) {
        self.cancel();
    }
}
