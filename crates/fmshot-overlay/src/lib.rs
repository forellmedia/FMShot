//! Natives Auswahl-Overlay (ADR 0003): ein Fenster pro Monitor ohne Webview.
//! Zeigt Fadenkreuz, Abdunklung, Auswahlrahmen, Lupe und Pixelmasse.
//!
//! Das Overlay erscheint sofort über dem laufenden Bildschirm; das
//! eingefrorene Bild wird nachgereicht ([`OverlayHandle::set_frozen`]), sobald
//! die Aufnahme fertig ist. So hängt die gefühlte Geschwindigkeit nicht an der
//! Aufnahmezeit des Systems.
//!
//! - macOS: `NSPanel` + Core-Animation-Ebenen (GPU, kein Kopieren pro Bild)
//! - Windows: Popup-Fenster mit GDI, neu gezeichnet werden nur geänderte
//!   Rechtecke (ADR 0003, Nachtrag Windows)
//! - Linux: folgt

use std::sync::Arc;

use fmshot_core::RgbaImage;

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod windows;

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
    #[error("Overlay: {0}")]
    Backend(String),
}

pub type DoneCallback = Box<dyn FnOnce(OverlayResult)>;

/// Griff auf ein offenes Overlay. Nur im Haupt-Thread benutzbar.
pub struct OverlayHandle {
    #[cfg(target_os = "macos")]
    inner: macos::Handle,
    #[cfg(target_os = "windows")]
    inner: windows::Handle,
}

impl OverlayHandle {
    /// Reicht das eingefrorene Bild eines Monitors nach (physische Pixel).
    pub fn set_frozen(&self, display_id: u32, image: Arc<RgbaImage>) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        self.inner.set_frozen(display_id, image);
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let _ = (display_id, image);
    }

    /// Schliesst das Overlay ohne Ergebnis (z. B. wenn die Aufnahme scheitert).
    pub fn cancel(&self) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        self.inner.cancel();
    }

    pub fn is_open(&self) -> bool {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        return self.inner.is_open();
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
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
    #[cfg(target_os = "windows")]
    return windows::open(mode, windows, on_done).map(|inner| OverlayHandle { inner });
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (mode, windows, on_done);
        Err(OverlayError::NotImplemented)
    }
}

/// Knopf in der Scroll-Leiste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ScrollAction {
    Finish,
    Cancel,
}

/// Rahmen und Bedienleiste während einer Scroll-Aufnahme.
pub struct ScrollUi {
    #[cfg(target_os = "macos")]
    inner: macos::ScrollUi,
    #[cfg(target_os = "windows")]
    inner: windows::ScrollUi,
}

impl ScrollUi {
    /// Statuszeile, z. B. „Scrollen … 2400 px“.
    pub fn set_status(&self, text: &str) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        self.inner.set_status(text);
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let _ = text;
    }

    pub fn close(&self) {
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        self.inner.close();
    }
}

/// Zeigt Rahmen und Leiste um `rect` (physische Pixel des Monitors).
/// Mausklicks und Scrollen gehen weiterhin an die App darunter.
pub fn open_scroll_ui(
    display_id: u32,
    rect: (u32, u32, u32, u32),
    labels: (&str, &str, &str),
    on_action: Box<dyn Fn(ScrollAction)>,
) -> Result<ScrollUi, OverlayError> {
    #[cfg(target_os = "macos")]
    return macos::open_scroll_ui(display_id, rect, labels, on_action)
        .map(|inner| ScrollUi { inner });
    #[cfg(target_os = "windows")]
    return windows::open_scroll_ui(display_id, rect, labels, on_action)
        .map(|inner| ScrollUi { inner });
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (display_id, rect, labels, on_action);
        Err(OverlayError::NotImplemented)
    }
}

/// Knopf in der Leiste der Video-Aufnahme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordAction {
    /// Pause bzw. Weiter.
    Pause,
    Stop,
    Cancel,
}

/// Texte der Video-Leiste (übersetzt).
#[derive(Debug, Clone, Copy)]
pub struct RecordLabels<'a> {
    pub status: &'a str,
    pub pause: &'a str,
    pub stop: &'a str,
    pub cancel: &'a str,
}

/// Rahmen und Leiste während einer Video-Aufnahme.
pub struct RecordUi {
    #[cfg(target_os = "macos")]
    inner: macos::RecordUi,
}

impl RecordUi {
    /// Statuszeile, z. B. „● 0:12  Mikro ▮▮▮▯▯“.
    pub fn set_status(&self, text: &str) {
        #[cfg(target_os = "macos")]
        self.inner.set_status(text);
        #[cfg(not(target_os = "macos"))]
        let _ = text;
    }

    /// Text des Pause-Knopfs („Pause“ / „Weiter“).
    pub fn set_pause_label(&self, text: &str) {
        #[cfg(target_os = "macos")]
        self.inner.set_pause_label(text);
        #[cfg(not(target_os = "macos"))]
        let _ = text;
    }

    pub fn close(&self) {
        #[cfg(target_os = "macos")]
        self.inner.close();
    }
}

/// Zeigt die Leiste der Video-Aufnahme. Mit `rect` (physische Pixel des
/// Monitors) zusätzlich einen roten Rahmen um den Bereich. FMShot selbst ist
/// im Video nie zu sehen.
pub fn open_record_ui(
    display_id: u32,
    rect: Option<(u32, u32, u32, u32)>,
    labels: &RecordLabels<'_>,
    on_action: Box<dyn Fn(RecordAction)>,
) -> Result<RecordUi, OverlayError> {
    #[cfg(target_os = "macos")]
    return macos::open_record_ui(display_id, rect, labels, on_action)
        .map(|inner| RecordUi { inner });
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (display_id, rect, labels, on_action);
        Err(OverlayError::NotImplemented)
    }
}

/// Knöpfe der Schnellzugriff-Vorschau.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PreviewAction {
    Copy,
    Save,
    Edit,
    Text,
    Close,
    /// Bild wurde in eine andere App gezogen.
    Dragged,
}

/// Wo die Vorschau erscheint.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum PreviewPlacement {
    /// Neben diesem Punkt (globale Punkte, Ursprung oben links).
    At(f64, f64),
    /// Unten rechts auf dem Hauptmonitor.
    Corner,
}

/// Zeigt die Schnellzugriff-Vorschau. `image` ist ein Vorschaubild,
/// `file` die Datei, die beim Ziehen übergeben wird. Schliesst sich nach
/// einigen Sekunden von selbst, solange die Maus nicht darüber ist.
/// Muss im Haupt-Thread laufen.
pub fn show_preview(
    image: Arc<RgbaImage>,
    file: &str,
    placement: PreviewPlacement,
    labels: [&str; 4],
    on_action: Box<dyn Fn(PreviewAction)>,
) -> Result<(), OverlayError> {
    #[cfg(target_os = "macos")]
    return macos::show_preview(image, file, placement, labels, on_action);
    #[cfg(target_os = "windows")]
    return windows::show_preview(image, file, placement, labels, on_action);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = (image, file, placement, labels, on_action);
        Err(OverlayError::NotImplemented)
    }
}

/// Kurzer Hinweis am unteren Bildschirmrand (z. B. „Text kopiert“), der von
/// selbst verschwindet. Stiehlt keinen Fokus. Muss im Haupt-Thread laufen.
pub fn show_toast(text: &str) {
    #[cfg(target_os = "macos")]
    macos::show_toast(text);
    #[cfg(target_os = "windows")]
    windows::show_toast(text);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let _ = text;
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
