//! Scroll-Aufnahme (ADR 0006): Bereich wählen, dann laufend aufnehmen und
//! zusammensetzen, während gescrollt wird – von Hand oder automatisch.

use std::cell::RefCell;
use std::sync::Arc;
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use grabmark_capture::PixelRect;
use grabmark_overlay::{ScrollAction, ScrollUi, Selection};
use grabmark_stitch::{Push, Stitcher};
use tauri::{AppHandle, Manager};

use crate::capture::{self, CaptureState};
use crate::i18n::t;

/// Höchste Bildhöhe in Pixeln (Schutz vor Endlos-Seiten).
const MAX_HEIGHT: u32 = 40_000;
/// Pause zwischen zwei Aufnahmen beim Scrollen von Hand.
const MANUAL_INTERVAL: Duration = Duration::from_millis(60);
/// Wartezeit nach einem automatischen Scroll-Schritt (Seite zeichnet nach).
const AUTO_SETTLE: Duration = Duration::from_millis(280);
/// So oft „nichts bewegt“ hintereinander = Seitenende erreicht.
const AUTO_END_AFTER: u32 = 3;

/// Laufende Scroll-Aufnahme – erlaubt „Fertig“ per Tastenkürzel.
#[derive(Default)]
pub struct ScrollState {
    session: Mutex<Option<Flags>>,
}

#[derive(Clone)]
struct Flags {
    finish: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
}

thread_local! {
    static UI: RefCell<Option<ScrollUi>> = const { RefCell::new(None) };
}

/// Läuft schon eine Scroll-Aufnahme? Dann beendet das Kürzel sie.
pub fn finish_running(app: &AppHandle) -> bool {
    let state = app.state::<ScrollState>();
    let guard = state.session.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(flags) = guard.as_ref() {
        flags.finish.store(true, Ordering::Release);
        true
    } else {
        false
    }
}

/// Wird nach der Bereichsauswahl aufgerufen.
pub fn begin(app: &AppHandle, sel: Selection, scale: f32, auto: bool) {
    let flags = Flags {
        finish: Arc::new(AtomicBool::new(false)),
        cancel: Arc::new(AtomicBool::new(false)),
    };
    *app.state::<ScrollState>()
        .session
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(flags.clone());

    let rect = (sel.x, sel.y, sel.width, sel.height);
    let ui_flags = flags.clone();
    let status = if auto {
        t("scroll.auto_running", &[])
    } else {
        t("scroll.hint", &[])
    };
    let (finish_label, cancel_label) = (t("scroll.finish", &[]), t("scroll.cancel", &[]));
    let _ = app.run_on_main_thread(move || {
        let ui = grabmark_overlay::open_scroll_ui(
            sel.display_id,
            rect,
            (&status, &finish_label, &cancel_label),
            Box::new(move |action| match action {
                ScrollAction::Finish => ui_flags.finish.store(true, Ordering::Release),
                ScrollAction::Cancel => ui_flags.cancel.store(true, Ordering::Release),
            }),
        );
        match ui {
            Ok(ui) => UI.with(|u| *u.borrow_mut() = Some(ui)),
            Err(e) => log::error!("Scroll-Leiste: {e}"),
        }
    });

    let app = app.clone();
    std::thread::spawn(move || run(&app, sel, scale, auto, flags));
}

fn run(app: &AppHandle, sel: Selection, scale: f32, auto: bool, flags: Flags) {
    let state = app.state::<CaptureState>();
    let rect = PixelRect {
        x: sel.x,
        y: sel.y,
        width: sel.width,
        height: sel.height,
    };
    // Mitte des Bereichs in globalen Punkten – dort wird automatisch gescrollt.
    let center = state.capturer.displays().ok().and_then(|ds| {
        let d = ds.into_iter().find(|d| d.id == sel.display_id)?;
        let s = d.scale_factor.max(1.0);
        Some((
            d.bounds.x + (sel.x as f32 + sel.width as f32 / 2.0) / s,
            d.bounds.y + (sel.y as f32 + sel.height as f32 / 2.0) / s,
        ))
    });
    // Ein Schritt ≈ 70 % der sichtbaren Höhe, damit sich Bilder überlappen.
    let step = ((sel.height as f32 / scale.max(1.0)) * 0.7).round() as i32;

    let mut stitcher = Stitcher::new(MAX_HEIGHT);
    let mut unchanged = 0;
    let mut error = None;
    // Kurz warten, bis das Overlay verschwunden ist.
    std::thread::sleep(Duration::from_millis(120));
    while !flags.finish.load(Ordering::Acquire) && !flags.cancel.load(Ordering::Acquire) {
        let frame = match state.capturer.capture_region(sel.display_id, rect) {
            Ok(f) => f,
            Err(e) => {
                error = Some(e.to_string());
                break;
            }
        };
        let result = stitcher.push(frame.image);
        let height = stitcher.height();
        let status = match result {
            Push::NoMatch => t("scroll.too_fast", &[]),
            _ if auto => t("scroll.auto_progress", &[("height", &height.to_string())]),
            _ => t("scroll.progress", &[("height", &height.to_string())]),
        };
        let _ = app.run_on_main_thread(move || {
            UI.with(|u| {
                if let Some(ui) = u.borrow().as_ref() {
                    ui.set_status(&status);
                }
            })
        });
        if stitcher.is_full() {
            break;
        }
        if auto {
            unchanged = if matches!(result, Push::Unchanged) {
                unchanged + 1
            } else {
                0
            };
            if unchanged >= AUTO_END_AFTER {
                break;
            }
            if let Some(c) = center
                && let Err(e) = state.capturer.scroll_at(c, step)
            {
                error = Some(e.to_string());
                break;
            }
            std::thread::sleep(AUTO_SETTLE);
        } else {
            std::thread::sleep(MANUAL_INTERVAL);
        }
    }

    *app.state::<ScrollState>()
        .session
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = None;
    let _ = app.run_on_main_thread(|| {
        UI.with(|u| {
            if let Some(ui) = u.borrow_mut().take() {
                ui.close();
            }
        })
    });

    if let Some(e) = error {
        capture::show_error(app, &e);
        return;
    }
    if flags.cancel.load(Ordering::Acquire) {
        return;
    }
    match stitcher.finish() {
        Some(image) => capture::finish_scroll(app, image, scale),
        None => log::warn!("Scroll-Aufnahme ohne Bild"),
    }
}
