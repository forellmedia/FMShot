//! Registriert die globalen Tastenkürzel beim System.

use fmshot_hotkeys::{Action, Platform, default_bindings};
use tauri::AppHandle;
use tauri_plugin_global_shortcut::{GlobalShortcutExt, ShortcutState};

use crate::capture;
use crate::tray::show_main_window;

pub fn register_defaults(app: &AppHandle) {
    let platform = Platform::current();
    // Bis es Einstellungen gibt: macOS-Systemkürzel nicht ersetzen (ADR/Produkt).
    for (action, shortcut) in default_bindings(platform, false) {
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

fn run(app: &AppHandle, action: Action) {
    match action {
        Action::CaptureRegion => capture::start_region(app),
        Action::OpenGallery => show_main_window(app),
        // Folgen in Meilenstein 4.
        Action::CaptureScreen
        | Action::CaptureWindow
        | Action::CaptureScroll
        | Action::CaptureText => log::info!("{action:?} ist noch nicht umgesetzt"),
    }
}
