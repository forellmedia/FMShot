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
use std::time::Instant;

use nabshot_core::RgbaImage;

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
        log::debug!(
            "Overlay: eingefrorenes Bild für Monitor {display_id}, {}×{}",
            image.width(),
            image.height()
        );
        #[cfg(any(target_os = "macos", target_os = "windows"))]
        self.inner.set_frozen(display_id, image);
        #[cfg(not(any(target_os = "macos", target_os = "windows")))]
        let _ = (display_id, image);
    }

    /// Schliesst das Overlay ohne Ergebnis (z. B. wenn die Aufnahme scheitert).
    pub fn cancel(&self) {
        log::debug!("Overlay von aussen abgebrochen");
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
    let started = Instant::now();
    log::info!("Overlay öffnen: {mode:?}, {} Fenster", windows.len());
    let on_done: DoneCallback = Box::new(move |result| {
        let ms = started.elapsed().as_millis();
        match &result {
            OverlayResult::Selected(s) => log::info!(
                "Overlay: Bereich {}×{} px bei ({}, {}) auf Monitor {} gewählt nach {ms} ms",
                s.width,
                s.height,
                s.x,
                s.y,
                s.display_id
            ),
            OverlayResult::Window(id) => {
                log::info!("Overlay: Fenster {id} gewählt nach {ms} ms")
            }
            OverlayResult::Cancelled => log::info!("Overlay abgebrochen nach {ms} ms"),
        }
        on_done(result)
    });
    #[cfg(target_os = "macos")]
    let result = macos::open(mode, windows, on_done).map(|inner| OverlayHandle { inner });
    #[cfg(target_os = "windows")]
    let result = windows::open(mode, windows, on_done).map(|inner| OverlayHandle { inner });
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let result = {
        let _ = (mode, windows, on_done);
        Err(OverlayError::NotImplemented)
    };
    logged("Overlay", started, result)
}

/// Protokolliert, ob ein Fenster aufging und wie lange das dauerte.
fn logged<T>(
    what: &str,
    started: Instant,
    result: Result<T, OverlayError>,
) -> Result<T, OverlayError> {
    match &result {
        Ok(_) => log::debug!("{what} offen nach {} ms", started.elapsed().as_millis()),
        // Auf dieser Plattform fehlt es eben – kein Fehler.
        Err(OverlayError::NotImplemented) => {
            log::debug!("{what}: auf dieser Plattform nicht umgesetzt")
        }
        Err(e) => log::error!("{what} öffnen fehlgeschlagen: {e}"),
    }
    result
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
        log::debug!("Scroll-Leiste schliessen");
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
    let started = Instant::now();
    log::debug!("Scroll-Leiste öffnen: Monitor {display_id}, Bereich {rect:?}");
    let on_action: Box<dyn Fn(ScrollAction)> = Box::new(move |action| {
        log::info!("Scroll-Leiste: {action:?}");
        on_action(action)
    });
    #[cfg(target_os = "macos")]
    let result =
        macos::open_scroll_ui(display_id, rect, labels, on_action).map(|inner| ScrollUi { inner });
    #[cfg(target_os = "windows")]
    let result = windows::open_scroll_ui(display_id, rect, labels, on_action)
        .map(|inner| ScrollUi { inner });
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let result = {
        let _ = (display_id, rect, labels, on_action);
        Err(OverlayError::NotImplemented)
    };
    logged("Scroll-Leiste", started, result)
}

/// Knopf in der Leiste der Video-Aufnahme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RecordAction {
    /// Pause bzw. Weiter.
    Pause,
    /// Live zeichnen ein/aus.
    Draw,
    /// Webcam ein/aus.
    Webcam,
    /// Webcam gross / Bildschirm klein und zurück.
    WebcamLarge,
    Stop,
    Cancel,
}

