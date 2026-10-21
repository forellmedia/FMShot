//! Video und Ton (ADR 0011): Bildschirm als Video aufnehmen, Systemton,
//! Mikrofon und Webcam, Dekodieren und Kodieren für den Export.
//!
//! Kodiert wird immer mit den Bordmitteln des Systems (Hardware), nie mit
//! FFmpeg (LGPL/GPL, Regel 5):
//! - macOS: ScreenCaptureKit-Stream → AVAssetWriter (HEVC/AAC); Mikrofon und
//!   Webcam über AVFoundation, damit alle Spuren dieselbe Systemuhr haben;
//!   Export: AVAssetReader → `grabmark-video` (wgpu) → AVAssetWriter
//! - Windows: Media Foundation – folgt
//! - Linux: PipeWire + openh264 – folgt
//!
//! Die Bilder des Bildschirms gehen ohne Kopie direkt zum Encoder. Darum
//! liegt die Video-Aufnahme hier und nicht in `grabmark-capture`, das nur
//! Einzelbilder liefert.

use std::ffi::c_void;
use std::path::{Path, PathBuf};
use std::time::Duration;

use grabmark_core::{RgbaImage, VideoCodec, VideoEncoding, VideoQuality, WebcamLayout};

#[cfg(target_os = "macos")]
mod macos;

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("Video-Aufnahme ist auf dieser Plattform noch nicht umgesetzt")]
    NotImplemented,
    #[error("Keine Freigabe für die Bildschirmaufnahme")]
    PermissionDenied,
    #[error("Aufnahmeziel nicht gefunden")]
    TargetNotFound,
    #[error("Es wurde kein Bild aufgenommen")]
    Empty,
    #[error("{0}")]
    Backend(String),
}

pub type Result<T> = std::result::Result<T, MediaError>;

/// Was aufgenommen wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordTarget {
    /// Ganzer Monitor.
    Display { display_id: u32 },
    /// Ausschnitt eines Monitors in physischen Pixeln relativ zum Monitor.
    Region {
        display_id: u32,
        x: u32,
        y: u32,
        width: u32,
        height: u32,
    },
    /// Ein einzelnes Fenster (folgt ihm, auch wenn es verdeckt wird).
    Window { window_id: u32 },
}

/// Wohin die Spuren geschrieben werden. Fehlt ein Pfad, wird die Spur nicht
/// aufgenommen.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordFiles {
    /// Bildschirm als Video (MP4, HEVC); `None` bei „nur Webcam“.
    pub screen: Option<PathBuf>,
    /// Ton des Systems (M4A, AAC).
    pub system_audio: Option<PathBuf>,
    /// Mikrofon (M4A, AAC).
    pub microphone: Option<PathBuf>,
    /// Webcam (MP4, HEVC). Wird nur geschrieben, wenn die Webcam während
    /// der Aufnahme einmal eingeschaltet war.
    pub webcam: Option<PathBuf>,
    /// Erkannte Gesichter der Webcam (`<id>_faces.json`, [`FaceTrack`]),
    /// etwa 10 je Sekunde – für den Kopfausschnitt (Etappe 3).
    ///
    /// [`FaceTrack`]: grabmark_core::FaceTrack
    pub faces: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordOptions {
    /// `None` = nur Webcam: kein Bildschirm, kein Systemton. Die Kamera
    /// wird mit [`Recording::set_webcam`] eingeschaltet, und zwar vor
    /// [`Recording::begin`].
    pub target: Option<RecordTarget>,
    pub files: RecordFiles,
    /// Bilder pro Sekunde (höchstens; ein ruhiger Bildschirm liefert weniger).
    pub fps: u32,
    /// `None` = Standard-Mikrofon des Systems.
    pub microphone_id: Option<String>,
    /// Mauszeiger mit aufnehmen. Ab Etappe 5 zeichnet Grabmark ihn selbst.
    pub show_cursor: bool,
    /// `None` = Standard-Kamera des Systems. Eingeschaltet wird sie mit
    /// [`Recording::set_webcam`] (braucht `files.webcam`).
    pub camera_id: Option<String>,
}

