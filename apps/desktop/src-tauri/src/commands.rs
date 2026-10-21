use serde::Serialize;
use specta::Type;

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct AppInfo {
    pub version: String,
    /// `macos`, `windows` oder `linux`
    pub platform: String,
    pub arch: String,
}

/// Version und Plattform – für „Über Grabmark“ und „Fehler melden“.
#[tauri::command]
#[specta::specta]
pub fn app_info(app: tauri::AppHandle) -> AppInfo {
    AppInfo {
        version: app.package_info().version.to_string(),
        platform: std::env::consts::OS.into(),
        arch: std::env::consts::ARCH.into(),
    }
}
