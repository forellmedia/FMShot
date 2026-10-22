//! Tray-Symbol – Nabshot läuft im Hintergrund.
//!
//! Linksklick öffnet ein kleines Fenster mit den Tabs Bild, Video und
//! Einstellungen (Svelte-Seite `/tray`). Rechtsklick zeigt das klassische
//! Menü. Unter Linux meldet das Tray keine Klicks; dort öffnet jeder Klick
//! das Menü, das alles Wichtige ebenfalls enthält.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use specta::Type;
use tauri::menu::{Menu, MenuItem, PredefinedMenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
#[cfg(target_os = "macos")]
use tauri::window::{Effect, EffectState, EffectsBuilder};
use tauri::{
    AppHandle, Emitter, Manager, PhysicalPosition, Rect, WebviewUrl, WebviewWindow,
    WebviewWindowBuilder, WindowEvent,
};
use tauri_plugin_autostart::ManagerExt;
use tauri_plugin_opener::OpenerExt;

use crate::capture::{self, CaptureState};
use crate::i18n::t;

/// Fenster-Kennung des Tray-Fensters.
pub const POPOVER: &str = "tray";
/// Fenster-Kennung der Einstellungen (eigenes Fenster, Svelte-Seite `/settings`).
pub const SETTINGS: &str = "settings";
const SETTINGS_SIZE: (f64, f64) = (620.0, 660.0);
const POPOVER_SIZE: (f64, f64) = (340.0, 500.0);
/// Klick aufs Symbol bei offenem Fenster: Das Fenster verliert zuerst den
/// Fokus und schliesst sich – der Klick soll es nicht gleich wieder öffnen.
const REOPEN_GUARD: Duration = Duration::from_millis(350);

/// Wann das Tray-Fenster zuletzt durch Fokusverlust geschlossen wurde.
#[derive(Default)]
pub struct PopoverState {
    hidden_at: Mutex<Option<Instant>>,
}

/// Fehler bei Fensteraufrufen, die nur selten etwas bedeuten – fürs Debug-Log.
fn check<T, E: std::fmt::Display>(what: &str, result: Result<T, E>) {
    if let Err(e) = result {
        log::debug!("{what}: {e}");
    }
}

pub fn create(app: &AppHandle) -> tauri::Result<()> {
    let item = |id: &str, key: &str| MenuItem::with_id(app, id, t(key, &[]), true, None::<&str>);
    let region = item("capture_region", "tray.capture_region")?;
    let window = item("capture_window", "capture.window")?;
    let screen = item("capture_screen", "capture.screen")?;
    let scroll = item("capture_scroll", "capture.scroll")?;
    let scroll_auto = item("capture_scroll_auto", "capture.scroll_auto")?;
    let text = item("capture_text", "capture.text")?;
    let video = item("capture_video", "tray.capture_video")?;
    let webcam = item("capture_webcam", "tray.capture_webcam")?;
    let open = item("open_window", "tray.open_editor")?;
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
            &video,
            &webcam,
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
        log::debug!("Tray-Menü neu aufgebaut");
        tray.set_menu(Some(menu))?;
        tray.set_tooltip(Some(t("tray.tooltip", &[])))?;
        return Ok(());
    }
    let mut builder = TrayIconBuilder::with_id("main")
        .tooltip(t("tray.tooltip", &[]))
        .menu(&menu)
        // Linksklick = Tray-Fenster; unter Linux gibt es keine Klick-Ereignisse.
        .show_menu_on_left_click(cfg!(target_os = "linux"))
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                toggle_popover(tray.app_handle(), rect);
            }
        })
        .on_menu_event(|app, event| {
            log::info!("Tray-Menü: {}", event.id.as_ref());
            on_menu_event(app, event.id.as_ref())
        });
    // Schwarz-auf-durchsichtig als Vorlage: macOS färbt es passend zur
    // Menüleiste (hell/dunkel). Unter Windows/Linux das App-Symbol.
    if cfg!(target_os = "macos") {
        let icon = tauri::image::Image::from_bytes(include_bytes!("../icons/tray.png"))?;
        builder = builder.icon(icon).icon_as_template(true);
    } else if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    log::debug!("Tray-Symbol angelegt");
    Ok(())
}