/// Texte der Video-Leiste (übersetzt).
#[derive(Debug, Clone, Copy)]
pub struct RecordLabels<'a> {
    pub status: &'a str,
    pub pause: &'a str,
    /// Knopf „Zeichnen“; `None` = kein Knopf (nur Webcam).
    pub draw: Option<&'a str>,
    /// Knopf „Webcam aus/an“; `None` = kein Knopf (keine Kamera).
    pub webcam: Option<&'a str>,
    /// Knopf „Gross/Klein“; nur mit Webcam-Knopf.
    pub webcam_large: Option<&'a str>,
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

    /// Grosse Zeit der laufenden Aufnahme, daneben klein `detail` (Pegel,
    /// Warnung). `live` = roter Punkt (aus = Pause).
    pub fn set_time(&self, time: &str, detail: &str, live: bool) {
        #[cfg(target_os = "macos")]
        self.inner.set_time(time, detail, live);
        #[cfg(not(target_os = "macos"))]
        let _ = (time, detail, live);
    }

    /// Aufnahme unterbrochen: Rahmen und Text werden orange.
    pub fn set_failed(&self, text: &str) {
        #[cfg(target_os = "macos")]
        self.inner.set_failed(text);
        #[cfg(not(target_os = "macos"))]
        let _ = text;
    }

    /// Rahmen an einen neuen Bereich legen (physische Pixel des Monitors),
    /// z. B. wenn das aufgenommene Fenster verschoben wurde.
    pub fn set_rect(&self, rect: (u32, u32, u32, u32)) {
        #[cfg(target_os = "macos")]
        self.inner.set_rect(rect);
        #[cfg(not(target_os = "macos"))]
        let _ = rect;
    }

    /// Text eines Knopfs, z. B. Pause → „Weiter“, Webcam → „Webcam an“.
    pub fn set_label(&self, action: RecordAction, text: &str) {
        #[cfg(target_os = "macos")]
        self.inner.set_label(action, text);
        #[cfg(not(target_os = "macos"))]
        let _ = (action, text);
    }

    pub fn close(&self) {
        log::debug!("Aufnahmeleiste schliessen");
        #[cfg(target_os = "macos")]
        self.inner.close();
    }
}

/// Zeigt die Leiste der Video-Aufnahme. Mit `rect` (physische Pixel des
/// Monitors) zusätzlich einen roten Rahmen um den Bereich – füllt er den
/// Monitor, liegt der Rahmen am Rand nach innen. Nabshot selbst ist im Video
/// nie zu sehen.
pub fn open_record_ui(
    display_id: u32,
    rect: Option<(u32, u32, u32, u32)>,
    labels: &RecordLabels<'_>,
    on_action: Box<dyn Fn(RecordAction)>,
) -> Result<RecordUi, OverlayError> {
    let started = Instant::now();
    log::debug!("Aufnahmeleiste öffnen: Monitor {display_id}, Rahmen {rect:?}");
    let on_action: Box<dyn Fn(RecordAction)> = Box::new(move |action| {
        log::info!("Aufnahmeleiste: {action:?}");
        on_action(action)
    });
    #[cfg(target_os = "macos")]
    let result =
        macos::open_record_ui(display_id, rect, labels, on_action).map(|inner| RecordUi { inner });
    #[cfg(not(target_os = "macos"))]
    let result = {
        let _ = (display_id, rect, labels, on_action);
        Err(OverlayError::NotImplemented)
    };
    logged("Aufnahmeleiste", started, result)
}

/// Aussehen der Webcam-Blase.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct BubbleStyle {
    /// Eckenradius als Anteil der kürzeren Seite (0.5 = Kreis bzw. Pille).
    pub corner_radius: f64,
    /// Rahmenbreite als Anteil der kürzeren Seite und Farbe (RGBA, 0 bis 1).
    pub border: Option<(f64, [f64; 4])>,
    pub shadow: bool,
    /// Ohne Fläche hinter dem Bild (Hintergrund entfernt: Nur die Person
    /// schwebt über dem Bildschirm).
    pub transparent: bool,
}

/// Live-Bild der Webcam während der Aufnahme (ADR 0011): schwebt über
/// allem, lässt sich ziehen, das Scrollrad ändert die Grösse. Nabshot selbst
/// ist nie im Bildschirmvideo – die Blase also auch nicht.
pub struct WebcamBubble {
    #[cfg(target_os = "macos")]
    inner: macos::Bubble,
}

impl WebcamBubble {
    /// Lage und Grösse in physischen Pixeln relativ zum Monitor, auf dem
    /// die Blase geöffnet wurde (darf ausserhalb liegen).
    pub fn frame(&self) -> (f64, f64, f64, f64) {
        #[cfg(target_os = "macos")]
        return self.inner.frame();
        #[cfg(not(target_os = "macos"))]
        (0.0, 0.0, 0.0, 0.0)
    }

