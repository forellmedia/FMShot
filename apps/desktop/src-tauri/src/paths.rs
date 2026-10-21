//! Ordner für Daten, Einstellungen, Cache und Logs.
//!
//! Tauri leitet seine Ordner aus der App-ID ab (`ch.forellmedia.fmshot`, bleibt
//! wegen der macOS-Freigaben). Grabmark legt seine Dateien stattdessen unter
//! dem eigenen Namen ab, z. B. `~/Library/Application Support/Grabmark`
//! (ADR 0013).

use std::path::PathBuf;
use std::sync::OnceLock;

const NAME: &str = "Grabmark";

pub struct AppDirs {
    /// Bibliothek.
    pub data: PathBuf,
    /// `settings.json` und Markierungsdateien (macOS/Windows: wie `data`).
    pub config: PathBuf,
    pub cache: PathBuf,
    pub log: PathBuf,
}

/// Die Ordner der App (werden erst beim Schreiben angelegt).
pub fn get() -> &'static AppDirs {
    static DIRS: OnceLock<AppDirs> = OnceLock::new();
    DIRS.get_or_init(|| {
        let base = |dir: Option<PathBuf>| dir.expect("Benutzerordner nicht gefunden").join(NAME);
        let data = base(dirs::data_dir());
        AppDirs {
            config: base(dirs::config_dir()),
            cache: base(dirs::cache_dir()),
            #[cfg(target_os = "macos")]
            log: base(dirs::home_dir().map(|h| h.join("Library/Logs"))),
            #[cfg(not(target_os = "macos"))]
            log: data.join("logs"),
            data,
        }
    })
}
