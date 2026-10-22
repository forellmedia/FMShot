//! Speicherplatz von Bibliothek und Auto-Export: anzeigen, warnen,
//! aufräumen. Gemessen wird in `nabshot-library`.

use std::sync::Mutex;
use std::time::{Duration, Instant};

use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Emitter, Manager};

use crate::capture::CaptureState;
use crate::editor::CaptureInfo;

/// Höchstens so oft erscheint der Hinweis von selbst.
const WARN_EVERY: Duration = Duration::from_secs(24 * 3600);

#[derive(Default)]
pub struct StorageState {
    last_warning: Mutex<Option<Instant>>,
    /// Hinweis wartet, bis das Hauptfenster ihn abholt.
    pending: Mutex<bool>,
}

/// Belegter Platz und Grenzen (Bytes).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StorageSummary {
    pub library_bytes: f64,
    pub export_bytes: f64,
    /// `None` = keine Grenze.
    pub library_limit: Option<f64>,
    pub export_limit: Option<f64>,
    pub library_dir: String,
    /// Wohin der Auto-Export schreibt (auch wenn er gerade aus ist).
    pub export_dir: String,
}

impl StorageSummary {
    fn over(&self) -> bool {
        self.library_limit.is_some_and(|l| self.library_bytes > l)
            || self.export_limit.is_some_and(|l| self.export_bytes > l)
    }
}

/// Eine Aufnahme in der Aufräum-Liste.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StorageItem {
    pub capture: CaptureInfo,
    pub library_bytes: f64,
    pub export_bytes: f64,
    pub export_path: Option<String>,
}

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct StorageDetails {
    pub summary: StorageSummary,
    pub items: Vec<StorageItem>,
}

fn export_dir(app: &AppHandle) -> String {
    crate::settings::get(app)
        .auto_export
        .folder
        .map(std::path::PathBuf::from)
        .or_else(dirs::desktop_dir)
        .map(|p| p.to_string_lossy().into_owned())
        .unwrap_or_default()
}

fn measure(app: &AppHandle) -> Result<(StorageSummary, nabshot_library::StorageUsage), String> {
    let state = app.state::<CaptureState>();
    let usage = state.library.storage_usage().map_err(|e| {
        log::error!("Speicher messen: {e}");
        e.to_string()
    })?;
    let limits = crate::settings::get(app).storage;
    let summary = StorageSummary {
        library_bytes: usage.library_bytes as f64,
        export_bytes: usage.export_bytes as f64,
        library_limit: limits.library_bytes().map(|b| b as f64),
        export_limit: limits.export_bytes().map(|b| b as f64),
        library_dir: state.library.root().to_string_lossy().into_owned(),
        export_dir: export_dir(app),
    };
    Ok((summary, usage))
}

/// Für die Anzeige unten in der Bildliste.
#[tauri::command]
#[specta::specta]
pub fn storage_summary(app: AppHandle) -> Result<StorageSummary, String> {
    Ok(measure(&app)?.0)
}

/// Für das Aufräumen: alle Aufnahmen mit ihrem Platz.
#[tauri::command]
#[specta::specta]
pub fn storage_details(app: AppHandle) -> Result<StorageDetails, String> {
    let (summary, usage) = measure(&app)?;
    let items = usage
        .captures
        .iter()
        .map(|c| StorageItem {
            capture: CaptureInfo::from(&c.record),
            library_bytes: c.library_bytes as f64,
            export_bytes: c.export_bytes as f64,
            export_path: c
                .export_path
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned()),
        })
        .collect();
    Ok(StorageDetails { summary, items })
}