    pub fn set_visible(&self, visible: bool) {
        log::debug!("Webcam-Blase sichtbar={visible}");
        #[cfg(target_os = "macos")]
        self.inner.set_visible(visible);
        #[cfg(not(target_os = "macos"))]
        let _ = visible;
    }

    pub fn close(&self) {
        log::debug!("Webcam-Blase schliessen");
        #[cfg(target_os = "macos")]
        self.inner.close();
    }
}

/// Öffnet die Webcam-Blase auf `display_id` an `rect` (physische Pixel
/// relativ zum Monitor). Muss im Haupt-Thread laufen.
///
/// # Safety
/// `layer` muss auf eine gültige Ebene des Systems zeigen (macOS: `CALayer`,
/// z. B. aus `nabshot_media::PreviewLayer`). Die Blase hält sie selbst fest.
pub unsafe fn open_webcam_bubble(
    display_id: u32,
    rect: (f64, f64, f64, f64),
    style: BubbleStyle,
    layer: *mut std::ffi::c_void,
) -> Result<WebcamBubble, OverlayError> {
    let started = Instant::now();
    log::debug!("Webcam-Blase öffnen: Monitor {display_id}, {rect:?}, {style:?}");
    #[cfg(target_os = "macos")]
    // SAFETY: vom Aufrufer zugesichert.
    let result = unsafe { macos::open_bubble(display_id, rect, style, layer) }
        .map(|inner| WebcamBubble { inner });
    #[cfg(not(target_os = "macos"))]
    let result = {
        let _ = (display_id, rect, style, layer);
        Err(OverlayError::NotImplemented)
    };
    logged("Webcam-Blase", started, result)
}