fn on_menu_event(app: &AppHandle, id: &str) {
    match id {
        "capture_region" => capture::start(app, capture::Mode::Region),
        "capture_window" => capture::start(app, capture::Mode::Window),
        "capture_screen" => capture::start(app, capture::Mode::Screen),
        "capture_text" => capture::start(app, capture::Mode::Text),
        "capture_scroll" => capture::start(app, capture::Mode::Scroll),
        "capture_scroll_auto" => capture::start(app, capture::Mode::ScrollAuto),
        "capture_video" => capture::start(app, capture::Mode::Video),
        "capture_webcam" => capture::start(app, capture::Mode::Webcam),
        "open_window" => open_editor(app),
        "open_library" => open_library(app),
        "settings" => open_settings(app),
        "quit" => crate::video::quit(app),
        other => log::warn!("Tray-Menü: unbekannter Eintrag {other}"),
    }
}

fn open_library(app: &AppHandle) {
    let root = app.state::<CaptureState>().library.root().to_path_buf();
    log::info!("Bibliotheksordner öffnen");
    if let Err(e) = app.opener().open_path(root.to_string_lossy(), None::<&str>) {
        log::error!("Bibliothek öffnen: {e}");
    }
}

/// Einstellungen als eigenes Fenster, unabhängig vom Hauptfenster. Es wird
/// bei jedem Öffnen neu angelegt und beim Schliessen entfernt – so zeigt es
/// immer den aktuellen Stand.
pub fn open_settings(app: &AppHandle) {
    // Mit offenem Fenster ins Dock und in ⌘⇥, damit man es wiederfindet.
    #[cfg(target_os = "macos")]
    check(
        "Dock-Symbol zeigen",
        app.set_activation_policy(tauri::ActivationPolicy::Regular),
    );
    if let Some(window) = app.get_webview_window(SETTINGS) {
        log::info!("Einstellungen: Fenster nach vorne");
        check("Einstellungen", window.unminimize());
        check("Einstellungen", window.show());
        check("Einstellungen", window.set_focus());
        return;
    }
    let start = Instant::now();
    let builder = WebviewWindowBuilder::new(app, SETTINGS, WebviewUrl::App("settings".into()))
        .title(t("settings.title", &[]))
        .inner_size(SETTINGS_SIZE.0, SETTINGS_SIZE.1)
        .min_inner_size(520.0, 420.0)
        .center();
    // Glas: der Schreibtisch scheint durch, Titelleiste über dem Inhalt.
    #[cfg(target_os = "macos")]
    let builder = builder
        .transparent(true)
        .title_bar_style(tauri::TitleBarStyle::Overlay)
        .hidden_title(true)
        .effects(glass(Effect::Sidebar, None));
    let built = builder.build();
    match built {
        Ok(window) => {
            log::info!(
                "Einstellungen: Fenster geöffnet ({:.0} ms)",
                start.elapsed().as_secs_f64() * 1000.0
            );
            check("Einstellungen", window.set_focus());
        }
        Err(e) => log::error!("Einstellungen: {e}"),
    }
}

/// Ohne sichtbares Fenster (Haupt- oder Einstellungsfenster) verschwindet
/// Nabshot aus Dock und ⌘⇥. `closing` = Fenster, das gerade zugeht.
pub fn update_activation(app: &AppHandle, closing: &str) {
    #[cfg(target_os = "macos")]
    {
        let open = ["main", SETTINGS].iter().any(|label| {
            *label != closing
                && app
                    .get_webview_window(label)
                    .is_some_and(|w| w.is_visible().unwrap_or(false))
        });
        let policy = if open {
            tauri::ActivationPolicy::Regular
        } else {
            tauri::ActivationPolicy::Accessory
        };
        log::debug!("Fenster {closing} geht zu – Dock-Symbol: {open}");
        check("Dock-Symbol", app.set_activation_policy(policy));
    }
    #[cfg(not(target_os = "macos"))]
    let _ = (app, closing);
}

