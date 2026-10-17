//! Ablauf einer Aufnahme.
//!
//! **Bereich:** Tastenkürzel → Overlay sofort öffnen (durchsichtig) und
//! gleichzeitig alle Monitore aufnehmen → eingefrorenes Bild ins Overlay →
//! Auswahl → Ausschnitt → Zwischenablage + Bibliothek (im Hintergrund).
//!
//! **Fenster:** Fensterliste holen → Overlay mit Hervorhebung → Klick →
//! Fenster einzeln aufnehmen (mit runden Ecken).
//!
//! **Vollbild:** Monitor unter der Maus direkt aufnehmen, ohne Overlay.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use fmshot_capture::{Frame, ScreenCapturer, display_at};
use fmshot_core::RgbaImage;
use fmshot_library::{CaptureRecord, Library, NewCapture};
use fmshot_overlay::{OverlayHandle, OverlayMode, OverlayResult, Selection, WindowTarget};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

use crate::i18n::t;

/// Gemeinsamer Zustand der App.
pub struct CaptureState {
    pub capturer: Box<dyn ScreenCapturer>,
    pub library: Arc<Library>,
    pub ocr: Option<Box<dyn fmshot_ocr::OcrEngine>>,
    /// Verhindert zwei Aufnahmen gleichzeitig (Taste doppelt gedrückt).
    busy: AtomicBool,
}

impl CaptureState {
    pub fn new(capturer: Box<dyn ScreenCapturer>, library: Arc<Library>) -> Self {
        Self {
            capturer,
            library,
            ocr: fmshot_ocr::platform_engine(),
            busy: AtomicBool::new(false),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Region,
    Window,
    Screen,
    /// Bereich wählen, Text erkennen, Text in die Zwischenablage.
    Text,
    /// Bereich wählen, dann von Hand scrollen.
    Scroll,
    /// Bereich wählen, FMShot scrollt selbst.
    ScrollAuto,
}

impl Mode {
    /// Scroll-Aufnahme so, wie in den Einstellungen gewählt (von Hand/automatisch).
    pub fn scroll(app: &AppHandle) -> Mode {
        match crate::settings::get(app).scroll_mode {
            fmshot_settings::ScrollMode::Manual => Mode::Scroll,
            fmshot_settings::ScrollMode::Auto => Mode::ScrollAuto,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Mode::Region => "region",
            Mode::Window => "window",
            Mode::Screen => "screen",
            Mode::Text => "text",
            Mode::Scroll | Mode::ScrollAuto => "scroll",
        }
    }
}

thread_local! {
    /// Das offene Overlay – lebt nur im Haupt-Thread.
    static OVERLAY: RefCell<Option<OverlayHandle>> = const { RefCell::new(None) };
}

/// Was von einer Bereichsaufnahme schon da ist. Bild und Auswahl kommen in
/// beliebiger Reihenfolge an; wer zuletzt kommt, schliesst ab.
#[derive(Default)]
struct Pending {
    frames: Option<Vec<Frame>>,
    selection: Option<Selection>,
    finished: bool,
}

/// Startet eine Aufnahme. Kehrt sofort zurück.
pub fn start(app: &AppHandle, mode: Mode) {
    let started = Instant::now();
    // Zweites Drücken während einer Scroll-Aufnahme = „Fertig“.
    if matches!(mode, Mode::Scroll | Mode::ScrollAuto) && crate::scroll::finish_running(app) {
        return;
    }
    let state = app.state::<CaptureState>();
    if state.busy.swap(true, Ordering::AcqRel) {
        return;
    }
    if !state.capturer.has_permission() {
        state.capturer.request_permission();
        release(app);
        show_permission_dialog(app);
        return;
    }
    if mode == Mode::ScrollAuto && !state.capturer.has_input_permission() {
        state.capturer.request_input_permission();
        release(app);
        show_info(
            app,
            &t("scroll.permission_title", &[]),
            &t("scroll.permission_body", &[]),
        );
        return;
    }
    match mode {
        Mode::Region | Mode::Text | Mode::Scroll | Mode::ScrollAuto => {
            start_region(app, started, mode)
        }
        Mode::Window => start_window(app),
        Mode::Screen => start_screen(app),
    }
}

fn release(app: &AppHandle) {
    app.state::<CaptureState>()
        .busy
        .store(false, Ordering::Release);
}

fn start_region(app: &AppHandle, started: Instant, mode: Mode) {
    let pending = Arc::new(Mutex::new(Pending::default()));

    // 1. Overlay sofort öffnen (Haupt-Thread).
    let (app_main, pending_main) = (app.clone(), pending.clone());
    let opened = app.run_on_main_thread(move || {
        let (app_done, pending_done) = (app_main.clone(), pending_main.clone());
        let result = fmshot_overlay::open(
            OverlayMode::Region,
            Vec::new(),
            Box::new(move |result| {
                OVERLAY.with(|o| o.borrow_mut().take());
                match result {
                    OverlayResult::Selected(sel) => {
                        pending_done
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .selection = Some(sel);
                        try_finish_region(&app_done, &pending_done, mode);
                    }
                    _ => {
                        pending_done
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .finished = true;
                        release(&app_done);
                    }
                }
            }),
        );
        match result {
            Ok(handle) => {
                OVERLAY.with(|o| *o.borrow_mut() = Some(handle));
                crate::services::timing(
                    &app_main,
                    format!(
                        "Taste → Overlay sichtbar: {:.0} ms",
                        started.elapsed().as_secs_f64() * 1000.0
                    ),
                );
            }
            Err(e) => {
                release(&app_main);
                show_error(&app_main, &e.to_string());
            }
        }
    });
    if let Err(e) = opened {
        log::error!("Haupt-Thread nicht erreichbar: {e}");
        release(app);
        return;
    }

    // 2. Gleichzeitig aufnehmen (Hintergrund).
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<CaptureState>();
        match state.capturer.capture_all() {
            Ok(frames) => {
                crate::services::timing(
                    &app,
                    format!(
                        "Taste → Bildschirm eingefroren: {:.0} ms",
                        started.elapsed().as_secs_f64() * 1000.0
                    ),
                );
                let images: Vec<(u32, Arc<RgbaImage>)> = frames
                    .iter()
                    .map(|f| (f.display_id, Arc::new(f.image.clone())))
                    .collect();
                let _ = app.run_on_main_thread(move || {
                    OVERLAY.with(|o| {
                        if let Some(h) = o.borrow().as_ref() {
                            for (id, img) in images {
                                h.set_frozen(id, img);
                            }
                        }
                    });
                });
                pending.lock().unwrap_or_else(|e| e.into_inner()).frames = Some(frames);
                try_finish_region(&app, &pending, mode);
            }
            Err(e) => {
                let _ = app.run_on_main_thread(|| {
                    OVERLAY.with(|o| {
                        if let Some(h) = o.borrow_mut().take() {
                            h.cancel();
                        }
                    });
                });
                release(&app);
                show_error(&app, &e.to_string());
            }
        }
    });
}

