//! Verdrahtung der Desktop-App: Fenster, Tray, Tastenkürzel, Commands.
//! Logik gehört nach `crates/` (siehe CLAUDE.md).

mod capture;
mod commands;
mod editor;
mod hotkeys;
mod i18n;
mod menu;
mod scroll;
mod services;
mod settings;
mod tray;
mod updater;
mod video;
mod video_edit;
mod vision;

use std::sync::Arc;

use fmshot_library::Library;
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
        editor::clipboard_change_count,
        editor::delete_capture,
        editor::set_starred,
        editor::reveal_capture,
        editor::start_capture,
        editor::copy_video,
        video_edit::video_tracks,
        video_edit::export_video,
        video_edit::export_gif,
        video_edit::video_snapshot,
        editor::save_video_as,
        tray::open_main,
        tray::quit_app,
        tray::list_microphones,
        tray::list_cameras,
        tray::get_launch_at_login,
        tray::set_launch_at_login,
        settings::get_settings,
        settings::set_settings,
        settings::get_hotkeys,
        settings::shortcut_label,
        settings::pause_hotkeys,
        settings::choose_folder,
        settings::system_shortcuts_active,
        settings::open_shortcut_settings,
        services::get_diagnostics,
        services::search_captures,
        services::query_captures,
        services::report_bug,
        updater::check_for_update,
        updater::install_update,
        vision::content_search_status,
    ])
}

/// Paniken ins Log schreiben. Die Meldung landet sonst nur auf stderr – und
/// das sieht beim Start aus dem Finder niemand.
fn log_panics() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        log::error!(
            "Absturz: {info}\n{}",
            std::backtrace::Backtrace::force_capture()
        );
        log::logger().flush();
        default_hook(info);
    }));
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    log_panics();
    let builder = specta_builder();
    tauri::Builder::default()
        // Zweiter Start (z. B. `FMShot capture region`): an die laufende App
        // weiterreichen statt ein zweites Tray-Symbol zu öffnen.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            handle_args(app, &argv);
        }))
        .plugin(
            tauri_plugin_log::Builder::new()
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_global_shortcut::Builder::new().build())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_updater::Builder::new().build())
        // Mit der Anmeldung starten (Einstellungen). macOS: LaunchAgent.
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(builder.invoke_handler())
        .setup(move |app| {
            builder.mount_events(app);

            // Kein Dock-Symbol – FMShot lebt im Tray.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let config = app.path().app_config_dir()?.join("settings.json");
            app.manage(settings::SettingsState::load(config));
            i18n::set_language(settings::get(app.handle()).language.as_deref());

            let root = app.path().app_data_dir()?.join("Library");
            let library = Arc::new(Library::open(&root)?);
            log::info!("Bibliothek: {}", root.display());
            app.manage(capture::CaptureState::new(
                fmshot_capture::platform_capturer(),
                library,
            ));

            app.manage(editor::EditorState::new());
            app.manage(scroll::ScrollState::default());
            app.manage(video::VideoState::default());
            app.manage(video_edit::VideoEditState::default());
            app.manage(tray::PopoverState::default());
            app.manage(services::Services::new());
            app.manage(vision::VisionState::new(app.handle()));
            app.state::<capture::CaptureState>().capturer.warm_up();
            tray::create(app.handle())?;
            menu::create(app.handle())?;
            hotkeys::register(app.handle());
            services::start(app.handle());
            vision::start(app.handle());
            // Erster Start mit Argumenten (z. B. aus der Kommandozeile).
            let argv: Vec<String> = std::env::args().collect();
            handle_args(app.handle(), &argv);
            Ok(())
        })
        .on_menu_event(|app, event| menu::on_event(app, event.id().as_ref()))
        .on_window_event(|window, event| match event {
            // Einstellungen schliessen sich wirklich (beim nächsten Öffnen frisch).
            WindowEvent::CloseRequested { .. } if window.label() == tray::SETTINGS => {
                tray::update_activation(window.app_handle(), tray::SETTINGS);
            }
            // Sonst: schliessen = verstecken; die App läuft im Tray weiter.
            WindowEvent::CloseRequested { api, .. } => {
                api.prevent_close();
                if window.label() == tray::POPOVER {
                    tray::hide_popover(window.app_handle());
                } else {
                    tray::hide_main_window(window.app_handle());
                }
            }
            _ => {}
        })
        .run(tauri::generate_context!())
        .expect("FMShot konnte nicht gestartet werden");
}

/// Befehle aus der Kommandozeile: `capture region|screen|window|scroll|text|video`
/// oder `gallery`.
fn handle_args(app: &tauri::AppHandle, argv: &[String]) {
    let args: Vec<&str> = argv.iter().skip(1).map(String::as_str).collect();
    let mode = match args.as_slice() {
        ["capture", "region", ..] => Some(capture::Mode::Region),
        ["capture", "screen", ..] => Some(capture::Mode::Screen),
        ["capture", "window", ..] => Some(capture::Mode::Window),
        ["capture", "scroll", ..] => Some(capture::Mode::Scroll),
        ["capture", "text", ..] => Some(capture::Mode::Text),
        ["capture", "video", ..] => Some(capture::Mode::Video),
        ["capture", "webcam", ..] => Some(capture::Mode::Webcam),
        ["gallery", ..] => {
            tray::show_main_window(app);
            None
        }
        ["settings", ..] => {
            tray::open_settings(app);
            None
        }
        _ => None,
    };
    if let Some(mode) = mode {
        let app = app.clone();
        // Etwas warten, falls die App gerade erst startet.
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(150));
            capture::start(&app, mode);
        });
    }
    let _ = app.emit("cli", args.join(" "));
}

#[cfg(test)]
mod tests {
    use specta_typescript::Typescript;

    /// Release muss abwickeln können: wry fängt Objective-C-Exceptions von
    /// WebKit mit `objc2::exception::catch` – mit `panic = "abort"` beendet
    /// jede davon die App. Tests selbst laufen immer mit „unwind“, darum wird
    /// die Einstellung geprüft.
    #[test]
    fn release_bricht_bei_panik_nicht_ab() {
        let manifest = include_str!("../../../../Cargo.toml");
        let release = manifest
            .split("[profile.release]")
            .nth(1)
            .and_then(|s| s.split("\n[").next())
            .expect("[profile.release] fehlt");
        let aborts = release
            .lines()
            .map(|l| l.split('#').next().unwrap_or("").replace(' ', ""))
            .any(|l| l == "panic=\"abort\"");
        assert!(!aborts, "panic = \"abort\" in [profile.release]");
    }

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