// --- Tray-Fenster ------------------------------------------------------------------

fn toggle_popover(app: &AppHandle, icon: Rect) {
    let recently_hidden = app
        .state::<PopoverState>()
        .hidden_at
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .is_some_and(|t| t.elapsed() < REOPEN_GUARD);
    if let Some(window) = app.get_webview_window(POPOVER) {
        if window.is_visible().unwrap_or(false) {
            hide_popover(app);
            return;
        }
        if recently_hidden {
            log::debug!("Tray-Fenster: Klick kurz nach dem Schliessen – bleibt zu");
            return;
        }
        show_popover(&window, icon);
        return;
    }
    let start = Instant::now();
    match build_popover(app) {
        Ok(window) => {
            log::debug!(
                "Tray-Fenster angelegt ({:.0} ms)",
                start.elapsed().as_secs_f64() * 1000.0
            );
            show_popover(&window, icon)
        }
        Err(e) => log::error!("Tray-Fenster: {e}"),
    }
}

/// Ecken des Tray-Fensters (wie Menüs unter macOS 26).
#[cfg(target_os = "macos")]
const POPOVER_RADIUS: f64 = 16.0;

/// Glas-Hintergrund (macOS-Vibrancy) für ein durchsichtiges Fenster.
#[cfg(target_os = "macos")]
fn glass(effect: Effect, radius: Option<f64>) -> tauri::utils::config::WindowEffectsConfig {
    let builder = EffectsBuilder::new()
        .effect(effect)
        .state(EffectState::FollowsWindowActiveState);
    match radius {
        Some(r) => builder.radius(r).build(),
        None => builder.build(),
    }
}

/// Wird beim ersten Klick angelegt und danach nur versteckt – das spart
/// Speicher, solange niemand es öffnet, und ist danach sofort da.
fn build_popover(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let builder = WebviewWindowBuilder::new(app, POPOVER, WebviewUrl::App("tray".into()))
        .title("Nabshot")
        .inner_size(POPOVER_SIZE.0, POPOVER_SIZE.1)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .shadow(true);
    #[cfg(target_os = "macos")]
    let builder = builder
        .transparent(true)
        .effects(glass(Effect::Popover, Some(POPOVER_RADIUS)));
    let window = builder.build()?;
    let app = app.clone();
    window.on_window_event(move |event| {
        // Das Overlay einer Aufnahme per Tastenkürzel nimmt den Fokus – das
        // Tray-Fenster bleibt dann offen und kommt mit aufs Bild.
        if let WindowEvent::Focused(false) = event
            && !crate::capture::shows_own_windows(&app)
        {
            hide_popover(&app);
        }
    });
    Ok(window)
}

