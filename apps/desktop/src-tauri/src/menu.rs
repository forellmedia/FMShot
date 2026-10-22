//! App-Menü (macOS): ⌘Q schliesst nur den Editor. Nabshot beendet sich nur
//! über „Nabshot beenden“ im Menü.

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
    log::debug!("App-Menü angelegt");
    Ok(())
}

pub fn on_event(app: &AppHandle, id: &str) {
    match id {
        "close_editor" => {
            log::info!("App-Menü: Editor schliessen");
            crate::tray::hide_main_window(app)
        }
        "quit_app" => {
            log::info!("App-Menü: Beenden");
            crate::video::quit(app)
        }
        // Tray-Menü und Standard-Einträge (Kopieren, Einsetzen …) landen auch hier.
        other => log::debug!("Menü: {other}"),
    }
}
