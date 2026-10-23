//! App-Menü (macOS): ⌘Q schliesst nur den Editor, ⌘H versteckt die Fenster
//! und lässt das Dock-Symbol stehen. Nabshot beendet sich nur über „Nabshot
//! beenden“ im Menü.

use tauri::AppHandle;
use tauri::menu::{Menu, MenuItem};

use crate::i18n::t;

/// Ersetzt im Standard-Menü das vordefinierte „Beenden“ (⌘Q). Das schickt
/// macOS direkt an `terminate:` – Tauri bekommt davon nichts mit und kann es
/// nicht verhindern. Ebenso „Ausblenden“ (⌘H): Blendet macOS die App aus,
/// holt das Auswahl-Overlay beim nächsten Screenshot alle Fenster zurück,
/// und der Editor blitzt kurz auf.
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
        // Standard-Menü von Tauri: Über, –, Dienste, –, Ausblenden, Andere
        // ausblenden, –, Beenden.
        const HIDE_INDEX: usize = 4;
        if app_menu.items()?.len() > HIDE_INDEX {
            app_menu.remove_at(HIDE_INDEX)?;
            app_menu.insert(&item("hide_app", "menu.hide", Some("Cmd+H"))?, HIDE_INDEX)?;
        } else {
            log::warn!("App-Menü: „Ausblenden“ nicht gefunden");
        }
        app_menu.append(&item("close_editor", "menu.close_editor", Some("Cmd+Q"))?)?;
        app_menu.append(&item("quit_app", "tray.quit", None)?)?;
    }
    app.set_menu(menu)?;
    log::debug!("App-Menü angelegt");
    Ok(())
}

/// Einträge des App-Menüs – das Tray-Menü bekommt sie ebenfalls gemeldet.
pub fn is_app_menu_item(id: &str) -> bool {
    matches!(id, "close_editor" | "hide_app" | "quit_app")
}

pub fn on_event(app: &AppHandle, id: &str) {
    match id {
        "close_editor" => {
            log::info!("App-Menü: Editor schliessen");
            crate::tray::hide_main_window(app)
        }
        "hide_app" => {
            log::info!("App-Menü: Ausblenden");
            crate::tray::hide_windows_keep_dock(app)
        }
        "quit_app" => {
            log::info!("App-Menü: Beenden");
            crate::video::quit(app)
        }
        // Tray-Menü und Standard-Einträge (Kopieren, Einsetzen …) landen auch hier.
        other => log::debug!("Menü: {other}"),
    }
}