/// Unter dem Symbol (macOS, Menüleiste oben) bzw. darüber (Windows,
/// Taskleiste unten), immer ganz auf dem Monitor.
fn show_popover(window: &WebviewWindow, icon: Rect) {
    let scale = window.scale_factor().unwrap_or(1.0);
    let pos = icon.position.to_physical::<f64>(scale);
    let size = icon.size.to_physical::<f64>(scale);
    let (w, h) = (POPOVER_SIZE.0 * scale, POPOVER_SIZE.1 * scale);
    let gap = 6.0 * scale;
    let monitor = window
        .available_monitors()
        .ok()
        .and_then(|ms| {
            ms.into_iter().find(|m| {
                let p = m.position();
                let s = m.size();
                pos.x >= p.x as f64
                    && pos.x < p.x as f64 + s.width as f64
                    && pos.y >= p.y as f64
                    && pos.y < p.y as f64 + s.height as f64
            })
        })
        .or_else(|| window.primary_monitor().ok().flatten());
    let mut x = pos.x + size.width / 2.0 - w / 2.0;
    let mut y = pos.y + size.height + gap;
    if let Some(m) = monitor {
        let area = m.work_area();
        let (ax, ay) = (area.position.x as f64, area.position.y as f64);
        let (aw, ah) = (area.size.width as f64, area.size.height as f64);
        // Symbol in der unteren Hälfte (Taskleiste unten): darüber öffnen.
        if pos.y > ay + ah / 2.0 {
            y = pos.y - h - gap;
        }
        x = x.clamp(ax + gap, ax + aw - w - gap);
        y = y.clamp(ay + gap, ay + ah - h - gap);
    }
    log::info!("Tray-Fenster zeigen");
    check(
        "Tray-Fenster",
        window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32)),
    );
    check("Tray-Fenster", window.show());
    check("Tray-Fenster", window.set_focus());
    check("Tray-Fenster", window.emit("tray-opened", ()));
}

/// Tray-Fenster nach einer Aufnahme wieder in den Vordergrund, falls offen.
pub fn refocus_popover(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(POPOVER)
        && window.is_visible().unwrap_or(false)
    {
        log::debug!("Tray-Fenster: Fokus zurück");
        check("Tray-Fenster", window.set_focus());
    }
}

pub fn hide_popover(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(POPOVER)
        && window.is_visible().unwrap_or(false)
    {
        log::info!("Tray-Fenster verstecken");
        check("Tray-Fenster", window.hide());
        *app.state::<PopoverState>()
            .hidden_at
            .lock()
            .unwrap_or_else(|e| e.into_inner()) = Some(Instant::now());
    }
}

// --- Hauptfenster ------------------------------------------------------------------

pub fn show_main_window(app: &AppHandle) {
    // Mit offenem Fenster ins Dock und in ⌘⇥, damit man es wiederfindet.
    #[cfg(target_os = "macos")]
    check(
        "Dock-Symbol zeigen",
        app.set_activation_policy(tauri::ActivationPolicy::Regular),
    );
    if let Some(window) = app.get_webview_window("main") {
        log::info!("Hauptfenster zeigen");
        check("Hauptfenster", window.show());
        check("Hauptfenster", window.unminimize());
        check("Hauptfenster", window.set_focus());
    } else {
        log::warn!("Hauptfenster fehlt");
    }
}

/// Hauptfenster mit dem Editor zeigen (eine offene Galerie schliesst sich).
fn open_editor(app: &AppHandle) {
    show_main_window(app);
    check("Ereignis open-editor", app.emit("open-editor", ()));
}

/// Fenster verstecken – Nabshot läuft im Tray weiter.
pub fn hide_main_window(app: &AppHandle) {
    if let Some(window) = app.get_webview_window("main") {
        log::info!("Hauptfenster verstecken");
        check("Hauptfenster", window.hide());
    }
    update_activation(app, "main");
}

// --- Commands für das Tray-Fenster ---------------------------------------------------

/// Was das Tray-Fenster im Hauptfenster öffnen kann.
#[derive(Debug, Clone, Copy, serde::Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum MainView {
    Editor,
    Gallery,
    Settings,
    Library,
    /// Hauptfenster mit „Speicher aufräumen“.
    Cleanup,
}

#[tauri::command]
#[specta::specta]
// async: Fenster aus einem synchronen Befehl anzulegen blockiert unter Windows.
pub async fn open_main(app: AppHandle, view: MainView) {
    log::info!("Tray-Fenster: {view:?} öffnen");
    hide_popover(&app);
    match view {
        MainView::Editor => open_editor(&app),
        MainView::Gallery => {
            show_main_window(&app);
            check("Ereignis open-gallery", app.emit("open-gallery", ()));
        }
        MainView::Settings => open_settings(&app),
        MainView::Library => open_library(&app),
        MainView::Cleanup => crate::storage::open_cleanup(&app),
    }
}

