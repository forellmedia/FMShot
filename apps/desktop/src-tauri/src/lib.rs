//! Verdrahtung der Desktop-App: Fenster, Tray, Tastenkürzel, Commands.
//! Logik gehört nach `crates/` (siehe CLAUDE.md).

mod capture;
mod commands;
mod editor;
mod hotkeys;
mod i18n;
mod logs;
mod menu;
mod paths;
mod scroll;
mod services;
mod settings;
mod tray;
mod updater;
mod video;
mod video_edit;
mod vision;

use std::sync::Arc;
use std::time::Instant;

use grabmark_library::Library;
use tauri::{Emitter, Manager, RunEvent, WindowEvent};
use tauri_specta::{Builder, collect_commands};

/// Alle Commands, die das Frontend aufrufen darf. Die TypeScript-Typen dazu
/// erzeugt `cargo test -p grabmark-desktop` nach `src/lib/bindings.ts`.
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
        video_edit::video_is_current,
        video_edit::cancel_video_export,
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
        logs::log_frontend,
        logs::open_log_folder,
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
        // Zweiter Start (z. B. `Grabmark capture region`): an die laufende App
        // weiterreichen statt ein zweites Tray-Symbol zu öffnen.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            handle_args(app, &argv);
        }))
        .plugin(logs::plugin())
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
            let setup_start = Instant::now();
            // Dauer eines Startschritts ins Log.
            let step = |name: &str, start: Instant| {
                log::debug!(
                    "Start: {name} {:.0} ms",
                    start.elapsed().as_secs_f64() * 1000.0
                );
            };
            builder.mount_events(app);

            // Kein Dock-Symbol – Grabmark lebt im Tray.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            let config = paths::get().config.join("settings.json");
            app.manage(settings::SettingsState::load(config));
            i18n::set_language(settings::get(app.handle()).language.as_deref());
            logs::log_startup(app.handle());

            log_displays(app.handle());

            let root = paths::get().data.join("Library");
            let t = Instant::now();
            let library = Arc::new(Library::open(&root).inspect_err(|e| {
                log::error!("Bibliothek {} öffnen: {e}", root.display());
            })?);
            // Vorschaubilder und Videos lädt das Frontend über `asset:`.
            app.asset_protocol_scope().allow_directory(&root, true)?;
            log::info!(
                "Bibliothek: {} ({:.0} ms)",
                root.display(),
                t.elapsed().as_secs_f64() * 1000.0
            );
            let t = Instant::now();
            app.manage(capture::CaptureState::new(
                grabmark_capture::platform_capturer(),
                library,
            ));
            step("Aufnahme und Texterkennung", t);

            app.manage(editor::EditorState::new());
            app.manage(scroll::ScrollState::default());
            app.manage(video::VideoState::default());
            app.manage(video_edit::VideoEditState::default());
            app.manage(tray::PopoverState::default());
            app.manage(services::Services::new());
            let t = Instant::now();
            app.manage(vision::VisionState::new(app.handle()));
            step("Bildinhalt vorbereiten", t);
            let t = Instant::now();
            app.state::<capture::CaptureState>().capturer.warm_up();
            step("Aufnahme aufwärmen", t);
            let t = Instant::now();
            tray::create(app.handle()).inspect_err(|e| log::error!("Tray-Symbol anlegen: {e}"))?;
            menu::create(app.handle()).inspect_err(|e| log::error!("App-Menü anlegen: {e}"))?;
            step("Tray und Menü", t);
            let t = Instant::now();
            hotkeys::register(app.handle());
            step("Tastenkürzel", t);
            services::start(app.handle());
            // Abgestürzte Aufnahme vom letzten Mal? (ADR 0012)
            video::offer_recovery(app.handle());
            let t = Instant::now();
            vision::start(app.handle());
            step("Bildinhalt starten", t);
            // Erster Start mit Argumenten (z. B. aus der Kommandozeile).
            let argv: Vec<String> = std::env::args().collect();
            handle_args(app.handle(), &argv);
            log::info!(
                "Start fertig in {:.0} ms",
                setup_start.elapsed().as_secs_f64() * 1000.0
            );
            Ok(())
        })
        .on_menu_event(|app, event| menu::on_event(app, event.id().as_ref()))
        .on_window_event(|window, event| match event {
            // Einstellungen schliessen sich wirklich (beim nächsten Öffnen frisch).
            WindowEvent::CloseRequested { .. } if window.label() == tray::SETTINGS => {
                log::info!("Einstellungen geschlossen");
                tray::update_activation(window.app_handle(), tray::SETTINGS);
            }
            // Sonst: schliessen = verstecken; die App läuft im Tray weiter.
            WindowEvent::CloseRequested { api, .. } => {
                log::debug!("Fenster {} schliessen → verstecken", window.label());
                api.prevent_close();
                if window.label() == tray::POPOVER {
                    tray::hide_popover(window.app_handle());
                } else {
                    tray::hide_main_window(window.app_handle());
                }
            }
            _ => {}
        })
        .build(tauri::generate_context!())
        .inspect_err(|e| log::error!("Start fehlgeschlagen: {e}"))
        .expect("Grabmark konnte nicht gestartet werden")
        .run(|_app, event| {
            if let RunEvent::Exit = event {
                log::info!("Grabmark beendet");
                log::logger().flush();
            }
        });
}

/// Monitore beim Start (Anzahl, Grösse, Massstab) – über Tauri, ohne
/// Bildschirm-Freigabe.
fn log_displays(app: &tauri::AppHandle) {
    match app.available_monitors() {
        Ok(monitors) => {
            log::info!("{} Monitor(e)", monitors.len());
            for m in monitors {
                let size = m.size();
                log::info!(
                    "Monitor: {}×{} px bei ({}, {}), Massstab {}",
                    size.width,
                    size.height,
                    m.position().x,
                    m.position().y,
                    m.scale_factor()
                );
            }
        }
        Err(e) => log::warn!("Monitore abfragen: {e}"),
    }
}

/// Befehle aus der Kommandozeile: `capture region|screen|window|scroll|text|video`
/// oder `gallery`.
fn handle_args(app: &tauri::AppHandle, argv: &[String]) {
    let args: Vec<&str> = argv.iter().skip(1).map(String::as_str).collect();
    if !args.is_empty() {
        log::info!("Kommandozeile: {}", args.join(" "));
    }
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
        [] => None,
        _ => {
            log::debug!("Kommandozeile: kein bekannter Befehl");
            None
        }
    };
    if let Some(mode) = mode {
        let app = app.clone();
        // Etwas warten, falls die App gerade erst startet.
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(150));
            capture::start(&app, mode);
        });
    }
    if let Err(e) = app.emit("cli", args.join(" ")) {
        log::debug!("Ereignis cli: {e}");
    }
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
