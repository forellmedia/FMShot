//! Bildschirmaufnahme pro Plattform.
//!
//! Alle Plattformen implementieren [`ScreenCapturer`]. Der Rest der App kennt
//! nur diesen Trait (ADR 0003, 0006).
//! - macOS: ScreenCaptureKit (`SCScreenshotManager`, ab macOS 14)
//! - Windows: DXGI Desktop Duplication / Windows Graphics Capture
//! - Linux: X11 bzw. xdg-desktop-portal + PipeWire unter Wayland

use fmshot_core::{Rect, RgbaImage};

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

/// Ein angeschlossener Monitor.
#[derive(Debug, Clone, PartialEq)]
pub struct Display {
    pub id: u32,
    pub name: String,
    /// Position und Grösse in logischen Punkten im globalen Koordinatensystem.
    pub bounds: Rect,
    /// Physische Pixel pro logischem Punkt (z. B. 2.0 bei Retina).
    pub scale_factor: f32,
    pub is_primary: bool,
}

/// Ein aufgenommenes Bild mit der Herkunft.
#[derive(Debug, Clone)]
pub struct Frame {
    pub display_id: u32,
    pub scale_factor: f32,
    pub image: RgbaImage,
}

#[derive(Debug, thiserror::Error)]
pub enum CaptureError {
    #[error(
        "Bildschirmaufnahme ist nicht erlaubt – Berechtigung in den Systemeinstellungen erteilen"
    )]
    PermissionDenied,
    #[error("Monitor {0} nicht gefunden")]
    UnknownDisplay(u32),
    #[error("Auf dieser Plattform noch nicht umgesetzt: {0}")]
    NotImplemented(&'static str),
    #[error("Aufnahme fehlgeschlagen: {0}")]
    Backend(String),
}

pub type Result<T> = std::result::Result<T, CaptureError>;

pub trait ScreenCapturer: Send + Sync {
    fn displays(&self) -> Result<Vec<Display>>;
    /// Nimmt einen ganzen Monitor in voller physischer Auflösung auf.
    fn capture_display(&self, display_id: u32) -> Result<Frame>;
    /// Nimmt alle Monitore auf einmal auf – Grundlage fürs eingefrorene Overlay.
    fn capture_all(&self) -> Result<Vec<Frame>> {
        self.displays()?
            .iter()
            .map(|d| self.capture_display(d.id))
            .collect()
    }
}

/// Die Implementierung für die aktuelle Plattform.
pub fn platform_capturer() -> Box<dyn ScreenCapturer> {
    #[cfg(target_os = "macos")]
    return Box::new(macos::MacCapturer::new());
    #[cfg(target_os = "windows")]
    return Box::new(windows::WindowsCapturer::new());
    #[cfg(target_os = "linux")]
    return Box::new(linux::LinuxCapturer::new());
}
