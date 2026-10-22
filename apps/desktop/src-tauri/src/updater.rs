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
    let start = std::time::Instant::now();
    log::info!("Update prüfen");
    let update = check(&app).await?;
    match &update {
        Some(u) => log::info!(
            "Update verfügbar: {} → {} ({:.0} ms)",
            u.current_version,
            u.version,
            start.elapsed().as_secs_f64() * 1000.0
        ),
        None => log::info!(
            "Kein Update ({:.0} ms)",
            start.elapsed().as_secs_f64() * 1000.0
        ),
    }
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
    log::info!("Update installieren");
    let Some(update) = check(&app).await? else {
        log::warn!("Update installieren: keines mehr verfügbar");
        return Ok(());
    };
    let start = std::time::Instant::now();
    update
        .download_and_install(|_, _| {}, || {})
        .await
        .map_err(|e| {
            log::error!("Update {} installieren: {e}", update.version);
            e.to_string()
        })?;
    log::info!(
        "Update {} installiert ({:.1} s) – Neustart",
        update.version,
        start.elapsed().as_secs_f64()
    );
    log::logger().flush();
    app.restart()
}

/// Fragt nach einer neueren Version; Fehler kommen ins Log.
async fn check(app: &AppHandle) -> Result<Option<tauri_plugin_updater::Update>, String> {
    app.updater()
        .map_err(|e| {
            log::error!("Updater: {e}");
            e.to_string()
        })?
        .check()
        .await
        .map_err(|e| {
            log::error!("Update prüfen: {e}");
            e.to_string()
        })
}
