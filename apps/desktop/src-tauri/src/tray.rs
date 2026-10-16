//! Tray-Symbol mit Menü – FMShot läuft im Hintergrund.

use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::TrayIconBuilder;
use tauri::{AppHandle, Manager};
use tauri_plugin_opener::OpenerExt;

use crate::capture::{self, CaptureState};
use crate::i18n::t;

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let region = MenuItem::with_id(
        app,
        "capture_region",
        t("tray.capture_region", &[]),
        true,
        None::<&str>,
    )?;
    let open = MenuItem::with_id(
        app,
        "open_window",
        t("tray.open_window", &[]),
        true,
        None::<&str>,
    )?;
    let library = MenuItem::with_id(
        app,
        "open_library",
        t("tray.open_library", &[]),
        true,
        None::<&str>,
    )?;
    let quit = MenuItem::with_id(app, "quit", t("tray.quit", &[]), true, None::<&str>)?;
    let separator = PredefinedMenuItem::separator(app)?;
    let menu = Menu::with_items(
        app,
        &[&region, &separator, &open, &library, &separator, &quit],
    )?;

    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip(t("tray.tooltip", &[]))
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "capture_region" => capture::start_region(app),
            "open_window" => show_main_window(app),
            "open_library" => {
                let root = app.state::<CaptureState>().library.root().to_path_buf();
                if let Err(e) = app.opener().open_path(root.to_string_lossy(), None::<&str>) {
                    log::error!("Bibliothek öffnen: {e}");
                }
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
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.set_focus();
    }
}