/// Lautstärke-Spitzen seit der letzten Abfrage, 0.0 bis 1.0.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct Levels {
    pub system: f32,
    pub microphone: f32,
}

/// Ergebnis einer beendeten Aufnahme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct RecordSummary {
    /// Länge ohne Pausen.
    pub duration: Duration,
    /// Grösse des Videos in Pixeln.
    pub width: u32,
    pub height: u32,
}

/// Eine laufende Aufnahme. Läuft nach [`start_recording`] schon (für eine
/// kurze Anlaufzeit), schreibt aber erst nach [`Recording::begin`].
pub trait Recording: Send + Sync {
    /// Ab jetzt schreiben (nach dem Countdown).
    fn begin(&self);
    fn pause(&self);
    fn resume(&self);
    fn is_paused(&self) -> bool;
    /// Aufgenommene Zeit ohne Pausen.
    fn elapsed(&self) -> Duration;
    /// Pegel seit dem letzten Aufruf (setzt sie zurück).
    fn levels(&self) -> Levels;
    /// Ein Fehler, der die Aufnahme beendet hat (z. B. Fenster geschlossen).
    fn failure(&self) -> Option<String>;
    /// Beendet die Aufnahme und schliesst die Dateien ab (blockiert kurz).
    fn stop(&self) -> Result<RecordSummary>;
    /// Bricht ab und löscht die Dateien.
    fn cancel(&self);
    /// Webcam ein- oder ausschalten. Blockiert, bis die Kamera läuft bzw.
    /// steht (bis etwa 1 s) – nicht im Haupt-Thread aufrufen.
    fn set_webcam(&self, on: bool) -> Result<()>;
    fn webcam_on(&self) -> bool;
    /// Live-Bild der Webcam für die Blase; `None`, solange die Webcam nie
    /// eingeschaltet war. Zeigt es wie `layout` (Spiegeln, Kopfausschnitt,
    /// Hintergrund). Im Haupt-Thread aufrufen.
    fn webcam_preview(&self, layout: &WebcamLayout) -> Option<PreviewLayer>;
}

/// Live-Bild der Webcam als Ebene des Systems (macOS: `CALayer`), das die
/// Blase in `grabmark-overlay` anzeigt.
pub struct PreviewLayer {
    #[cfg(target_os = "macos")]
    pub(crate) layer: macos::SendLayer,
}

impl PreviewLayer {
    /// Zeiger auf die Ebene (macOS: `CALayer *`), gültig, solange `self` lebt.
    pub fn as_ptr(&self) -> *mut c_void {
        #[cfg(target_os = "macos")]
        return self.layer.as_ptr();
        #[cfg(not(target_os = "macos"))]
        std::ptr::null_mut()
    }
}

/// Startet eine Aufnahme. Blockiert, bis der Bildschirm-Stream läuft.
pub fn start_recording(options: RecordOptions) -> Result<Box<dyn Recording>> {
    #[cfg(target_os = "macos")]
    return macos::start_recording(options).map(|r| Box::new(r) as Box<dyn Recording>);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = options;
        Err(MediaError::NotImplemented)
    }
}

/// Ein Mikrofon oder eine Kamera des Systems.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Device {
    pub id: String,
    pub name: String,
}

pub type AudioDevice = Device;

pub fn microphones() -> Vec<Device> {
    #[cfg(target_os = "macos")]
    return macos::microphones();
    #[cfg(not(target_os = "macos"))]
    Vec::new()
}

pub fn cameras() -> Vec<Device> {
    #[cfg(target_os = "macos")]
    return macos::cameras();
    #[cfg(not(target_os = "macos"))]
    Vec::new()
}

/// Stand einer Freigabe (Mikrofon, Kamera).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Permission {
    Granted,
    /// Noch nie gefragt – das System fragt beim ersten Zugriff.
    NotDetermined,
    Denied,
}

pub fn microphone_permission() -> Permission {
    #[cfg(target_os = "macos")]
    return macos::microphone_permission();
    #[cfg(not(target_os = "macos"))]
    Permission::Granted
}