/// Neue Grösse der Blase nach einem Schritt am Scrollrad (Punkte): Höhe
/// folgt dem Seitenverhältnis, die Breite bleibt in `min`…`max`.
pub fn bubble_resize(size: (f64, f64), delta: f64, min: f64, max: f64) -> (f64, f64) {
    let aspect = size.0 / size.1.max(1.0);
    let factor = (1.0 + delta * 0.02).clamp(0.8, 1.25);
    let width = (size.0 * factor).clamp(min, max);
    (width, width / aspect)
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
    let started = Instant::now();
    log::debug!(
        "Vorschau zeigen: {}×{}, {placement:?}",
        image.width(),
        image.height()
    );
    let on_action: Box<dyn Fn(PreviewAction)> = Box::new(move |action| {
        log::debug!("Vorschau: {action:?}");
        on_action(action)
    });
    #[cfg(target_os = "macos")]
    let result = macos::show_preview(image, file, placement, labels, on_action);
    #[cfg(target_os = "windows")]
    let result = windows::show_preview(image, file, placement, labels, on_action);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    let result = {
        let _ = (image, file, placement, labels, on_action);
        Err(OverlayError::NotImplemented)
    };
    logged("Vorschau", started, result)
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

/// Meldet Mausklicks in Fenstern von Nabshot selbst (Aufnahmeleiste,
/// Zeichenfläche, Webcam-Blase, Tray …), solange der Wächter lebt. So
/// erscheinen sie in der Mausspur des Videos nicht als Klick.
pub struct OwnClickWatch {
    #[cfg(target_os = "macos")]
    _inner: Option<macos::ClickWatch>,
}

/// Startet den Wächter; `on_press` läuft im Haupt-Thread. Erstellen und
/// freigeben im Haupt-Thread. Windows/Linux: meldet vorerst nichts.
pub fn watch_own_clicks(on_press: Box<dyn Fn()>) -> OwnClickWatch {
    #[cfg(target_os = "macos")]
    return OwnClickWatch {
        _inner: macos::watch_own_clicks(on_press),
    };
    #[cfg(not(target_os = "macos"))]
    {
        let _ = on_press;
        OwnClickWatch {}
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
    fn blase_waechst_mit_scrollrad_und_bleibt_in_grenzen() {
        let (w, h) = bubble_resize((200.0, 100.0), 5.0, 80.0, 600.0);
        assert!((w - 220.0).abs() < 1e-9 && (h - 110.0).abs() < 1e-9);
        assert_eq!(
            bubble_resize((590.0, 590.0), 20.0, 80.0, 600.0),
            (600.0, 600.0)
        );
        assert_eq!(
            bubble_resize((82.0, 82.0), -20.0, 80.0, 600.0),
            (80.0, 80.0)
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

// --- Live zeichnen während der Video-Aufnahme --------------------------------

/// Werkzeug der Zeichenleiste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DrawTool<'a> {
    /// Name eines SF Symbols (macOS), z. B. `arrow.up.right`.
    pub symbol: &'a str,
    /// Taste, die das Werkzeug wählt (klein geschrieben).
    pub key: char,
}

/// Was auf der Zeichenfläche passiert. Lagen in Pixeln relativ zum
/// aufgenommenen Bereich.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum DrawEvent {
    Tool(usize),
    Color(usize),
    Press {
        x: f32,
        y: f32,
    },
    Drag {
        x: f32,
        y: f32,
        shift: bool,
    },
    Release {
        x: f32,
        y: f32,
        shift: bool,
    },
    /// Esc gedrückt.
    Escape,
    /// Knopf „Fertig“ in der Leiste oder Rechtsklick.
    Exit,
}

/// Durchsichtige Fläche über dem Monitor, auf der während der Aufnahme
/// gezeichnet wird; links eine Werkzeugleiste. Sie ist (wie alles von
/// Nabshot) nicht im Bildschirmvideo. Inaktiv gehen Mausklicks an die Apps
/// darunter, und nur die Zeichnung ist zu sehen.
pub struct DrawSurface {
    #[cfg(target_os = "macos")]
    inner: macos::draw::Surface,
}

impl DrawSurface {
    /// Zeichnen ein/aus: Werkzeugleiste, Fadenkreuz, Tastatur.
    pub fn set_active(&self, active: bool) {
        log::debug!("Zeichenfläche aktiv={active}");
        #[cfg(target_os = "macos")]
        self.inner.set_active(active);
        #[cfg(not(target_os = "macos"))]
        let _ = active;
    }

    pub fn is_active(&self) -> bool {
        #[cfg(target_os = "macos")]
        return self.inner.is_active();
        #[cfg(not(target_os = "macos"))]
        false
    }

    /// Bild der Zeichnungen (RGBA, gerade Deckkraft) über dem ganzen
    /// Bereich, beliebig skaliert; `None` = nichts zu sehen.
    pub fn set_image(&self, image: Option<Arc<RgbaImage>>) {
        #[cfg(target_os = "macos")]
        self.inner.set_image(image);
        #[cfg(not(target_os = "macos"))]
        let _ = image;
    }

    /// Hervorgehobenes Werkzeug und Farbe.
    pub fn set_selected(&self, tool: usize, color: usize) {
        #[cfg(target_os = "macos")]
        self.inner.set_selected(tool, color);
        #[cfg(not(target_os = "macos"))]
        let _ = (tool, color);
    }

    pub fn close(&self) {
        log::debug!("Zeichenfläche schliessen");
        #[cfg(target_os = "macos")]
        self.inner.close();
    }
}

/// Öffnet die Zeichenfläche (inaktiv) auf `display_id`. `area` ist der
/// aufgenommene Bereich in physischen Pixeln des Monitors; `colors` als
/// RGBA 0 bis 1. Muss im Haupt-Thread laufen.
pub fn open_draw_surface(
    display_id: u32,
    area: (u32, u32, u32, u32),
    tools: &[DrawTool<'_>],
    colors: &[[f64; 4]],
    on_event: Box<dyn Fn(DrawEvent)>,
) -> Result<DrawSurface, OverlayError> {
    let started = Instant::now();
    log::debug!(
        "Zeichenfläche öffnen: Monitor {display_id}, Bereich {area:?}, {} Werkzeuge, {} Farben",
        tools.len(),
        colors.len()
    );
    #[cfg(target_os = "macos")]
    let result = macos::draw::open(display_id, area, tools, colors, on_event)
        .map(|inner| DrawSurface { inner });
    #[cfg(not(target_os = "macos"))]
    let result = {
        let _ = (display_id, area, tools, colors, on_event);
        Err(OverlayError::NotImplemented)
    };
    logged("Zeichenfläche", started, result)
}
