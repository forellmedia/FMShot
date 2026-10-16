//! Tray-Symbol mit Menü – FMShot läuft im Hintergrund.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::capture::{self, CaptureState};
use crate::i18n::t;

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let item = |id: &str, key: &str| MenuItem::with_id(app, id, t(key, &[]), true, None::<&str>);
    let region = item("capture_region", "tray.capture_region")?;
    let window = item("capture_window", "capture.window")?;
    let screen = item("capture_screen", "capture.screen")?;
    let scroll = item("capture_scroll", "capture.scroll")?;
    let scroll_auto = item("capture_scroll_auto", "capture.scroll_auto")?;
    let text = item("capture_text", "capture.text")?;
    let open = item("open_window", "tray.open_window")?;
    let library = item("open_library", "tray.open_library")?;
    let settings = item("settings", "tray.settings")?;
    let quit = item("quit", "tray.quit")?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[
            &region,
            &window,
            &screen,
            &scroll,
            &scroll_auto,
            &text,
            &separator,
            &open,
            &library,
            &settings,
            &separator,
            &quit,
        ],
    )?;

    // Schon vorhanden (z. B. Sprache geändert): nur das Menü ersetzen.
    if let Some(tray) = app.tray_by_id("main") {
        tray.set_menu(Some(menu))?;
        tray.set_tooltip(Some(t("tray.tooltip", &[])))?;
        return Ok(());
    }
    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip(t("tray.tooltip", &[]))
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "capture_region" => capture::start(app, capture::Mode::Region),
            "capture_window" => capture::start(app, capture::Mode::Window),
            "capture_screen" => capture::start(app, capture::Mode::Screen),
            "capture_text" => capture::start(app, capture::Mode::Text),
            "capture_scroll" => capture::start(app, capture::Mode::Scroll),
            "capture_scroll_auto" => capture::start(app, capture::Mode::ScrollAuto),
            "open_window" => show_main_window(app),
            "open_library" => {
                let root = app.state::<CaptureState>().library.root().to_path_buf();
                if let Err(e) = app.opener().open_path(root.to_string_lossy(), None::<&str>) {
                    log::error!("Bibliothek öffnen: {e}");
                }
            }
            "settings" => {
                use tauri::Emitter;
                show_main_window(app);
                let _ = app.emit("open-settings", ());
            }
            "quit" => app.exit(0),
            _ => {}
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

pub fn show_main_window(app: &AppHandle) {
    // Mit offenem Fenster ins Dock und in ⌘⇥, damit man es wiederfindet.
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Regular);
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

/// Fenster verstecken – FMShot läuft im Tray weiter.
pub fn hide_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.hide();
    }
    #[cfg(target_os = "macos")]
    let _ = app.set_activation_policy(tauri::ActivationPolicy::Accessory);
}
