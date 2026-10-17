//! Einstellungen: laden, speichern, anwenden – und die Commands dafür.

use std::path::PathBuf;
use std::sync::RwLock;

use fmshot_hotkeys::{Action, Platform, Shortcut};
use fmshot_settings::{Settings, action_key};
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Manager};
use tauri_plugin_dialog::DialogExt;

pub struct SettingsState {
    path: PathBuf,
    current: RwLock<Settings>,
}

impl SettingsState {
    pub fn load(path: PathBuf) -> Self {
        let current = Settings::load(&path).unwrap_or_else(|e| {
            log::error!("{e} – nehme Standardwerte");
            Settings::default()
        });
        Self {
            path,
            current: RwLock::new(current),
        }
    }

    pub fn get(&self) -> Settings {
        self.current
            .read()
            .unwrap_or_else(|e| e.into_inner())
            .clone()
    }
}

pub fn get(app: &AppHandle) -> Settings {
    app.state::<SettingsState>().get()
}

/// Ein Tastenkürzel für die Einstellungen.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct HotkeyInfo {
    /// Schlüssel wie in `Settings::hotkeys`.
    pub action: String,
    /// Gespeichertes Format (`Primary+Shift+2`), leer = keines.
    pub shortcut: String,
    /// Anzeige für Menschen (`⇧⌘2`).
    pub label: String,
    /// Kollidiert mit einem anderen Kürzel.
    pub conflict: bool,
}

#[tauri::command]
#[specta::specta]
pub fn get_settings(app: AppHandle) -> Settings {
    get(&app)
}

/// Speichert und wendet sofort an (Tastenkürzel, Sprache, Tray-Menü).
#[tauri::command]
#[specta::specta]
pub fn set_settings(app: AppHandle, settings: Settings) -> Result<(), String> {
    let state = app.state::<SettingsState>();
    settings.save(&state.path).map_err(|e| e.to_string())?;
    *state.current.write().unwrap_or_else(|e| e.into_inner()) = settings.clone();
    crate::i18n::set_language(settings.language.as_deref());
    crate::hotkeys::register(&app);
    if let Err(e) = crate::tray::create(&app) {
        log::error!("Tray-Menü: {e}");
    }
    if let Err(e) = crate::menu::create(&app) {
        log::error!("App-Menü: {e}");
    }
    Ok(())
}

/// Aktuelle Belegung inklusive Konflikten.
#[tauri::command]
#[specta::specta]
pub fn get_hotkeys(app: AppHandle) -> Vec<HotkeyInfo> {
    let settings = get(&app);
    let platform = Platform::current();
    let effective = settings.effective_hotkeys(platform);
    let conflicts = settings.hotkey_conflicts(platform);
    Action::ALL
        .iter()
        .map(|&action| {
            let sc: Option<&Shortcut> =
                effective.iter().find(|(a, _)| *a == action).map(|(_, s)| s);
            HotkeyInfo {
                action: action_key(action).into(),
                shortcut: sc.map(|s| s.to_string()).unwrap_or_default(),
                label: sc.map(|s| s.label(platform)).unwrap_or_default(),
                conflict: conflicts.iter().any(|(a, b)| *a == action || *b == action),
            }
        })
        .collect()
}

/// Wandelt ein Kürzel ins Anzeigeformat (für das Aufnahme-Feld).
#[tauri::command]
#[specta::specta]
pub fn shortcut_label(shortcut: String) -> Result<String, String> {
    let sc: Shortcut = shortcut
        .parse()
        .map_err(|e: fmshot_hotkeys::ParseError| e.to_string())?;
    Ok(sc.label(Platform::current()))
}

/// Während ein neues Kürzel aufgenommen wird, dürfen die alten nicht auslösen.
#[tauri::command]
#[specta::specta]
pub fn pause_hotkeys(app: AppHandle, paused: bool) {
    if paused {
        crate::hotkeys::unregister_all(&app);
    } else {
        crate::hotkeys::register(&app);
    }
}

/// Ordner für den Auto-Export wählen.
#[tauri::command]
#[specta::specta]
pub async fn choose_folder(app: AppHandle) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        app.dialog()
            .file()
            .blocking_pick_folder()
            .and_then(|p| p.into_path().ok())
            .map(|p| p.to_string_lossy().into_owned())
    })
    .await
    .ok()
    .flatten()
}
