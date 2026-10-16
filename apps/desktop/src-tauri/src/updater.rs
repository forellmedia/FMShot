//! Updates über GitHub Releases (`latest.json`), signiert mit dem
//! Updater-Schlüssel aus `scripts/make-updater-key.sh`.

use serde::Serialize;
use specta::Type;
use tauri::AppHandle;
use tauri_plugin_updater::UpdaterExt;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    pub current_version: String,
    /// Release-Notizen aus GitHub
    pub notes: Option<String>,
}

/// Prüft, ob eine neuere Version veröffentlicht ist.
#[tauri::command]
#[specta::specta]
pub async fn check_for_update(app: AppHandle) -> Result<Option<UpdateInfo>, String> {
    let update = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?;
    Ok(update.map(|u| UpdateInfo {
        version: u.version.clone(),
        current_version: u.current_version.clone(),
        notes: u.body.clone(),
    }))
}

/// Lädt das Update, prüft die Signatur, installiert und startet neu.
#[tauri::command]
#[specta::specta]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    let Some(update) = app
        .updater()
        .map_err(|e| e.to_string())?
        .check()
        .await
        .map_err(|e| e.to_string())?
    else {
        return Ok(());
    };
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| e.to_string())?;
    app.restart()
}