#[tauri::command]
#[specta::specta]
pub fn quit_app(app: AppHandle) {
    log::info!("Beenden (Tray-Fenster)");
    // Läuft eine Aufnahme, wird sie erst gesichert.
    crate::video::quit(&app);
}

/// Ein Mikrofon für die Auswahl im Video-Tab.
#[derive(Debug, Clone, Serialize, Type)]
pub struct Microphone {
    pub id: String,
    pub name: String,
}

#[tauri::command]
#[specta::specta]
pub async fn list_microphones() -> Vec<Microphone> {
    tauri::async_runtime::spawn_blocking(|| {
        let start = Instant::now();
        let list: Vec<Microphone> = nabshot_media::microphones()
            .into_iter()
            .map(|d| Microphone {
                id: d.id,
                name: d.name,
            })
            .collect();
        log::debug!(
            "Mikrofone: {} ({:.0} ms)",
            list.len(),
            start.elapsed().as_secs_f64() * 1000.0
        );
        list
    })
    .await
    .unwrap_or_else(|e| {
        log::warn!("Mikrofone auflisten: {e}");
        Vec::new()
    })
}

/// Eine Kamera für die Auswahl im Video-Tab.
#[derive(Debug, Clone, Serialize, Type)]
pub struct Camera {
    pub id: String,
    pub name: String,
}

#[tauri::command]
#[specta::specta]
pub async fn list_cameras() -> Vec<Camera> {
    tauri::async_runtime::spawn_blocking(|| {
        let start = Instant::now();
        let list: Vec<Camera> = nabshot_media::cameras()
            .into_iter()
            .map(|d| Camera {
                id: d.id,
                name: d.name,
            })
            .collect();
        log::debug!(
            "Kameras: {} ({:.0} ms)",
            list.len(),
            start.elapsed().as_secs_f64() * 1000.0
        );
        list
    })
    .await
    .unwrap_or_else(|e| {
        log::warn!("Kameras auflisten: {e}");
        Vec::new()
    })
}

/// Startet Nabshot mit der Anmeldung am Computer?
#[tauri::command]
#[specta::specta]
pub fn get_launch_at_login(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or_else(|e| {
        log::warn!("Mit Anmeldung starten abfragen: {e}");
        false
    })
}

/// Früherer Name (ADR 0015): Das alte Startobjekt `Grabmark.plist` zeigt auf
/// eine App, die es nicht mehr gibt. Weg damit – und neu anlegen, wenn
/// „Mit Anmeldung starten“ an war.
#[cfg(target_os = "macos")]
pub fn migrate_launch_agent(app: &AppHandle) {
    let Some(old) = dirs::home_dir().map(|h| h.join("Library/LaunchAgents/Grabmark.plist")) else {
        return;
    };
    if !old.exists() {
        return;
    }
    match std::fs::remove_file(&old) {
        Ok(()) => log::info!("Altes Startobjekt entfernt: {}", old.display()),
        Err(e) => {
            log::warn!("Altes Startobjekt {}: {e}", old.display());
            return;
        }
    }
    match app.autolaunch().enable() {
        Ok(()) => log::info!("Mit Anmeldung starten: unter neuem Namen eingerichtet"),
        Err(e) => log::error!("Mit Anmeldung starten neu einrichten: {e}"),
    }
}

#[tauri::command]
#[specta::specta]
pub fn set_launch_at_login(app: AppHandle, enabled: bool) -> Result<(), String> {
    log::info!("Mit Anmeldung starten: {enabled}");
    let autolaunch = app.autolaunch();
    if enabled {
        autolaunch.enable()
    } else {
        autolaunch.disable()
    }
    .inspect_err(|e| log::error!("Mit Anmeldung starten: {e}"))
    .map_err(|e| e.to_string())
}
