//! Natives Auswahl-Overlay (ADR 0003): ein Fenster pro Monitor ohne Webview.
//! Zeigt Fadenkreuz, Abdunklung, Auswahlrahmen, Lupe und Pixelmasse.
//!
//! Das Overlay erscheint sofort über dem laufenden Bildschirm; das
//! eingefrorene Bild wird nachgereicht ([`OverlayHandle::set_frozen`]), sobald
//! die Aufnahme fertig ist. So hängt die gefühlte Geschwindigkeit nicht an der
//! Aufnahmezeit des Systems.
//!
//! - macOS: `NSPanel` + Core-Animation-Ebenen (GPU, kein Kopieren pro Bild)
//! - Windows / Linux: folgen

use std::sync::Arc;

use fmshot_core::RgbaImage;

#[cfg(target_os = "macos")]
mod macos;

/// Was im Overlay ausgewählt wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum OverlayMode {
    /// Bereich aufziehen.
    Region,
    /// Fenster unter der Maus hervorheben, Klick wählt es.
    Window,
}

/// Ein Fenster, das im Fenster-Modus gewählt werden kann.
#[derive(Debug, Clone, PartialEq)]
pub struct WindowTarget {
    pub id: u32,
    /// Globale Koordinaten in Punkten, Ursprung oben links am Hauptmonitor.
    pub bounds: (f64, f64, f64, f64),
}

/// Gewählter Bereich in physischen Pixeln des Monitors.
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
    /// Fenster-Modus: dieses Fenster wurde angeklickt.
    Window(u32),
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

/// Griff auf ein offenes Overlay. Nur im Haupt-Thread benutzbar.
pub struct OverlayHandle {
    #[cfg(target_os = "macos")]
    inner: macos::Handle,
}

impl OverlayHandle {
    /// Reicht das eingefrorene Bild eines Monitors nach (physische Pixel).
    pub fn set_frozen(&self, display_id: u32, image: Arc<RgbaImage>) {
        #[cfg(target_os = "macos")]
        self.inner.set_frozen(display_id, image);
        #[cfg(not(target_os = "macos"))]
        let _ = (display_id, image);
    }

    /// Schliesst das Overlay ohne Ergebnis (z. B. wenn die Aufnahme scheitert).
    pub fn cancel(&self) {
        #[cfg(target_os = "macos")]
        self.inner.cancel();
    }

    pub fn is_open(&self) -> bool {
        #[cfg(target_os = "macos")]
        return self.inner.is_open();
        #[cfg(not(target_os = "macos"))]
        false
    }
}

/// Öffnet das Overlay auf allen Monitoren. `on_done` wird genau einmal im
/// Haupt-Thread aufgerufen, sobald ausgewählt oder abgebrochen wurde.
///
/// `windows` wird nur im Fenster-Modus gebraucht (von vorne nach hinten).
pub fn open(
    mode: OverlayMode,
    windows: Vec<WindowTarget>,
    on_done: DoneCallback,
) -> Result<OverlayHandle, OverlayError> {
    #[cfg(target_os = "macos")]
    return macos::open(mode, windows, on_done).map(|inner| OverlayHandle { inner });
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (mode, windows, on_done);
        Err(OverlayError::NotImplemented)
    }
}

/// Rechteck in Punkten (Ursprung oben links) → physische Pixel.
/// Rundet nach aussen, damit nichts Sichtbares abgeschnitten wird, und
/// begrenzt auf den Monitor.
pub fn points_to_pixels(
    rect: (f64, f64, f64, f64),
    points_size: (f64, f64),
    pixel_size: (u32, u32),
) -> (u32, u32, u32, u32) {
    let sx = pixel_size.0 as f64 / points_size.0.max(1.0);
    let sy = pixel_size.1 as f64 / points_size.1.max(1.0);
    let (x, y, w, h) = rect;
    let x0 = (x * sx).floor().clamp(0.0, pixel_size.0 as f64);
    let y0 = (y * sy).floor().clamp(0.0, pixel_size.1 as f64);
    let x1 = ((x + w) * sx).ceil().clamp(0.0, pixel_size.0 as f64);
    let y1 = ((y + h) * sy).ceil().clamp(0.0, pixel_size.1 as f64);
    (x0 as u32, y0 as u32, (x1 - x0) as u32, (y1 - y0) as u32)
}

/// Normalisiert ein Rechteck aus zwei Eckpunkten (Ziehen in jede Richtung).
pub fn rect_from_points(a: (f64, f64), b: (f64, f64)) -> (f64, f64, f64, f64) {
    let x = a.0.min(b.0);
    let y = a.1.min(b.1);
    (x, y, (a.0 - b.0).abs(), (a.1 - b.1).abs())
}

/// Oberstes Fenster unter dem Punkt (globale Punkte).
pub fn window_at(windows: &[WindowTarget], p: (f64, f64)) -> Option<&WindowTarget> {
    windows.iter().find(|w| {
        let (x, y, ww, wh) = w.bounds;
        p.0 >= x && p.1 >= y && p.0 < x + ww && p.1 < y + wh
    })
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

    #[test]
    fn oberstes_fenster_gewinnt() {
        let windows = vec![
            WindowTarget {
                id: 1,
                bounds: (10.0, 10.0, 50.0, 50.0),
            },
            WindowTarget {
                id: 2,
                bounds: (0.0, 0.0, 200.0, 200.0),
            },
        ];
        assert_eq!(window_at(&windows, (20.0, 20.0)).map(|w| w.id), Some(1));
        assert_eq!(window_at(&windows, (100.0, 100.0)).map(|w| w.id), Some(2));
        assert_eq!(window_at(&windows, (300.0, 0.0)), None);
    }
}
