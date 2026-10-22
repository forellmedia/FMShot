//! Registriert die globalen Tastenkürzel beim System.

use nabshot_capture::OwnWindows;
use nabshot_hotkeys::{Action, Platform};
use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::capture;
use crate::tray::show_main_window;

pub fn unregister_all(app: &AppHandle) {
    match app.global_shortcut().unregister_all() {
        Ok(()) => log::debug!("Tastenkürzel abgemeldet"),
        Err(e) => log::warn!("Tastenkürzel abmelden: {e}"),
    }
}

/// Registriert die Kürzel aus den Einstellungen (vorher alle abmelden).
pub fn register(app: &AppHandle) {
    let start = std::time::Instant::now();
    unregister_all(app);
    let platform = Platform::current();
    let (mut ok, mut failed) = (0, 0);
    for (action, shortcut) in crate::settings::get(app).effective_hotkeys(platform) {
        let accelerator = shortcut.accelerator(platform);
        let result = app.global_shortcut().on_shortcut(
            accelerator.as_str(),
            move |app, _shortcut, event| {
                if event.state == ShortcutState::Pressed {
                    run(app, action);
                }
            },
        );
        match result {
            Ok(()) => {
                ok += 1;
                log::info!("Tastenkürzel {} → {action:?}", shortcut.label(platform));
            }
            Err(e) => {
                failed += 1;
                log::warn!("Tastenkürzel {accelerator} nicht registriert: {e}");
            }
        }
    }
    log::debug!(
        "Tastenkürzel: {ok} registriert, {failed} fehlgeschlagen ({:.0} ms)",
        start.elapsed().as_secs_f64() * 1000.0
    );
}

/// Führt ein Tastenkürzel aus. Offene Fenster von Nabshot bleiben dabei im
/// Bild – so lässt sich Nabshot selbst fotografieren.
pub fn run(app: &AppHandle, action: Action) {
    log::info!("Tastenkürzel ausgelöst: {action:?}");
    let start = |mode| capture::start_with(app, mode, OwnWindows::Shown);
    match action {
        Action::CaptureRegion => start(capture::Mode::Region),
        Action::CaptureWindow => start(capture::Mode::Window),
        Action::CaptureScreen => start(capture::Mode::Screen),
        Action::CaptureText => start(capture::Mode::Text),
        Action::CaptureVideo => start(capture::Mode::Video),
        Action::CaptureWebcam => start(capture::Mode::Webcam),
        Action::RecordDraw => crate::video::toggle(app, crate::video::Toggle::Draw),
        Action::RecordPause => crate::video::toggle(app, crate::video::Toggle::Pause),
        Action::OpenGallery => {
            show_main_window(app);
            if let Err(e) = app.emit("open-gallery", ()) {
                log::debug!("Ereignis open-gallery: {e}");
            }
        }
        Action::CaptureScroll => start(capture::Mode::scroll(app)),
    }
}
