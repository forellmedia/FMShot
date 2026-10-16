//! Natives Auswahl-Overlay (ADR 0003): ein Fenster pro Monitor ohne Webview.
//! Zeigt das eingefrorene Bild, Abdunklung, Auswahlrahmen, Lupe und Pixelmasse.
//!
//! - macOS: `NSPanel` + Core-Animation-Ebenen (GPU, kein Kopieren pro Bild)
//! - Windows / Linux: folgen

use std::sync::Arc;

use fmshot_capture::Display;
use fmshot_core::RgbaImage;

#[cfg(target_os = "macos")]
mod macos;

/// Ein Monitor mit seinem eingefrorenen Bild.
#[derive(Debug, Clone)]
pub struct OverlayScreen {
    pub display: Display,
    pub image: Arc<RgbaImage>,
}

/// Gewählter Bereich in physischen Pixeln des Monitorbilds.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Selection {
    pub display_id: u32,
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayResult {
    Selected(Selection),
    Cancelled,
}

#[derive(Debug, thiserror::Error)]
pub enum OverlayError {
    #[error("Overlay muss im Haupt-Thread geöffnet werden")]
    NotMainThread,
    #[error("Keine Monitore zum Anzeigen")]
    NoScreens,
    #[error("Overlay ist auf dieser Plattform noch nicht umgesetzt")]
    NotImplemented,
}

pub type DoneCallback = Box<dyn FnOnce(OverlayResult)>;

/// Öffnet das Overlay auf allen Monitoren. `on_done` wird genau einmal im
/// Haupt-Thread aufgerufen, sobald ausgewählt oder abgebrochen wurde.
///
/// Muss im Haupt-Thread aufgerufen werden.
pub fn select_region(
    screens: Vec<OverlayScreen>,
    on_done: DoneCallback,
) -> Result<(), OverlayError> {
    if screens.is_empty() {
        return Err(OverlayError::NoScreens);
    }
    #[cfg(target_os = "macos")]
    return macos::select_region(screens, on_done);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = on_done;
        Err(OverlayError::NotImplemented)
    }
}

/// Rechteck in Punkten (Ursprung oben links) → Pixel des Monitorbilds.
/// Rundet nach aussen, damit nichts Sichtbares abgeschnitten wird, und
/// begrenzt auf das Bild.
pub fn points_to_pixels(
    rect: (f64, f64, f64, f64),
    points_size: (f64, f64),
    image_size: (u32, u32),
) -> (u32, u32, u32, u32) {
    let sx = image_size.0 as f64 / points_size.0.max(1.0);
    let sy = image_size.1 as f64 / points_size.1.max(1.0);
    let (x, y, w, h) = rect;
    let x0 = (x * sx).floor().clamp(0.0, image_size.0 as f64);
    let y0 = (y * sy).floor().clamp(0.0, image_size.1 as f64);
    let x1 = ((x + w) * sx).ceil().clamp(0.0, image_size.0 as f64);
    let y1 = ((y + h) * sy).ceil().clamp(0.0, image_size.1 as f64);
    (x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32)
}

/// Normalisiert ein Rechteck aus zwei Eckpunkten (Ziehen in jede Richtung).
pub fn rect_from_points(a: (f64, f64), b: (f64, f64)) -> (f64, f64, f64, f64) {
    let x = a.0.min(b.0);
    let y = a.1.min(b.1);
    (x, y, (a.0 - b.0).abs(), (a.1 - b.1).abs())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn retina_punkte_werden_doppelte_pixel() {
        let px = points_to_pixels((10.0, 20.0, 100.0, 50.0), (1512.0, 982.0), (3024, 1964));
        assert_eq!(px, (20, 40, 200, 100));
    }

    #[test]
    fn halbe_pixel_werden_nach_aussen_gerundet() {
        let px = points_to_pixels((0.4, 0.4, 1.0, 1.0), (100.0, 100.0), (100, 100));
        assert_eq!(px, (0, 0, 2, 2));
    }

    #[test]
    fn auswahl_wird_auf_bild_begrenzt() {
        let px = points_to_pixels((90.0, -5.0, 50.0, 50.0), (100.0, 100.0), (200, 200));
        assert_eq!(px, (180, 0, 20, 90));
    }

    #[test]
    fn ziehen_nach_links_oben() {
        assert_eq!(
            rect_from_points((50.0, 40.0), (10.0, 20.0)),
            (10.0, 20.0, 40.0, 20.0)
        );
    }
}
