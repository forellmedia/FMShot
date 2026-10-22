//! Bildschirmaufnahme pro Plattform.
//!
//! Alle Plattformen implementieren [`ScreenCapturer`]. Der Rest der App kennt
//! nur diesen Trait (ADR 0003, 0006).
//! - macOS: ScreenCaptureKit (`SCScreenshotManager`, ab macOS 14)
//! - Windows: GDI (`BitBlt`, `PrintWindow`), siehe ADR 0003 (Nachtrag Windows)
//! - Linux: X11 bzw. xdg-desktop-portal + PipeWire unter Wayland
//!
//! **Globale Koordinaten** (Ursprung immer oben links am Hauptmonitor):
//! - macOS: logische Punkte.
//! - Windows: physische Pixel des virtuellen Desktops. Mit gemischten
//!   Skalierungen gibt es dort kein durchgehendes logisches System;
//!   [`Display::scale_factor`] ist deshalb 1.0, die Skalierung des Monitors
//!   steht in [`Frame::scale_factor`].

use nabshot_core::{Rect, RgbaImage};

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
    /// Position und Grösse in globalen Koordinaten (siehe oben).
    pub bounds: Rect,
    /// Physische Pixel pro globaler Einheit (z. B. 2.0 bei Retina, unter
    /// Windows immer 1.0).
    pub scale_factor: f32,
    pub is_primary: bool,
}

/// Rechteck in physischen Pixeln relativ zu einem Monitor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PixelRect {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

/// Ein sichtbares Fenster (für die Fenster-Aufnahme).
#[derive(Debug, Clone, PartialEq)]
pub struct WindowInfo {
    pub id: u32,
    /// Globale Koordinaten (siehe oben).
    pub bounds: Rect,
    pub app_name: String,
    pub title: String,
}

/// Sind die Fenster von Nabshot selbst im Bild?
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OwnWindows {
    /// Nein – etwa wenn die Aufnahme aus Nabshot heraus startet.
    Hidden,
    /// Normale Fenster (Editor, Galerie, Einstellungen) ja, damit sich
    /// Nabshot selbst fotografieren lässt. Overlay und schwebende Fenster
    /// (Tray-Fenster, Hinweise) bleiben trotzdem draussen.
    Shown,
}

/// Ein aufgenommenes Bild mit der Herkunft.
#[derive(Debug, Clone)]
pub struct Frame {
    pub display_id: u32,
    /// Physische Pixel pro logischem Punkt des Monitors (Retina 2.0,
    /// Windows mit 150 % 1.5) – bestimmt die Auflösung in PNG und
    /// Zwischenablage.
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
    /// Darf die App den Bildschirm aufnehmen? (macOS: Datenschutz-Freigabe)
    fn has_permission(&self) -> bool {
        true
    }
    /// Fragt die Freigabe beim System an. Gibt zurück, ob sie jetzt besteht.
    fn request_permission(&self) -> bool {
        true
    }
    fn displays(&self) -> Result<Vec<Display>>;
    /// Mauszeiger in globalen Punkten (Ursprung oben links am Hauptmonitor).
    fn cursor_position(&self) -> Option<(f32, f32)> {
        None
    }

    /// Ist die linke Maustaste gerade gedrückt? (Mausspur fürs Video, ADR 0011)
    fn mouse_button_down(&self) -> bool {
        false
    }

    /// Bereitet die Aufnahme vor (Caches füllen), damit die erste schnell ist.
    fn warm_up(&self) {}

    /// Sichtbare normale Fenster, von vorne nach hinten. Die eigenen Fenster
    /// der App sind nur mit [`OwnWindows::Shown`] dabei.
    fn windows(&self, _own: OwnWindows) -> Result<Vec<WindowInfo>> {
        Err(CaptureError::NotImplemented("Fenster auflisten"))
    }

    /// Lage eines Fensters jetzt (globale Punkte) – z. B. damit der Rahmen
    /// einer Video-Aufnahme dem Fenster folgt. Günstig genug für mehrmals
    /// pro Sekunde. `None` = unbekannt oder geschlossen.
    fn window_bounds(&self, _window_id: u32) -> Option<Rect> {
        None
    }

