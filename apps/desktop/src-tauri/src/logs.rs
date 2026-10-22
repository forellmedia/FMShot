//! Log-Dateien: Format, Rotation, Aufräumen nach Alter und Meldungen aus dem
//! Frontend.
//!
//! Eine Datei wächst bis [`MAX_FILE_SIZE`], dann benennt das Plugin sie mit
//! Datum um (`Nabshot_2026-10-22_17-20-51.log`) und beginnt eine neue. Alte
//! Dateien löscht [`prune`] nach den Tagen aus den Einstellungen.

use std::path::{Path, PathBuf};
use std::time::{Duration, SystemTime};

use tauri::{AppHandle, Runtime};
use tauri_plugin_log::{RotationStrategy, Target, TargetKind, TimezoneStrategy};
use time::OffsetDateTime;
use time::macros::format_description;

/// Grösse einer Datei, bevor die nächste beginnt.
const MAX_FILE_SIZE: u128 = 5_000_000;

/// Obergrenze für alle Dateien zusammen – falls etwas Amok läuft.
const MAX_TOTAL_SIZE: u64 = 500_000_000;

/// Das Log-Plugin: Ortszeit mit Millisekunden; Nabshot selbst ab `debug`,
/// fremde Bibliotheken ab `info`.
pub fn plugin<R: Runtime>() -> tauri::plugin::TauriPlugin<R> {
    let time_format =
        format_description!("[year]-[month]-[day] [hour]:[minute]:[second].[subsecond digits:3]");
    // Zuerst die Zeitzone: Sie setzt ein eigenes Format, das `format` danach
    // ersetzt (sie gilt auch für die Namen rotierter Dateien).
    tauri_plugin_log::Builder::new()
        .timezone_strategy(TimezoneStrategy::UseLocal)
        .level(log::LevelFilter::Debug)
        .filter(|meta| meta.target().starts_with("nabshot") || meta.level() <= log::Level::Info)
        .format(move |out, message, record| {
            let now = TimezoneStrategy::UseLocal.get_now();
            out.finish(format_args!(
                "{} {:<5} [{}] {}",
                now.format(&time_format).unwrap_or_default(),
                record.level(),
                record.target(),
                message
            ))
        })
        .max_file_size(MAX_FILE_SIZE)
        .rotation_strategy(RotationStrategy::KeepAll)
        .targets([
            Target::new(TargetKind::Stdout),
            Target::new(TargetKind::Folder {
                path: crate::paths::get().log.clone(),
                file_name: None,
            }),
        ])
        .build()
}

/// Die aktuelle Log-Datei.
pub fn current_file(app: &AppHandle) -> PathBuf {
    crate::paths::get()
        .log
        .join(format!("{}.log", app.package_info().name))
}

/// Löscht Log-Dateien, die länger als `days` nicht mehr beschrieben wurden,
/// und die ältesten, falls alle zusammen zu gross sind. Die aktuelle Datei
/// bleibt immer. Gibt die gelöschten Dateien zurück.
pub fn prune(dir: &Path, current: &Path, days: u32, now: SystemTime) -> Vec<PathBuf> {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return Vec::new();
    };
    let mut files: Vec<(PathBuf, SystemTime, u64)> = entries
        .filter_map(|e| {
            let path = e.ok()?.path();
            let name = path.file_name()?.to_str()?;
            if path == current || !(name.ends_with(".log") || name.ends_with(".log.bak")) {
                return None;
            }
            let meta = path.metadata().ok()?;
            Some((path, meta.modified().ok()?, meta.len()))
        })
        .collect();
    // Älteste zuerst.
    files.sort_by_key(|(_, modified, _)| *modified);

    let max_age = Duration::from_secs(u64::from(days) * 86_400);
    let mut total: u64 = files.iter().map(|(_, _, len)| len).sum::<u64>()
        + current.metadata().map(|m| m.len()).unwrap_or(0);
    let mut removed = Vec::new();
    for (path, modified, len) in files {
        let too_old = now.duration_since(modified).unwrap_or_default() > max_age;
        if !too_old && total <= MAX_TOTAL_SIZE {
            continue;
        }
        match std::fs::remove_file(&path) {
            Ok(()) => {
                total = total.saturating_sub(len);
                removed.push(path);
            }
            Err(e) => log::warn!("Log löschen {}: {e}", path.display()),
        }
    }
    removed
}

