//! Video und Ton (ADR 0011): Bildschirm als Video aufnehmen, Systemton und
//! Mikrofon, Kodieren und Zusammenführen zu einem MP4.
//!
//! Kodiert wird immer mit den Bordmitteln des Systems (Hardware), nie mit
//! FFmpeg (LGPL/GPL, Regel 5):
//! - macOS: ScreenCaptureKit-Stream → AVAssetWriter (HEVC/AAC); Mikrofon über
//!   AVFoundation, damit alle Spuren dieselbe Systemuhr haben
//! - Windows: Media Foundation – folgt
//! - Linux: PipeWire + openh264 – folgt
//!
//! Die Bilder des Bildschirms gehen ohne Kopie direkt zum Encoder. Darum
//! liegt die Video-Aufnahme hier und nicht in `fmshot-capture`, das nur
//! Einzelbilder liefert.

use std::path::{Path, PathBuf};
use std::time::Duration;

use fmshot_core::RgbaImage;

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
    /// Bildschirm als Video (MP4, HEVC).
    pub screen: PathBuf,
    /// Ton des Systems (M4A, AAC).
    pub system_audio: Option<PathBuf>,
    /// Mikrofon (M4A, AAC).
    pub microphone: Option<PathBuf>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecordOptions {
    pub target: RecordTarget,
    pub files: RecordFiles,
    /// Bilder pro Sekunde (höchstens; ein ruhiger Bildschirm liefert weniger).
    pub fps: u32,
    /// `None` = Standard-Mikrofon des Systems.
    pub microphone_id: Option<String>,
    /// Mauszeiger mit aufnehmen. Ab Etappe 5 zeichnet FMShot ihn selbst.
    pub show_cursor: bool,
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

/// Ein Mikrofon des Systems.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AudioDevice {
    pub id: String,
    pub name: String,
}

pub fn microphones() -> Vec<AudioDevice> {
    #[cfg(target_os = "macos")]
    return macos::microphones();
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
    fn pegel_in_stufen() {
        assert_eq!(level_steps(0.0, 5), 0);
        assert_eq!(level_steps(1.0, 5), 5);
        // −20 dB = 0.1 → 60 % von 5
        assert_eq!(level_steps(0.1, 5), 3);
        assert_eq!(level_steps(0.001, 5), 0);
    }
}
