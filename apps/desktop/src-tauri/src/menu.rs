//! App-Menü (macOS): ⌘Q schliesst nur den Editor. Grabmark beendet sich nur
//! über „Grabmark beenden“ im Menü.

use tauri::AppHandle;
use tauri::menu::{Menu, MenuItem};

use crate::i18n::t;

/// Ersetzt im Standard-Menü das vordefinierte „Beenden“ (⌘Q). Das schickt
/// macOS direkt an `terminate:` – Tauri bekommt davon nichts mit und kann es
/// nicht verhindern.
pub fn create(app: &AppHandle) -> tauri::Result<()> {
    if !cfg!(target_os = "macos") {
        return Ok(());
    }
    let menu = Menu::default(app)?;
    if let Some(app_menu) = menu.items()?.first().and_then(|m| m.as_submenu()) {
        // Letzter Eintrag des App-Menüs ist das vordefinierte „Beenden“.
        if let Some(quit) = app_menu.items()?.last() {
            app_menu.remove(quit)?;
        }
        let item = |id: &str, key: &str, accel: Option<&str>| {
            MenuItem::with_id(app, id, t(key, &[]), true, accel)
        };
        app_menu.append(&item("close_editor", "menu.close_editor", Some("Cmd+Q"))?)?;
        app_menu.append(&item("quit_app", "tray.quit", None)?)?;
    }
    app.set_menu(menu)?;
    Ok(())
}

pub fn on_event(app: &AppHandle, id: &str) {
    match id {
        "close_editor" => crate::tray::hide_main_window(app),
        "quit_app" => crate::video::quit(app),
        _ => {}
    }
}