/// Schliesst ab, sobald Bild und Auswahl beide da sind.
fn try_finish_region(app: &AppHandle, pending: &Arc<Mutex<Pending>>, mode: Mode) {
    let (frames, sel) = {
        let mut p = pending.lock().unwrap_or_else(|e| e.into_inner());
        if p.finished || p.frames.is_none() || p.selection.is_none() {
            return;
        }
        p.finished = true;
        (p.frames.take().unwrap_or_default(), p.selection.take())
    };
    release(app);
    let Some(sel) = sel else { return };
    let Some(frame) = frames.into_iter().find(|f| f.display_id == sel.display_id) else {
        return;
    };
    if matches!(mode, Mode::Scroll | Mode::ScrollAuto) {
        crate::scroll::begin(app, sel, frame.scale_factor, mode == Mode::ScrollAuto);
        return;
    }
    let Some(image) = frame.image.crop(sel.x, sel.y, sel.width, sel.height) else {
        return;
    };
    if mode == Mode::Text {
        recognize_and_copy(app, image);
    } else {
        finish(app, mode, image, frame.scale_factor);
    }
}

/// „Text erfassen“: erkennen, kopieren, kurz Bescheid geben.
fn recognize_and_copy(app: &AppHandle, image: RgbaImage) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<CaptureState>();
        let Some(ocr) = state.ocr.as_ref() else {
            show_error(&app, &t("ocr.unavailable", &[]));
            return;
        };
        let started = Instant::now();
        let message = match ocr.recognize(&image) {
            Ok(blocks) if !blocks.is_empty() => {
                let text = fmshot_ocr::join_text(&blocks);
                match fmshot_clipboard::copy_text(&text) {
                    Ok(()) => t(
                        "ocr.copied",
                        &[("count", &text.chars().count().to_string())],
                    ),
                    Err(e) => e.to_string(),
                }
            }
            Ok(_) => t("ocr.nothing", &[]),
            Err(e) => e.to_string(),
        };
        log::info!(
            "Texterkennung in {:.0} ms: {message}",
            started.elapsed().as_secs_f64() * 1000.0
        );
        let _ = app.run_on_main_thread(move || fmshot_overlay::show_toast(&message));
    });
}