    /// Nimmt ein einzelnes Fenster auf (mit durchsichtigen Ecken).
    fn capture_window(&self, _window_id: u32) -> Result<Frame> {
        Err(CaptureError::NotImplemented("Fenster aufnehmen"))
    }

    /// Nimmt einen Ausschnitt eines Monitors auf (physische Pixel relativ zum
    /// Monitor) – für die Scroll-Aufnahme, die das viele Male pro Sekunde tut.
    fn capture_region(&self, _display_id: u32, _rect: PixelRect) -> Result<Frame> {
        Err(CaptureError::NotImplemented("Ausschnitt aufnehmen"))
    }

    /// Darf Nabshot Eingaben simulieren (automatisches Scrollen)?
    fn has_input_permission(&self) -> bool {
        false
    }

    /// Fragt die Berechtigung für simulierte Eingaben an.
    fn request_input_permission(&self) -> bool {
        false
    }

    /// Scrollt an `at` (globale Punkte) um `delta` Punkte nach unten.
    fn scroll_at(&self, _at: (f32, f32), _delta: i32) -> Result<()> {
        Err(CaptureError::NotImplemented("automatisch scrollen"))
    }

    /// Nimmt einen ganzen Monitor in voller physischer Auflösung auf.
    /// Fenster von Nabshot selbst (z. B. das Overlay) sind nie zu sehen.
    fn capture_display(&self, display_id: u32) -> Result<Frame>;
    /// Nimmt alle Monitore auf einmal auf – Grundlage fürs eingefrorene Overlay.
    /// Das Overlay ist nie zu sehen, die übrigen Fenster von Nabshot je nach `own`.
    fn capture_all(&self, _own: OwnWindows) -> Result<Vec<Frame>> {
        self.displays()?
            .iter()
            .map(|d| self.capture_display(d.id))
            .collect()
    }
}

/// Monitor, auf dem der Punkt liegt (globale Punkte).
pub fn display_at(displays: &[Display], p: (f32, f32)) -> Option<&Display> {
    displays.iter().find(|d| {
        let b = d.bounds;
        p.0 >= b.x && p.1 >= b.y && p.0 < b.right() && p.1 < b.bottom()
    })
}

/// Freigaben, die das System an einer eigenen Stelle der Einstellungen
/// verwaltet.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PrivacyPane {
    /// Bildschirmaufnahme (macOS: „Bildschirm- und Systemaudioaufnahme“)
    ScreenCapture,
    /// Simulierte Eingaben, z. B. automatisches Scrollen (macOS: „Bedienungshilfen“)
    Accessibility,
    Microphone,
    Camera,
}

/// Adresse, die direkt die passende Stelle der Systemeinstellungen öffnet.
/// `None`, wenn das System dafür keine Freigabe kennt (Linux: Portal fragt selbst).
pub fn privacy_settings_url(pane: PrivacyPane) -> Option<&'static str> {
    if cfg!(target_os = "macos") {
        Some(match pane {
            PrivacyPane::ScreenCapture => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture"
            }
            PrivacyPane::Accessibility => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Accessibility"
            }
            PrivacyPane::Microphone => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Microphone"
            }
            PrivacyPane::Camera => {
                "x-apple.systempreferences:com.apple.preference.security?Privacy_Camera"
            }
        })
    } else if cfg!(target_os = "windows") {
        match pane {
            PrivacyPane::Microphone => Some("ms-settings:privacy-microphone"),
            PrivacyPane::Camera => Some("ms-settings:privacy-webcam"),
            // Bildschirm und Eingaben brauchen unter Windows keine Freigabe.
            PrivacyPane::ScreenCapture | PrivacyPane::Accessibility => None,
        }
    } else {
        None
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

#[cfg(test)]
mod tests {
    use super::*;

    fn display(id: u32, x: f32) -> Display {
        Display {
            id,
            name: String::new(),
            bounds: Rect::new(x, 0.0, 100.0, 100.0),
            scale_factor: 1.0,
            is_primary: id == 1,
        }
    }

    #[test]
    fn monitor_unter_dem_punkt() {
        let d = [display(1, 0.0), display(2, 100.0)];
        assert_eq!(display_at(&d, (150.0, 50.0)).map(|d| d.id), Some(2));
        assert_eq!(display_at(&d, (250.0, 50.0)), None);
    }
}
