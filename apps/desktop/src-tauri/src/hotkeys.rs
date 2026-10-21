//! Registriert die globalen Tastenkürzel beim System.

use grabmark_hotkeys::{Action, Platform};
use tauri::{AppHandle, Emitter};
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::capture;
use crate::tray::show_main_window;

pub fn unregister_all(app: &AppHandle) {
    if let Err(e) = app.global_shortcut().unregister_all() {
        log::warn!("Tastenkürzel abmelden: {e}");
    }
}

/// Registriert die Kürzel aus den Einstellungen (vorher alle abmelden).
pub fn register(app: &AppHandle) {
    unregister_all(app);
    let platform = Platform::current();
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
            Ok(()) => log::info!("Tastenkürzel {} → {action:?}", shortcut.label(platform)),
            Err(e) => log::warn!("Tastenkürzel {accelerator} nicht registriert: {e}"),
        }
    }
}

pub fn run(app: &AppHandle, action: Action) {
    match action {
        Action::CaptureRegion => capture::start(app, capture::Mode::Region),
        Action::CaptureWindow => capture::start(app, capture::Mode::Window),
        Action::CaptureScreen => capture::start(app, capture::Mode::Screen),
        Action::CaptureText => capture::start(app, capture::Mode::Text),
        Action::CaptureVideo => capture::start(app, capture::Mode::Video),
        Action::CaptureWebcam => capture::start(app, capture::Mode::Webcam),
        Action::RecordDraw => crate::video::toggle(app, crate::video::Toggle::Draw),
        Action::RecordPause => crate::video::toggle(app, crate::video::Toggle::Pause),
        Action::OpenGallery => {
            show_main_window(app);
            let _ = app.emit("open-gallery", ());
        }
        Action::CaptureScroll => capture::start(app, capture::Mode::scroll(app)),
    }
}