/// Fragt die Mikrofon-Freigabe an (Systemfenster). Blockiert, bis der
/// Nutzer geantwortet hat; gibt zurück, ob sie erteilt wurde.
pub fn request_microphone_permission() -> bool {
    #[cfg(target_os = "macos")]
    return macos::request_microphone_permission();
    #[cfg(not(target_os = "macos"))]
    true
}

pub fn camera_permission() -> Permission {
    #[cfg(target_os = "macos")]
    return macos::camera_permission();
    #[cfg(not(target_os = "macos"))]
    Permission::Granted
}

/// Fragt die Kamera-Freigabe an (Systemfenster), blockiert bis zur Antwort.
pub fn request_camera_permission() -> bool {
    #[cfg(target_os = "macos")]
    return macos::request_camera_permission();
    #[cfg(not(target_os = "macos"))]
    true
}

/// Öffnet eine Videospur zum Lesen, Bild für Bild als BGRA (Export).
pub fn open_video(path: &Path) -> Result<Box<dyn grabmark_video::FrameSource>> {
    #[cfg(target_os = "macos")]
    return macos::codec::open_video(path)
        .map(|r| Box::new(r) as Box<dyn grabmark_video::FrameSource>);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        Err(MediaError::NotImplemented)
    }
}

/// Eine Tonspur für den Export, Lautstärke 0 (stumm) bis 1 (wie aufgenommen).
#[derive(Debug, Clone, PartialEq)]
pub struct AudioInput {
    pub path: PathBuf,
    pub volume: f32,
}

/// Legt ein MP4 (H.264 bzw. HEVC nach `encoding`, Ton AAC) an, das die
/// zusammengesetzten Bilder aufnimmt. Die Tonspuren in `audio` werden gemischt. `kept` sind die
/// Stücke der Aufnahme, die bleiben (leer = alles); sie werden
/// hintereinandergehängt und auf `duration` gekürzt.
pub fn create_mp4(
    path: &Path,
    size: (u32, u32),
    fps: u32,
    encoding: &VideoEncoding,
    audio: &[AudioInput],
    kept: &[grabmark_core::TimeRange],
    duration: Duration,
) -> Result<Box<dyn grabmark_video::FrameSink>> {
    #[cfg(target_os = "macos")]
    return macos::codec::create_mp4(path, size, fps, encoding, audio, kept, duration)
        .map(|w| Box::new(w) as Box<dyn grabmark_video::FrameSink>);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (path, size, fps, encoding, audio, kept, duration);
        Err(MediaError::NotImplemented)
    }
}

/// Führt die Spuren zu einem MP4 (H.264 + AAC) zusammen, das überall
/// abspielbar ist. Die Tonspuren werden gemischt.
pub fn export_mp4(files: &RecordFiles, output: &Path) -> Result<()> {
    #[cfg(target_os = "macos")]
    return macos::export_mp4(files, output);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (files, output);
        Err(MediaError::NotImplemented)
    }
}

/// Trennt die Person vom Hintergrund der Webcam (Export mit Hintergrund
/// unscharf/entfernt). `None` = auf dieser Plattform noch nicht möglich.
pub fn segmenter() -> Option<Box<dyn grabmark_video::Segmenter>> {
    #[cfg(target_os = "macos")]
    return Some(Box::new(macos::vision::VisionSegmenter::new()));
    #[cfg(not(target_os = "macos"))]
    None
}

/// Ein Bild aus dem Video (für Vorschaubild und Galerie), `at` ab Beginn.
pub fn frame_at(video: &Path, at: Duration) -> Result<RgbaImage> {
    #[cfg(target_os = "macos")]
    return macos::frame_at(video, at);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (video, at);
        Err(MediaError::NotImplemented)
    }
}

/// Freier Platz auf dem Laufwerk von `path` in Bytes (`None` = unbekannt).
pub fn free_space(path: &Path) -> Option<u64> {
    #[cfg(target_os = "macos")]
    return macos::free_space(path);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        None
    }
}

