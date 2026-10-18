//! Tray-Symbol – FMShot läuft im Hintergrund.
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
const POPOVER_SIZE: (f64, f64) = (340.0, 500.0);
/// Klick aufs Symbol bei offenem Fenster: Das Fenster verliert zuerst den
/// Fokus und schliesst sich – der Klick soll es nicht gleich wieder öffnen.
const REOPEN_GUARD: Duration = Duration::from_millis(350);

/// Wann das Tray-Fenster zuletzt durch Fokusverlust geschlossen wurde.
#[derive(Default)]
pub struct PopoverState {
    hidden_at: Mutex<Option<Instant>>,
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
            &video,
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
        .on_menu_event(|app, event| match event.id.as_ref() {
            "capture_region" => capture::start(app, capture::Mode::Region),
            "capture_window" => capture::start(app, capture::Mode::Window),
            "capture_screen" => capture::start(app, capture::Mode::Screen),
            "capture_text" => capture::start(app, capture::Mode::Text),
            "capture_scroll" => capture::start(app, capture::Mode::Scroll),
            "capture_scroll_auto" => capture::start(app, capture::Mode::ScrollAuto),
            "capture_video" => capture::start(app, capture::Mode::Video),
            "open_window" => show_main_window(app),
            "open_library" => open_library(app),
            "settings" => open_settings(app),
            "quit" => app.exit(0),
            _ => {}
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
    Ok(())
}

fn open_library(app: &AppHandle) {
    let root = app.state::<CaptureState>().library.root().to_path_buf();
    if let Err(e) = app.opener().open_path(root.to_string_lossy(), None::<&str>) {
        log::error!("Bibliothek öffnen: {e}");
    }
}

fn open_settings(app: &AppHandle) {
    show_main_window(app);
    let _ = app.emit("open-settings", ());
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
            return;
        }
        show_popover(&window, icon);
        return;
    }
    match build_popover(app) {
        Ok(window) => show_popover(&window, icon),
        Err(e) => log::error!("Tray-Fenster: {e}"),
    }
}

/// Wird beim ersten Klick angelegt und danach nur versteckt – das spart
/// Speicher, solange niemand es öffnet, und ist danach sofort da.
fn build_popover(app: &AppHandle) -> tauri::Result<WebviewWindow> {
    let window = WebviewWindowBuilder::new(app, POPOVER, WebviewUrl::App("tray".into()))
        .title("FMShot")
        .inner_size(POPOVER_SIZE.0, POPOVER_SIZE.1)
        .resizable(false)
        .decorations(false)
        .always_on_top(true)
        .skip_taskbar(true)
        .visible(false)
        .shadow(true)
        .build()?;
    let app = app.clone();
    window.on_window_event(move |event| {
        if let WindowEvent::Focused(false) = event {
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
    let _ = window.set_position(PhysicalPosition::new(x.round() as i32, y.round() as i32));
    let _ = window.show();
    let _ = window.set_focus();
    let _ = window.emit("tray-opened", ());
}

pub fn hide_popover(app: &AppHandle) {
    if let Some(window) = app.get_webview_window(POPOVER)
        && window.is_visible().unwrap_or(false)
    {
        let _ = window.hide();
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

// --- Commands für das Tray-Fenster ---------------------------------------------------

/// Was das Tray-Fenster im Hauptfenster öffnen kann.
#[derive(Debug, Clone, Copy, serde::Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum MainView {
    Gallery,
    Settings,
    Library,
}

#[tauri::command]
#[specta::specta]
pub fn open_main(app: AppHandle, view: MainView) {
    hide_popover(&app);
    match view {
        MainView::Gallery => {
            show_main_window(&app);
            let _ = app.emit("open-gallery", ());
        }
        MainView::Settings => open_settings(&app),
        MainView::Library => open_library(&app),
    }
}

#[tauri::command]
#[specta::specta]
pub fn quit_app(app: AppHandle) {
    app.exit(0);
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
        fmshot_media::microphones()
            .into_iter()
            .map(|d| Microphone {
                id: d.id,
                name: d.name,
            })
            .collect()
    })
    .await
    .unwrap_or_default()
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
        fmshot_media::cameras()
            .into_iter()
            .map(|d| Camera {
                id: d.id,
                name: d.name,
            })
            .collect()
    })
    .await
    .unwrap_or_default()
}

/// Startet FMShot mit der Anmeldung am Computer?
#[tauri::command]
#[specta::specta]
pub fn get_launch_at_login(app: AppHandle) -> bool {
    app.autolaunch().is_enabled().unwrap_or(false)
}

#[tauri::command]
#[specta::specta]
pub fn set_launch_at_login(app: AppHandle, enabled: bool) -> Result<(), String> {
    let autolaunch = app.autolaunch();
    if enabled {
        autolaunch.enable()
    } else {
        autolaunch.disable()
    }
    .map_err(|e| e.to_string())
}