/// Aufräumen: Aufnahmen aus der Bibliothek und/oder ihre exportierten
/// Dateien löschen. Gibt die freigegebenen Bytes zurück.
#[tauri::command]
#[specta::specta]
pub fn cleanup(
    app: AppHandle,
    ids: Vec<String>,
    library: bool,
    exported: bool,
) -> Result<f64, String> {
    let lib = app.state::<CaptureState>().library.clone();
    let started = Instant::now();
    // Grössen vorher messen – nach dem Löschen sind sie weg.
    let sizes: std::collections::HashMap<String, u64> = if library {
        lib.storage_usage()
            .map(|u| {
                u.captures
                    .into_iter()
                    .map(|c| (c.record.id, c.library_bytes))
                    .collect()
            })
            .unwrap_or_default()
    } else {
        Default::default()
    };
    let mut freed = 0u64;
    let mut failed = 0;
    for id in &ids {
        if exported {
            match lib.delete_export(id) {
                Ok(n) => freed += n,
                Err(e) => {
                    failed += 1;
                    log::error!("Aufräumen: Export von {id}: {e}");
                }
            }
        }
        if library {
            match lib.delete(id) {
                Ok(()) => freed += sizes.get(id).copied().unwrap_or(0),
                Err(e) => {
                    failed += 1;
                    log::error!("Aufräumen: Aufnahme {id}: {e}");
                }
            }
        }
    }
    log::info!(
        "Aufräumen: {} Aufnahmen (Bibliothek {library}, Export {exported}), {} MB frei, {failed} Fehler, {} ms",
        ids.len(),
        freed / 1_000_000,
        started.elapsed().as_millis()
    );
    if let Err(e) = app.emit("library-changed", ()) {
        log::debug!("Ereignis library-changed: {e}");
    }
    if failed > 0 {
        return Err(format!("{failed} von {} nicht gelöscht", ids.len()));
    }
    Ok(freed as f64)
}

/// Öffnet den Ordner der Bibliothek bzw. des Auto-Exports.
#[tauri::command]
#[specta::specta]
pub fn open_storage_folder(app: AppHandle, exported: bool) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let dir = if exported {
        export_dir(&app)
    } else {
        app.state::<CaptureState>()
            .library
            .root()
            .to_string_lossy()
            .into_owned()
    };
    app.opener()
        .open_path(dir, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Hat der Hintergrund-Job einen Hinweis hinterlegt? (Einmal abholen.)
#[tauri::command]
#[specta::specta]
pub fn take_storage_warning(app: AppHandle) -> bool {
    std::mem::take(
        &mut *app
            .state::<StorageState>()
            .pending
            .lock()
            .unwrap_or_else(|e| e.into_inner()),
    )
}

/// Hauptfenster mit dem Aufräumen öffnen (z. B. aus den Einstellungen).
pub fn open_cleanup(app: &AppHandle) {
    *app.state::<StorageState>()
        .pending
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = true;
    crate::tray::show_main_window(app);
    if let Err(e) = app.emit("storage-warning", ()) {
        log::debug!("Ereignis storage-warning: {e}");
    }
}

/// Hintergrund-Job: Zu viel belegt? Dann höchstens einmal am Tag das
/// Hauptfenster mit dem Aufräumen öffnen.
pub fn check(app: &AppHandle) {
    // Nicht mitten in eine Aufnahme hinein; der nächste Durchgang kommt.
    if crate::video::is_active(app) {
        log::debug!("Speicher prüfen: Aufnahme läuft – später");
        return;
    }
    let Ok((summary, _)) = measure(app) else {
        return;
    };
    log::debug!(
        "Speicher: Bibliothek {} MB / {:?}, Export {} MB / {:?}",
        summary.library_bytes as u64 / 1_000_000,
        summary.library_limit.map(|b| b as u64 / 1_000_000),
        summary.export_bytes as u64 / 1_000_000,
        summary.export_limit.map(|b| b as u64 / 1_000_000)
    );
    if !summary.over() {
        return;
    }
    let state = app.state::<StorageState>();
    {
        let mut last = state.last_warning.lock().unwrap_or_else(|e| e.into_inner());
        if last.is_some_and(|t| t.elapsed() < WARN_EVERY) {
            return;
        }
        *last = Some(Instant::now());
    }
    log::warn!(
        "Speicher über der Grenze: Bibliothek {} MB, Export {} MB – Aufräumen anbieten",
        summary.library_bytes as u64 / 1_000_000,
        summary.export_bytes as u64 / 1_000_000
    );
    *state.pending.lock().unwrap_or_else(|e| e.into_inner()) = true;
    let handle = app.clone();
    if let Err(e) = app.run_on_main_thread(move || crate::tray::show_main_window(&handle)) {
        log::warn!("Hauptfenster für Speicher-Hinweis: {e}");
    }
    if let Err(e) = app.emit("storage-warning", ()) {
        log::debug!("Ereignis storage-warning: {e}");
    }
}