/// Aufräumen mit den Tagen aus den Einstellungen.
pub fn prune_now(app: &AppHandle) {
    // Mindestens ein Tag, auch wenn die Datei von Hand verändert wurde.
    let days = crate::settings::get(app).log_retention_days.max(1);
    let removed = prune(
        &crate::paths::get().log,
        &current_file(app),
        days,
        SystemTime::now(),
    );
    if !removed.is_empty() {
        log::info!(
            "Logs älter als {days} Tage gelöscht: {}",
            removed
                .iter()
                .filter_map(|p| p.file_name()?.to_str())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
}

/// Erste Zeilen nach dem Start: womit läuft Nabshot?
pub fn log_startup(app: &AppHandle) {
    let info = app.package_info();
    log::info!(
        "──── Nabshot {} gestartet · {} {} · {} · PID {}",
        info.version,
        std::env::consts::OS,
        std::env::consts::ARCH,
        os_version().unwrap_or_else(|| "?".into()),
        std::process::id()
    );
    let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    log::info!("Zeitzone UTC{}", now.offset());
    log::info!("Log-Ordner: {}", crate::paths::get().log.display());
}

/// macOS-Version aus der Systemdatei; andere Systeme: noch nichts.
fn os_version() -> Option<String> {
    #[cfg(target_os = "macos")]
    {
        let plist =
            std::fs::read_to_string("/System/Library/CoreServices/SystemVersion.plist").ok()?;
        let after = plist.split("<key>ProductVersion</key>").nth(1)?;
        let value = after.split("<string>").nth(1)?.split("</string>").next()?;
        Some(format!("macOS {value}"))
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}

/// Stufe einer Meldung aus dem Frontend.
#[derive(Debug, Clone, Copy, serde::Deserialize, specta::Type)]
#[serde(rename_all = "lowercase")]
pub enum FrontendLevel {
    Error,
    Warn,
    Info,
    Debug,
}

/// Fehler und Hinweise aus den Fenstern ins gleiche Log schreiben.
#[tauri::command]
#[specta::specta]
pub fn log_frontend(window: tauri::Window, level: FrontendLevel, message: String) {
    let target = format!("nabshot_frontend::{}", window.label());
    let level = match level {
        FrontendLevel::Error => log::Level::Error,
        FrontendLevel::Warn => log::Level::Warn,
        FrontendLevel::Info => log::Level::Info,
        FrontendLevel::Debug => log::Level::Debug,
    };
    // Sehr lange Meldungen (z. B. ganze Objekte) kürzen.
    let message: String = message.chars().take(4000).collect();
    log::log!(target: &target, level, "{message}");
}

/// Öffnet den Ordner mit den Log-Dateien.
#[tauri::command]
#[specta::specta]
pub fn open_log_folder(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let dir = &crate::paths::get().log;
    std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    app.opener()
        .open_path(dir.to_string_lossy(), None::<&str>)
        .map_err(|e| e.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn touch(path: &Path, age_days: u64, len: usize) {
        std::fs::write(path, vec![b'x'; len]).unwrap();
        let modified = SystemTime::now() - Duration::from_secs(age_days * 86_400);
        std::fs::File::options()
            .write(true)
            .open(path)
            .unwrap()
            .set_modified(modified)
            .unwrap();
    }

    #[test]
    fn alte_logs_werden_geloescht() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path();
        let current = dir.join("Nabshot.log");
        touch(&current, 100, 10);
        touch(&dir.join("Nabshot_2026-01-01_10-00-00.log"), 40, 10);
        touch(&dir.join("Nabshot_2026-10-01_10-00-00.log"), 5, 10);
        touch(&dir.join("FMShot.log"), 400, 10);
        touch(&dir.join("notizen.txt"), 400, 10);

        let mut removed: Vec<String> = prune(dir, &current, 30, SystemTime::now())
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        removed.sort();
        assert_eq!(removed, ["FMShot.log", "Nabshot_2026-01-01_10-00-00.log"]);
        // Aktuelle Datei bleibt, auch wenn sie alt ist; fremde Dateien auch.
        assert!(current.exists());
        assert!(dir.join("notizen.txt").exists());
        assert!(dir.join("Nabshot_2026-10-01_10-00-00.log").exists());
    }

    #[test]
    fn fehlender_ordner_ist_kein_fehler() {
        let tmp = tempfile::tempdir().unwrap();
        let dir = tmp.path().join("gibt-es-nicht");
        assert!(prune(&dir, &dir.join("Nabshot.log"), 30, SystemTime::now()).is_empty());
    }
}