fn start_window(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<CaptureState>();
        let windows: Vec<WindowTarget> = match state.capturer.windows() {
            Ok(w) => w
                .into_iter()
                .map(|w| WindowTarget {
                    id: w.id,
                    bounds: (
                        w.bounds.x as f64,
                        w.bounds.y as f64,
                        w.bounds.width as f64,
                        w.bounds.height as f64,
                    ),
                })
                .collect(),
            Err(e) => {
                release(&app);
                show_error(&app, &e.to_string());
                return;
            }
        };
        let app_main = app.clone();
        let _ = app.run_on_main_thread(move || {
            let app_done = app_main.clone();
            let result = fmshot_overlay::open(
                OverlayMode::Window,
                windows,
                Box::new(move |result| {
                    OVERLAY.with(|o| o.borrow_mut().take());
                    let OverlayResult::Window(id) = result else {
                        release(&app_done);
                        return;
                    };
                    let app = app_done.clone();
                    // Kurz warten, bis das Overlay wirklich weg ist.
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(60));
                        let state = app.state::<CaptureState>();
                        let result = state.capturer.capture_window(id);
                        release(&app);
                        match result {
                            Ok(frame) => {
                                finish(&app, Mode::Window, frame.image, frame.scale_factor)
                            }
                            Err(e) => show_error(&app, &e.to_string()),
                        }
                    });
                }),
            );
            match result {
                Ok(handle) => OVERLAY.with(|o| *o.borrow_mut() = Some(handle)),
                Err(e) => {
                    release(&app_main);
                    show_error(&app_main, &e.to_string());
                }
            }
        });
    });
}

fn start_screen(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<CaptureState>();
        let cursor = state.capturer.cursor_position();
        let result = state.capturer.displays().and_then(|displays| {
            let target = cursor
                .and_then(|c| display_at(&displays, c))
                .or_else(|| displays.iter().find(|d| d.is_primary))
                .map(|d| d.id);
            let frames = state.capturer.capture_all()?;
            Ok(frames.into_iter().find(|f| Some(f.display_id) == target))
        });
        release(&app);
        match result {
            Ok(Some(frame)) => finish(&app, Mode::Screen, frame.image, frame.scale_factor),
            Ok(None) => {}
            Err(e) => show_error(&app, &e.to_string()),
        }
    });
}

/// Ergebnis einer Scroll-Aufnahme.
pub fn finish_scroll(app: &AppHandle, image: RgbaImage, scale_factor: f32) {
    finish(app, Mode::Scroll, image, scale_factor);
}

/// Zwischenablage und Bibliothek (im Hintergrund), danach Editor zeigen.
fn finish(app: &AppHandle, mode: Mode, image: RgbaImage, scale_factor: f32) {
    let capture = NewCapture {
        image,
        scale_factor,
        mode: mode.name().into(),
        created_at_ms: now_ms(),
    };
    let app = app.clone();
    std::thread::spawn(move || {
        let library = app.state::<CaptureState>().library.clone();
        let copy = crate::settings::get(&app).copy_to_clipboard;
        if let Some(rec) = save_and_copy(&app, &library, &capture, copy) {
            crate::services::after_capture(&app, &rec, &capture.image, mode);
        }
    });
}

/// Zwischenablage und Bibliothek.
pub fn save_and_copy(
    app: &AppHandle,
    library: &Library,
    capture: &NewCapture,
    copy: bool,
) -> Option<CaptureRecord> {
    let start = Instant::now();
    if copy {
        copy_to_clipboard(&capture.image, capture.scale_factor);
    }
    let copied = start.elapsed().as_secs_f64() * 1000.0;
    match library.save_capture(capture) {
        Ok(rec) => {
            crate::services::timing(
                app,
                format!(
                    "Aufnahme {}×{}: Zwischenablage {copied:.0} ms, gespeichert nach {:.0} ms",
                    rec.width,
                    rec.height,
                    start.elapsed().as_secs_f64() * 1000.0
                ),
            );
            Some(rec)
        }
        Err(e) => {
            log::error!("Speichern fehlgeschlagen: {e}");
            None
        }
    }
}

pub fn copy_to_clipboard(image: &RgbaImage, scale_factor: f32) {
    if let Err(e) = fmshot_clipboard::copy_image(image, scale_factor) {
        log::error!("Zwischenablage: {e}");
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

fn show_permission_dialog(app: &AppHandle) {
    let opener = app.clone();
    app.dialog()
        .message(t("permission.body", &[]))
        .title(t("permission.title", &[]))
        .kind(MessageDialogKind::Info)
        .show(move |_| {
            #[cfg(target_os = "macos")]
            {
                use tauri_plugin_opener::OpenerExt;
                let url =
                    "x-apple.systempreferences:com.apple.preference.security?Privacy_ScreenCapture";
                if let Err(e) = opener.opener().open_url(url, None::<&str>) {
                    log::error!("Systemeinstellungen öffnen: {e}");
                }
            }
            let _ = &opener;
        });
}

pub fn show_info(app: &AppHandle, title: &str, body: &str) {
    app.dialog()
        .message(body)
        .title(title)
        .kind(MessageDialogKind::Info)
        .show(|_| {});
}

pub fn show_error(app: &AppHandle, error: &str) {
    log::error!("Aufnahme: {error}");
    app.dialog()
        .message(t("error.capture", &[("error", error)]))
        .kind(MessageDialogKind::Error)
        .show(|_| {});
}
