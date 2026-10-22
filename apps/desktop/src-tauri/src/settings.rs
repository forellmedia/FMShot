//! Einstellungen: laden, speichern, anwenden – und die Commands dafür.

use std::path::PathBuf;
use std::sync::RwLock;

use nabshot_hotkeys::{Action, Platform, Shortcut};
use nabshot_settings::{Settings, action_key};
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Emitter, Manager};
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
        // Das Log-Plugin läuft hier schon; die Abweichungen vom Standard
        // zeigen, womit Nabshot startet.
        let custom = changed_fields(&Settings::default(), &current);
        log::debug!(
            "Einstellungen geladen: {} – abweichend vom Standard: {}",
            path.display(),
            if custom.is_empty() {
                "nichts".to_string()
            } else {
                custom.join(", ")
            }
        );
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
    let start = std::time::Instant::now();
    let state = app.state::<SettingsState>();
    settings.save(&state.path).map_err(|e| {
        log::error!("Einstellungen speichern: {e}");
        e.to_string()
    })?;
    let previous = std::mem::replace(
        &mut *state.current.write().unwrap_or_else(|e| e.into_inner()),
        settings.clone(),
    );
    let changed = changed_fields(&previous, &settings);
    if changed.is_empty() {
        log::debug!("Einstellungen gespeichert, unverändert");
    } else {
        log::info!("Einstellungen geändert: {}", changed.join(", "));
    }
    crate::i18n::set_language(settings.language.as_deref());
    // Alle Fenster laden in der neuen Sprache neu.
    if previous.language != settings.language {
        let _ = app.emit("language-changed", ());
    }
    crate::hotkeys::register(&app);
    if let Err(e) = crate::tray::create(&app) {
        log::error!("Tray-Menü: {e}");
    }
    if let Err(e) = crate::menu::create(&app) {
        log::error!("App-Menü: {e}");
    }
    crate::vision::wake(&app);
    log::debug!(
        "Einstellungen angewendet ({:.0} ms)",
        start.elapsed().as_secs_f64() * 1000.0
    );
    Ok(())
}

/// Geänderte Felder, z. B. `video.fps: 30 → 60`. Ordner und Geräte nur mit
/// Namen, ohne Wert.
fn changed_fields(old: &Settings, new: &Settings) -> Vec<String> {
    let (Ok(a), Ok(b)) = (serde_json::to_value(old), serde_json::to_value(new)) else {
        log::warn!("Einstellungen vergleichen: nicht serialisierbar");
        return Vec::new();
    };
    let mut out = Vec::new();
    diff("", &a, &b, &mut out);
    out
}

fn diff(path: &str, a: &serde_json::Value, b: &serde_json::Value, out: &mut Vec<String>) {
    use serde_json::Value;
    if a == b {
        return;
    }
    if let (Value::Object(x), Value::Object(y)) = (a, b) {
        let keys: std::collections::BTreeSet<&String> = x.keys().chain(y.keys()).collect();
        for key in keys {
            let sub = if path.is_empty() {
                key.clone()
            } else {
                format!("{path}.{key}")
            };
            diff(
                &sub,
                x.get(key).unwrap_or(&Value::Null),
                y.get(key).unwrap_or(&Value::Null),
                out,
            );
        }
        return;
    }
    const PRIVATE: [&str; 3] = ["folder", "microphone_id", "camera_id"];
    if PRIVATE.iter().any(|k| path.rsplit('.').next() == Some(k)) {
        out.push(path.to_string());
    } else {
        out.push(format!("{path}: {a} → {b}"));
    }
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
                label: sc.map(crate::i18n::shortcut_label).unwrap_or_default(),
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
        .map_err(|e: nabshot_hotkeys::ParseError| e.to_string())?;
    Ok(crate::i18n::shortcut_label(&sc))
}

/// Während ein neues Kürzel aufgenommen wird, dürfen die alten nicht auslösen.
#[tauri::command]
#[specta::specta]
pub fn pause_hotkeys(app: AppHandle, paused: bool) {
    log::debug!("Tastenkürzel pausiert: {paused}");
    if paused {
        crate::hotkeys::unregister_all(&app);
    } else {
        crate::hotkeys::register(&app);
    }
}

/// Sind die Bildschirmfoto-Kürzel des Systems (macOS ⇧⌘3/4/5) noch an?
/// `None` = gibt es auf dieser Plattform nicht.
#[tauri::command]
#[specta::specta]
pub fn system_shortcuts_active() -> Option<bool> {
    nabshot_hotkeys::system::screenshot_shortcuts_active()
}

/// Öffnet die Systemeinstellungen, in denen man diese Kürzel ausschaltet.
#[tauri::command]
#[specta::specta]
pub fn open_shortcut_settings(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    log::info!("Systemeinstellungen für Tastenkürzel öffnen");
    let url = nabshot_hotkeys::system::shortcut_settings_url().ok_or("nicht verfügbar")?;
    app.opener()
        .open_url(url, None::<&str>)
        .inspect_err(|e| log::error!("Systemeinstellungen öffnen: {e}"))
        .map_err(|e| e.to_string())
}

/// Ordner für den Auto-Export wählen.
#[tauri::command]
#[specta::specta]
pub async fn choose_folder(app: AppHandle) -> Option<String> {
    tauri::async_runtime::spawn_blocking(move || {
        let folder = app
            .dialog()
            .file()
            .blocking_pick_folder()
            .and_then(|p| {
                p.into_path()
                    .inspect_err(|e| log::warn!("Ordner wählen: {e}"))
                    .ok()
            })
            .map(|p| p.to_string_lossy().into_owned());
        log::info!("Ordner gewählt: {}", folder.is_some());
        folder
    })
    .await
    .inspect_err(|e| log::warn!("Ordner wählen: {e}"))
    .ok()
    .flatten()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn geaenderte_felder() {
        let old = Settings::default();
        let mut new = old.clone();
        new.content_search = !old.content_search;
        new.auto_export.folder = Some("/Users/x/Bilder".into());
        new.video.fps = old.video.fps + 1;
        let changed = changed_fields(&old, &new);
        assert_eq!(changed.len(), 3, "{changed:?}");
        assert!(changed.contains(&"auto_export.folder".to_string()));
        assert!(changed.iter().any(|c| c.starts_with("content_search: ")));
        assert!(changed.iter().any(|c| c.starts_with("video.fps: ")));
        assert!(changed_fields(&old, &old).is_empty());
    }
}
