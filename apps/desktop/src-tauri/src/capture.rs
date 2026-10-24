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

use nabshot_capture::{
    Frame, OwnWindows, PixelRect, PrivacyPane, ScreenCapturer, SpanPiece, compose_span, display_at,
};
use nabshot_core::RgbaImage;
use nabshot_library::{CaptureRecord, Library, NewCapture};
use nabshot_overlay::{
    OverlayHandle, OverlayMode, OverlayResult, Selection, SpanSelection, WindowTarget,
};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

use crate::i18n::t;

/// Gemeinsamer Zustand der App.
pub struct CaptureState {
    pub capturer: Box<dyn ScreenCapturer>,
    pub library: Arc<Library>,
    pub ocr: Option<Box<dyn nabshot_ocr::OcrEngine>>,
    /// Verhindert zwei Aufnahmen gleichzeitig (Taste doppelt gedrückt).
    busy: AtomicBool,
    /// Laufende Aufnahme zeigt offene Fenster von Nabshot (Tastenkürzel) –
    /// das Tray-Fenster bleibt dann offen, obwohl das Overlay den Fokus hat.
    own_shown: AtomicBool,
}

impl CaptureState {
    pub fn new(capturer: Box<dyn ScreenCapturer>, library: Arc<Library>) -> Self {
        Self {
            capturer,
            library,
            ocr: nabshot_ocr::platform_engine(),
            busy: AtomicBool::new(false),
            own_shown: AtomicBool::new(false),
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
    /// Bereich wählen, Nabshot scrollt selbst.
    ScrollAuto,
    /// Video (ADR 0011) – Ziel aus den Einstellungen.
    Video,
    /// Video nur mit der Webcam (ohne Bildschirm).
    Webcam,
}

impl Mode {
    /// Scroll-Aufnahme so, wie in den Einstellungen gewählt (von Hand/automatisch).
    pub fn scroll(app: &AppHandle) -> Mode {
        match crate::settings::get(app).scroll_mode {
            nabshot_settings::ScrollMode::Manual => Mode::Scroll,
            nabshot_settings::ScrollMode::Auto => Mode::ScrollAuto,
        }
    }

    fn name(self) -> &'static str {
        match self {
            Mode::Region => "region",
            Mode::Window => "window",
            Mode::Screen => "screen",
            Mode::Text => "text",
            Mode::Scroll | Mode::ScrollAuto => "scroll",
            Mode::Video => "video",
            Mode::Webcam => "webcam",
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
    selection: Option<Chosen>,
    finished: bool,
}

/// Gewählter Bereich: auf einem Monitor oder über mehrere.
enum Chosen {
    One(Selection),
    Span(SpanSelection),
}

/// Startet eine Aufnahme ohne die Fenster von Nabshot. Kehrt sofort zurück.
pub fn start(app: &AppHandle, mode: Mode) {
    start_with(app, mode, OwnWindows::Hidden);
}

/// Wie [`start`]; `own` bestimmt, ob offene Fenster von Nabshot (Editor,
/// Einstellungen) mit ins Bild kommen – bei globalen Tastenkürzeln ja.
pub fn start_with(app: &AppHandle, mode: Mode, own: OwnWindows) {
    let started = Instant::now();
    log::info!("Aufnahme starten: {mode:?} (eigene Fenster: {own:?})");
    // Zweites Drücken während einer Scroll-Aufnahme = „Fertig“.
    if matches!(mode, Mode::Scroll | Mode::ScrollAuto) && crate::scroll::finish_running(app) {
        log::info!("Scroll-Aufnahme läuft schon → fertig");
        return;
    }
    // Zweites Drücken während eines Videos = „Stopp“.
    if matches!(mode, Mode::Video | Mode::Webcam) && crate::video::stop_running(app) {
        log::info!("Video läuft schon → Stopp");
        return;
    }
    // Nur Webcam braucht keine Bildschirm-Freigabe.
    if mode == Mode::Webcam {
        crate::video::start_webcam(app);
        return;
    }
    let state = app.state::<CaptureState>();
    if state.busy.swap(true, Ordering::AcqRel) {
        log::warn!("Aufnahme {mode:?} ignoriert – es läuft schon eine");
        return;
    }
    state
        .own_shown
        .store(own == OwnWindows::Shown, Ordering::Release);
    if !state.capturer.has_permission() {
        release(app);
        // Beim ersten Mal fragt macOS mit eigenem Fenster (mit Knopf zu den
        // Einstellungen). Danach fragt es nicht mehr – dann direkt dorthin.
        let first = first_permission_request("screen");
        log::warn!("Bildschirm-Freigabe fehlt (erste Anfrage: {first})");
        if first {
            state.capturer.request_permission();
        } else {
            open_privacy_settings(app, PrivacyPane::ScreenCapture);
        }
        return;
    }
    if mode == Mode::ScrollAuto && !state.capturer.has_input_permission() {
        log::warn!("Bedienungshilfen-Freigabe fehlt für automatisches Scrollen");
        release(app);
        // Trägt Nabshot in die Liste „Bedienungshilfen“ ein (beim ersten Mal
        // mit Systemfenster); die Einstellungen öffnen wir direkt.
        let first = first_permission_request("input");
        state.capturer.request_input_permission();
        if !first {
            open_privacy_settings(app, PrivacyPane::Accessibility);
        }
        return;
    }
    match mode {
        // Scroll-Aufnahmen setzen Ausschnitte ohne Nabshot zusammen.
        Mode::Scroll | Mode::ScrollAuto => start_region(app, started, mode, OwnWindows::Hidden),
        Mode::Region | Mode::Text => start_region(app, started, mode, own),
        Mode::Window => start_window(app, started, own),
        Mode::Screen => start_screen(app, started, own),
        Mode::Video => {
            // Das Video verwaltet seinen Zustand selbst.
            release(app);
            crate::video::start(app, None);
        }
        Mode::Webcam => release(app),
    }
}

fn release(app: &AppHandle) {
    let state = app.state::<CaptureState>();
    state.busy.store(false, Ordering::Release);
    // Fokus zurück ans offene Tray-Fenster – sonst schlösse es sich beim
    // nächsten Klick daneben nicht mehr.
    if state.own_shown.swap(false, Ordering::AcqRel) {
        crate::tray::refocus_popover(app);
    }
}

/// Läuft eine Aufnahme, auf der Nabshot selbst zu sehen sein soll?
pub fn shows_own_windows(app: &AppHandle) -> bool {
    app.state::<CaptureState>()
        .own_shown
        .load(Ordering::Acquire)
}

fn start_region(app: &AppHandle, started: Instant, mode: Mode, own: OwnWindows) {
    let pending = Arc::new(Mutex::new(Pending::default()));

    // 1. Overlay sofort öffnen (Haupt-Thread).
    let (app_main, pending_main) = (app.clone(), pending.clone());
    // Über Monitore hinweg nur auf Wunsch und nur für Screenshots: Scroll-
    // Aufnahmen bleiben auf einem Monitor.
    let overlay_mode = if matches!(mode, Mode::Region | Mode::Text)
        && crate::settings::get(app).region_across_screens
    {
        OverlayMode::RegionAcrossScreens
    } else {
        OverlayMode::Region
    };
    let opened = app.run_on_main_thread(move || {
        let (app_done, pending_done) = (app_main.clone(), pending_main.clone());
        let result = nabshot_overlay::open_awaiting_frozen(
            overlay_mode,
            Box::new(move |result| {
                OVERLAY.with(|o| o.borrow_mut().take());
                let chosen = match result {
                    OverlayResult::Selected(sel) => Ok(Chosen::One(sel)),
                    OverlayResult::Spanned(span) => Ok(Chosen::Span(span)),
                    other => Err(other),
                };
                match chosen {
                    Ok(chosen) => {
                        log::debug!(
                            "Bereich gewählt nach {:.0} ms",
                            started.elapsed().as_secs_f64() * 1000.0
                        );
                        pending_done
                            .lock()
                            .unwrap_or_else(|e| e.into_inner())
                            .selection = Some(chosen);
                        try_finish_region(&app_done, &pending_done, mode, started);
                    }
                    Err(other) => {
                        log::info!("Auswahl beendet ohne Bereich: {other:?}");
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
        match state.capturer.capture_all(own) {
            Ok(frames) => {
                crate::services::timing(
                    &app,
                    format!(
                        "Taste → Bildschirm eingefroren: {:.0} ms",
                        started.elapsed().as_secs_f64() * 1000.0
                    ),
                );
                for f in &frames {
                    log::debug!(
                        "Monitor {} aufgenommen: {}×{}, Massstab {}",
                        f.display_id,
                        f.image.width(),
                        f.image.height(),
                        f.scale_factor
                    );
                }
                let images: Vec<(u32, Arc<RgbaImage>)> = frames
                    .iter()
                    .map(|f| (f.display_id, Arc::new(f.image.clone())))
                    .collect();
                if let Err(e) = app.run_on_main_thread(move || {
                    OVERLAY.with(|o| {
                        if let Some(h) = o.borrow().as_ref() {
                            for (id, img) in images {
                                h.set_frozen(id, img);
                            }
                        }
                    });
                }) {
                    log::warn!("Eingefrorenes Bild ans Overlay: {e}");
                }
                pending.lock().unwrap_or_else(|e| e.into_inner()).frames = Some(frames);
                try_finish_region(&app, &pending, mode, started);
            }
            Err(e) => {
                if let Err(e) = app.run_on_main_thread(|| {
                    OVERLAY.with(|o| {
                        if let Some(h) = o.borrow_mut().take() {
                            h.cancel();
                        }
                    });
                }) {
                    log::warn!("Overlay schliessen: {e}");
                }
                release(&app);
                show_error(&app, &e.to_string());
            }
        }
    });
}

/// Schliesst ab, sobald Bild und Auswahl beide da sind.
fn try_finish_region(app: &AppHandle, pending: &Arc<Mutex<Pending>>, mode: Mode, started: Instant) {
    let (frames, sel) = {
        let mut p = pending.lock().unwrap_or_else(|e| e.into_inner());
        if p.finished || p.frames.is_none() || p.selection.is_none() {
            return;
        }
        p.finished = true;
        (p.frames.take().unwrap_or_default(), p.selection.take())
    };
    release(app);
    let sel = match sel {
        Some(Chosen::One(sel)) => sel,
        Some(Chosen::Span(span)) => {
            finish_span(app, mode, &frames, &span, started);
            return;
        }
        None => return,
    };
    let Some(frame) = frames.into_iter().find(|f| f.display_id == sel.display_id) else {
        log::warn!("Kein Bild für Monitor {} der Auswahl", sel.display_id);
        return;
    };
    log::info!(
        "Bereich gewählt: {}×{} auf Monitor {}, Massstab {}",
        sel.width,
        sel.height,
        sel.display_id,
        frame.scale_factor
    );
    if matches!(mode, Mode::Scroll | Mode::ScrollAuto) {
        crate::scroll::begin(app, sel, frame.scale_factor, mode == Mode::ScrollAuto);
        return;
    }
    let Some(image) = frame.image.crop(sel.x, sel.y, sel.width, sel.height) else {
        log::warn!(
            "Ausschnitt {}×{} bei {},{} liegt ausserhalb des Bildes {}×{}",
            sel.width,
            sel.height,
            sel.x,
            sel.y,
            frame.image.width(),
            frame.image.height()
        );
        return;
    };
    if mode == Mode::Text {
        recognize_and_copy(app, image);
    } else {
        finish(app, mode, image, frame.scale_factor, started);
    }
}

/// Bereich über mehrere Monitore: Teile zu einem Bild zusammensetzen.
fn finish_span(
    app: &AppHandle,
    mode: Mode,
    frames: &[Frame],
    span: &SpanSelection,
    started: Instant,
) {
    let pieces: Vec<SpanPiece<'_>> = span
        .parts
        .iter()
        .filter_map(|part| {
            let frame = frames.iter().find(|f| f.display_id == part.display_id);
            if frame.is_none() {
                log::warn!("Kein Bild für Monitor {} der Auswahl", part.display_id);
            }
            frame.map(|f| SpanPiece {
                image: &f.image,
                src: PixelRect {
                    x: part.x,
                    y: part.y,
                    width: part.width,
                    height: part.height,
                },
                dest: part.dest,
                dest_size: part.dest_size,
                scale: f.scale_factor,
            })
        })
        .collect();
    let compose_started = Instant::now();
    let Some((image, scale)) = compose_span(span.size, &pieces) else {
        log::warn!("Bereich über Monitore: kein Bild");
        return;
    };
    log::info!(
        "Bereich über {} Monitore: {}×{}, Massstab {scale}, zusammengesetzt in {:.0} ms",
        pieces.len(),
        image.width(),
        image.height(),
        compose_started.elapsed().as_secs_f64() * 1000.0
    );
    if mode == Mode::Text {
        recognize_and_copy(app, image);
    } else {
        finish(app, mode, image, scale, started);
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
        let ms = || started.elapsed().as_secs_f64() * 1000.0;
        let message = match ocr.recognize(&image) {
            Ok(blocks) if !blocks.is_empty() => {
                let text = nabshot_ocr::join_text(&blocks);
                let count = text.chars().count();
                log::info!(
                    "Texterkennung: {} Blöcke, {count} Zeichen in {:.0} ms",
                    blocks.len(),
                    ms()
                );
                match nabshot_clipboard::copy_text(&text) {
                    Ok(()) => {
                        log::info!("Zwischenablage: Text kopiert");
                        t("ocr.copied", &[("count", &count.to_string())])
                    }
                    Err(e) => {
                        log::error!("Zwischenablage (Text): {e}");
                        e.to_string()
                    }
                }
            }
            Ok(_) => {
                log::info!("Texterkennung: kein Text gefunden ({:.0} ms)", ms());
                t("ocr.nothing", &[])
            }
            Err(e) => {
                log::error!("Texterkennung nach {:.0} ms: {e}", ms());
                e.to_string()
            }
        };
        if let Err(e) = app.run_on_main_thread(move || nabshot_overlay::show_toast(&message)) {
            log::debug!("Hinweis anzeigen: {e}");
        }
    });
}

fn start_window(app: &AppHandle, started: Instant, own: OwnWindows) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<CaptureState>();
        let list_start = Instant::now();
        let windows: Vec<WindowTarget> = match state.capturer.windows(own) {
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
        log::debug!(
            "Fensterliste: {} Fenster in {:.0} ms",
            windows.len(),
            list_start.elapsed().as_secs_f64() * 1000.0
        );
        let app_main = app.clone();
        let opened = app.run_on_main_thread(move || {
            let app_done = app_main.clone();
            let result = nabshot_overlay::open(
                OverlayMode::Window,
                windows,
                Box::new(move |result| {
                    OVERLAY.with(|o| o.borrow_mut().take());
                    let OverlayResult::Window(id) = result else {
                        log::info!("Fensterauswahl beendet ohne Fenster: {result:?}");
                        release(&app_done);
                        return;
                    };
                    log::debug!("Fenster {id} gewählt");
                    let app = app_done.clone();
                    // Kurz warten, bis das Overlay wirklich weg ist.
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_millis(60));
                        let state = app.state::<CaptureState>();
                        let shot = Instant::now();
                        let result = state.capturer.capture_window(id);
                        release(&app);
                        match result {
                            Ok(frame) => {
                                log::info!(
                                    "Fenster aufgenommen: {}×{} auf Monitor {}, Massstab {} ({:.0} ms)",
                                    frame.image.width(),
                                    frame.image.height(),
                                    frame.display_id,
                                    frame.scale_factor,
                                    shot.elapsed().as_secs_f64() * 1000.0
                                );
                                finish(&app, Mode::Window, frame.image, frame.scale_factor, started)
                            }
                            Err(e) => show_error(&app, &e.to_string()),
                        }
                    });
                }),
            );
            match result {
                Ok(handle) => {
                    OVERLAY.with(|o| *o.borrow_mut() = Some(handle));
                    log::debug!(
                        "Fenster-Overlay sichtbar nach {:.0} ms",
                        started.elapsed().as_secs_f64() * 1000.0
                    );
                }
                Err(e) => {
                    release(&app_main);
                    show_error(&app_main, &e.to_string());
                }
            }
        });
        if let Err(e) = opened {
            log::error!("Fenster-Overlay: Haupt-Thread nicht erreichbar: {e}");
        }
    });
}

fn start_screen(app: &AppHandle, started: Instant, own: OwnWindows) {
    let app = app.clone();
    std::thread::spawn(move || {
        let state = app.state::<CaptureState>();
        let cursor = state.capturer.cursor_position();
        let result = state.capturer.displays().and_then(|displays| {
            let target = cursor
                .and_then(|c| display_at(&displays, c))
                .or_else(|| displays.iter().find(|d| d.is_primary))
                .map(|d| d.id);
            let frames = state.capturer.capture_all(own)?;
            Ok(frames.into_iter().find(|f| Some(f.display_id) == target))
        });
        release(&app);
        match result {
            Ok(Some(frame)) => {
                log::info!(
                    "Monitor {} aufgenommen: {}×{}, Massstab {} ({:.0} ms)",
                    frame.display_id,
                    frame.image.width(),
                    frame.image.height(),
                    frame.scale_factor,
                    started.elapsed().as_secs_f64() * 1000.0
                );
                finish(&app, Mode::Screen, frame.image, frame.scale_factor, started)
            }
            Ok(None) => log::warn!("Vollbild: kein Monitor unter der Maus gefunden"),
            Err(e) => show_error(&app, &e.to_string()),
        }
    });
}

/// Ergebnis einer Scroll-Aufnahme.
/// `started` = Beginn der Scroll-Aufnahme (für die Zeitmessung).
pub fn finish_scroll(app: &AppHandle, image: RgbaImage, scale_factor: f32, started: Instant) {
    finish(app, Mode::Scroll, image, scale_factor, started);
}

/// Zwischenablage und Bibliothek (im Hintergrund), danach Editor zeigen.
/// `started` = Auslöser (Taste, Menü, …) – für die Zeitmessung.
fn finish(app: &AppHandle, mode: Mode, image: RgbaImage, scale_factor: f32, started: Instant) {
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
            log::info!(
                "Aufnahme {} gespeichert: {}×{}, {}, {:.0} ms seit Auslöser",
                rec.id,
                rec.width,
                rec.height,
                rec.mode,
                started.elapsed().as_secs_f64() * 1000.0
            );
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
    } else {
        log::debug!("Zwischenablage aus (Einstellung)");
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
    match nabshot_clipboard::copy_image(image, scale_factor) {
        Ok(()) => log::info!(
            "Zwischenablage: Bild {}×{} kopiert",
            image.width(),
            image.height()
        ),
        Err(e) => log::error!("Zwischenablage: {e}"),
    }
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Erste Anfrage nach einer Freigabe (`screen`, `input`, …)? Merkt sie sich
/// in einer Markierungsdatei im Konfigurationsordner.
pub fn first_permission_request(kind: &str) -> bool {
    let dir = &crate::paths::get().config;
    let marker = dir.join(format!("{kind}-permission-requested"));
    if marker.exists() {
        log::debug!("Freigabe {kind} wurde schon einmal angefragt");
        return false;
    }
    if let Err(e) = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(&marker, b"")) {
        log::warn!("Freigabe-Markierung: {e}");
    }
    true
}

/// Öffnet direkt die passende Stelle der Systemeinstellungen – ohne eigenen
/// Hinweis dazwischen.
pub fn open_privacy_settings(app: &AppHandle, pane: PrivacyPane) {
    use tauri_plugin_opener::OpenerExt;
    let Some(url) = nabshot_capture::privacy_settings_url(pane) else {
        log::debug!("Systemeinstellungen für {pane:?} gibt es hier nicht");
        return;
    };
    log::info!("Systemeinstellungen öffnen: {pane:?}");
    if let Err(e) = app.opener().open_url(url, None::<&str>) {
        log::error!("Systemeinstellungen öffnen: {e}");
    }
}

pub fn show_error(app: &AppHandle, error: &str) {
    log::error!("Aufnahme: {error}");
    app.dialog()
        .message(t("error.capture", &[("error", error)]))
        .kind(MessageDialogKind::Error)
        .show(|_| {});
}