/// Länge einer Spur. Funktioniert auch bei einer Datei, deren Aufnahme
/// abgebrochen ist (Fragmente alle 2 s) – dann bis zum letzten Fragment.
pub fn duration(path: &Path) -> Result<Duration> {
    #[cfg(target_os = "macos")]
    return macos::file_duration(path);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        Err(MediaError::NotImplemented)
    }
}

/// Videos mit H.264/HEVC brauchen gerade Kantenlängen (4:2:0).
pub fn even(v: u32) -> u32 {
    (v & !1).max(2)
}

/// Datenrate fürs Bildschirmvideo: Text bleibt scharf, ruhige Bildschirme
/// kosten trotzdem wenig (der Encoder spart bei unveränderten Bildern).
pub fn screen_bitrate(width: u32, height: u32, fps: u32) -> u32 {
    let bits = width as f64 * height as f64 * fps.max(1) as f64 * 0.08;
    bits.clamp(2_000_000.0, 40_000_000.0) as u32
}

/// Datenrate der Webcam (Kamerabild, kein Text).
pub fn camera_bitrate(width: u32, height: u32, fps: u32) -> u32 {
    let bits = width as f64 * height as f64 * fps.max(1) as f64 * 0.1;
    bits.clamp(2_000_000.0, 16_000_000.0) as u32
}

/// Datenrate des fertigen Videos: Standard ist 1,6× die Datenrate der
/// Aufnahme bei H.264; HEVC braucht rund 40 % weniger.
pub fn export_bitrate(width: u32, height: u32, fps: u32, encoding: &VideoEncoding) -> u32 {
    let codec = match encoding.codec {
        VideoCodec::H264 => 1.0,
        VideoCodec::Hevc => 0.6,
    };
    let quality = match encoding.quality {
        VideoQuality::Small => 0.5,
        VideoQuality::Standard => 1.0,
        VideoQuality::High => 2.0,
    };
    let base = (screen_bitrate(width, height, fps) as f64 * 1.6).min(60_000_000.0);
    (base * codec * quality).clamp(1_000_000.0, 100_000_000.0) as u32
}

/// Wandelt eine Lautstärke-Spitze in einen Balken mit `steps` Stufen
/// (logarithmisch, −50 dB … 0 dB).
pub fn level_steps(peak: f32, steps: u32) -> u32 {
    if peak <= 0.0 {
        return 0;
    }
    let db = 20.0 * peak.min(1.0).log10();
    let t = ((db + 50.0) / 50.0).clamp(0.0, 1.0);
    (t * steps as f32).round() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kanten_werden_gerade() {
        assert_eq!(even(1921), 1920);
        assert_eq!(even(1080), 1080);
        assert_eq!(even(1), 2);
    }

    #[test]
    fn datenrate_hat_grenzen() {
        assert_eq!(screen_bitrate(100, 100, 30), 2_000_000);
        assert_eq!(screen_bitrate(1920, 1080, 30), 4_976_640);
        assert_eq!(screen_bitrate(6016, 3384, 60), 40_000_000);
    }

    #[test]
    fn webcam_und_export_datenrate() {
        assert_eq!(camera_bitrate(1920, 1080, 30), 6_220_800);
        assert_eq!(camera_bitrate(640, 480, 30), 2_000_000);
        let standard = VideoEncoding::default();
        assert_eq!(export_bitrate(1920, 1080, 30, &standard), 7_962_624);
        let small_hevc = VideoEncoding {
            codec: VideoCodec::Hevc,
            quality: VideoQuality::Small,
            ..standard
        };
        assert_eq!(export_bitrate(1920, 1080, 30, &small_hevc), 2_388_787);
        let high = VideoEncoding {
            quality: VideoQuality::High,
            ..standard
        };
        assert_eq!(export_bitrate(1920, 1080, 30, &high), 15_925_248);
    }

    #[test]
    fn pegel_in_stufen() {
        assert_eq!(level_steps(0.0, 5), 0);
        assert_eq!(level_steps(1.0, 5), 5);
        // −20 dB = 0.1 → 60 % von 5
        assert_eq!(level_steps(0.1, 5), 3);
        assert_eq!(level_steps(0.001, 5), 0);
    }
}
