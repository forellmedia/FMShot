//! Ordner für Daten, Einstellungen, Cache und Logs.
//!
//! Tauri leitet seine Ordner aus der App-ID ab (`ch.forellmedia.fmshot`, bleibt
//! wegen der macOS-Freigaben). Nabshot legt seine Dateien stattdessen unter
//! dem eigenen Namen ab, z. B. `~/Library/Application Support/Nabshot`
//! (ADR 0013, ADR 0015).

use std::path::PathBuf;
use std::sync::OnceLock;

const NAME: &str = "Nabshot";
/// Früherer Name (ADR 0015): Seine Ordner zieht der erste Start um.
const OLD_NAME: &str = "Grabmark";

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

/// Meldungen des Umzugs – fürs Log, das erst danach startet.
static MIGRATION: OnceLock<Vec<String>> = OnceLock::new();

/// Ordner unter dem alten Namen in den neuen umbenennen (gleiches Laufwerk,
/// kein Kopieren; die Bibliothek speichert nur relative Pfade). Gibt es den
/// neuen Ordner schon, bleibt alles, wie es ist. Läuft vor dem Log.
pub fn migrate_old_name() {
    let mut bases: Vec<Option<PathBuf>> =
        vec![dirs::data_dir(), dirs::config_dir(), dirs::cache_dir()];
    #[cfg(target_os = "macos")]
    bases.push(dirs::home_dir().map(|h| h.join("Library/Logs")));
    let mut bases: Vec<PathBuf> = bases.into_iter().flatten().collect();
    bases.sort();
    bases.dedup();
    let notes = bases
        .iter()
        .filter_map(|base| migrate(&base.join(OLD_NAME), &base.join(NAME)))
        .collect();
    let _ = MIGRATION.set(notes);
}

fn migrate(old: &std::path::Path, new: &std::path::Path) -> Option<String> {
    if !old.is_dir() {
        return None;
    }
    if new.exists() {
        return Some(format!(
            "Umzug: {} bleibt liegen, {} gibt es schon",
            old.display(),
            new.display()
        ));
    }
    Some(match std::fs::rename(old, new) {
        Ok(()) => format!("Umzug: {} → {}", old.display(), new.display()),
        Err(e) => format!("Umzug {} fehlgeschlagen: {e}", old.display()),
    })
}

/// Was der Umzug beim Start getan hat.
pub fn migration_notes() -> &'static [String] {
    MIGRATION.get().map(Vec::as_slice).unwrap_or(&[])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn alter_ordner_wird_umbenannt() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("Grabmark");
        let new = tmp.path().join("Nabshot");
        std::fs::create_dir_all(old.join("Library")).unwrap();
        std::fs::write(old.join("settings.json"), b"{}").unwrap();
        assert!(migrate(&old, &new).unwrap().starts_with("Umzug:"));
        assert!(new.join("settings.json").exists());
        assert!(new.join("Library").is_dir());
        assert!(!old.exists());
        // Nichts mehr zu tun.
        assert_eq!(migrate(&old, &new), None);
    }

    #[test]
    fn vorhandener_neuer_ordner_wird_nicht_ueberschrieben() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("Grabmark");
        let new = tmp.path().join("Nabshot");
        std::fs::create_dir_all(&old).unwrap();
        std::fs::create_dir_all(&new).unwrap();
        std::fs::write(new.join("settings.json"), b"neu").unwrap();
        assert!(migrate(&old, &new).unwrap().contains("bleibt liegen"));
        assert!(old.exists());
        assert_eq!(std::fs::read(new.join("settings.json")).unwrap(), b"neu");
    }
}
