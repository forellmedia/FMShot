//! Ablauf einer Bereichsaufnahme:
//! Tastenkürzel → alle Monitore einfrieren → natives Overlay → Ausschnitt →
//! Zwischenablage + Bibliothek (im Hintergrund).

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use fmshot_capture::{CaptureError, ScreenCapturer};
use fmshot_core::RgbaImage;
use fmshot_library::{Library, NewCapture};
use fmshot_overlay::{OverlayResult, OverlayScreen};
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::{DialogExt, MessageDialogKind};

use crate::i18n::t;

/// Gemeinsamer Zustand der App.
pub struct CaptureState {
    pub capturer: Box<dyn ScreenCapturer>,
    pub library: Arc<Library>,
    /// Verhindert zwei Overlays gleichzeitig (Taste doppelt gedrückt).
    busy: AtomicBool,
}

impl CaptureState {
    pub fn new(capturer: Box<dyn ScreenCapturer>, library: Arc<Library>) -> Self {
        Self {
            capturer,
            library,
            busy: AtomicBool::new(false),
        }
    }
}

/// Startet eine Bereichsaufnahme. Kehrt sofort zurück.
pub fn start_region(app: &AppHandle) {
    let started = Instant::now();
    let state = app.state::<CaptureState>();
    if state.busy.swap(true, Ordering::AcqRel) {
        return;
    }
    let app = app.clone();
    // Aufnehmen blockiert – nie im Haupt-Thread.
    std::thread::spawn(move || {
        let state = app.state::<CaptureState>();
        if !state.capturer.has_permission() {
            state.capturer.request_permission();
            state.busy.store(false, Ordering::Release);
            show_permission_dialog(&app);
            return;
        }
        let screens = match freeze_screens(state.capturer.as_ref()) {
            Ok(s) => s,
            Err(e) => {
                state.busy.store(false, Ordering::Release);
                show_error(&app, &e.to_string());
                return;
            }
        };
        let frozen_ms = started.elapsed().as_secs_f64() * 1000.0;

        let app2 = app.clone();
        let result = app.run_on_main_thread(move || {
            let images: Vec<(u32, f32, Arc<RgbaImage>)> = screens
                .iter()
                .map(|s| (s.display.id, s.display.scale_factor, s.image.clone()))
                .collect();
            let app3 = app2.clone();
            let opened = fmshot_overlay::select_region(
                screens,
                Box::new(move |result| finish(&app3, result, &images)),
            );
            match opened {
                Ok(()) => log::info!(
                    "Overlay sichtbar nach {:.1} ms (Einfrieren {frozen_ms:.1} ms)",
                    started.elapsed().as_secs_f64() * 1000.0
                ),
                Err(e) => {
                    app2.state::<CaptureState>()
                        .busy
                        .store(false, Ordering::Release);
                    show_error(&app2, &e.to_string());
                }
            }
        });
        if let Err(e) = result {
            log::error!("Haupt-Thread nicht erreichbar: {e}");
            state.busy.store(false, Ordering::Release);
        }
    });
}

/// Nimmt alle Monitore auf und ordnet die Bilder ihren Monitoren zu.
fn freeze_screens(capturer: &dyn ScreenCapturer) -> Result<Vec<OverlayScreen>, CaptureError> {
    let displays = capturer.displays()?;
    let frames = capturer.capture_all()?;
    Ok(frames
        .into_iter()
        .filter_map(|frame| {
            let display = displays.iter().find(|d| d.id == frame.display_id)?.clone();
            Some(OverlayScreen {
                display,
                image: Arc::new(frame.image),
            })
        })
        .collect())
}

/// Läuft im Haupt-Thread, sobald das Overlay geschlossen ist.
fn finish(app: &AppHandle, result: OverlayResult, images: &[(u32, f32, Arc<RgbaImage>)]) {
    let state = app.state::<CaptureState>();
    state.busy.store(false, Ordering::Release);
    let OverlayResult::Selected(sel) = result else {
        return;
    };
    let Some((_, scale, image)) = images.iter().find(|(id, _, _)| *id == sel.display_id) else {
        return;
    };
    let Some(cropped) = image.crop(sel.x, sel.y, sel.width, sel.height) else {
        return;
    };
    let capture = NewCapture {
        image: cropped,
        scale_factor: *scale,
        mode: "region".into(),
        created_at_ms: now_ms(),
    };
    let library = state.library.clone();
    // Kodieren und Schreiben im Hintergrund – die Oberfläche bleibt frei.
    std::thread::spawn(move || save_and_copy(&library, capture));
}

/// Zwischenablage und Bibliothek. Öffentlich für den Selbsttest.
pub fn save_and_copy(library: &Library, capture: NewCapture) {
    let start = Instant::now();
    copy_to_clipboard(&capture.image);
    let copied = start.elapsed().as_secs_f64() * 1000.0;
    match library.save_capture(&capture) {
        Ok(rec) => log::info!(
            "Aufnahme {} ({}×{}) – Zwischenablage {copied:.1} ms, gespeichert nach {:.1} ms",
            rec.id,
            rec.width,
            rec.height,
            start.elapsed().as_secs_f64() * 1000.0
        ),
        Err(e) => log::error!("Speichern fehlgeschlagen: {e}"),
    }
}

fn copy_to_clipboard(image: &RgbaImage) {
    let data = arboard::ImageData {
        width: image.width() as usize,
        height: image.height() as usize,
        bytes: std::borrow::Cow::Borrowed(image.data()),
    };
    let result = arboard::Clipboard::new().and_then(|mut c| c.set_image(data));
    if let Err(e) = result {
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

fn show_error(app: &AppHandle, error: &str) {
    log::error!("Aufnahme: {error}");
    app.dialog()
        .message(t("error.capture", &[("error", error)]))
        .kind(MessageDialogKind::Error)
        .show(|_| {});
}
