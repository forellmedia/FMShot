//! Verdrahtung der Desktop-App: Fenster, Tray, Tastenkürzel, Commands.
//! Logik gehört nach `crates/` (siehe CLAUDE.md).

mod capture;
mod commands;
mod editor;
mod hotkeys;
mod i18n;
mod scroll;
mod tray;
mod updater;

use std::sync::Arc;

use fmshot_library::{CaptureRecord, Library};
use tauri::{Emitter, Manager, WindowEvent};
use tauri_specta::{Builder, collect_commands};

/// Alle Commands, die das Frontend aufrufen darf. Die TypeScript-Typen dazu
/// erzeugt `cargo test -p fmshot-desktop` nach `src/lib/bindings.ts`.
pub fn specta_builder() -> Builder<tauri::Wry> {
    Builder::<tauri::Wry>::new().commands(collect_commands![
        commands::app_info,
        editor::list_captures,
        editor::get_capture,
        editor::load_document,
        editor::save_document,
        editor::copy_document,
        editor::export_document,
        editor::add_asset,
        editor::add_asset_file,
        editor::add_asset_from_capture,
        editor::recognize_text,
        editor::copy_text,
        editor::delete_capture,
        editor::set_starred,
        editor::reveal_capture,
        editor::start_capture,
        updater::check_for_update,
        updater::install_update,
    ])
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    let builder = specta_builder();
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);

            // Kein Dock-Symbol – FMShot lebt im Tray.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let root = app.path().app_data_dir()?.join("Library");
            let library = Arc::new(Library::open(&root)?);
            log::info!("Bibliothek: {}", root.display());
            app.manage(capture::CaptureState::new(
                fmshot_capture::platform_capturer(),
                library,
            ));

            app.manage(editor::EditorState::new());
            app.manage(scroll::ScrollState::default());
            app.state::<capture::CaptureState>().capturer.warm_up();
            tray::create(app.handle())?;
            hotkeys::register_defaults(app.handle());
            Ok(())
        })
        .on_window_event(|window, event| {
            // Fenster schliessen = verstecken; die App läuft im Tray weiter.
            if let WindowEvent::CloseRequested { api, .. } = event {
                api.prevent_close();
                tray::hide_main_window(window.app_handle());
            }
        })
        .run(tauri::generate_context!())
        .expect("FMShot konnte nicht gestartet werden");
}

/// Nach jeder gespeicherten Aufnahme: Editor mit der Aufnahme zeigen.
pub(crate) fn after_capture(app: &tauri::AppHandle, rec: &CaptureRecord) {
    if let Err(e) = app.emit("capture-saved", rec.id.clone()) {
        log::error!("Ereignis capture-saved: {e}");
    }
    tray::show_main_window(app);
}

#[cfg(test)]
mod tests {
    use specta_typescript::Typescript;

    /// Schreibt die TypeScript-Bindings. Die CI prüft danach per `git diff`,
    /// ob die eingecheckte Datei aktuell ist.
    #[test]
    fn typescript_bindings_exportieren() {
        let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../src/lib/bindings.ts");
        super::specta_builder()
            .export(
                Typescript::default()
                    .header("// Automatisch erzeugt von tauri-specta – nicht von Hand ändern.\n"),
                path,
            )
            .expect("Bindings konnten nicht geschrieben werden");
    }
}
