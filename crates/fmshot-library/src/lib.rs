//! Die Bibliothek aller Aufnahmen (ADR 0004).
//!
//! ```text
//! Library/
//!   index.sqlite
//!   2026/10/<id>/
//!     original.png  doc.json  thumb.webp  assets/<hash>.png
//! ```
//!
//! Index, Suche, Auto-Export und Aufbewahrung folgen in Meilenstein 2 und 6.

use std::path::{Path, PathBuf};

pub const ORIGINAL_FILE: &str = "original.png";
pub const DOCUMENT_FILE: &str = "doc.json";
pub const THUMBNAIL_FILE: &str = "thumb.webp";
pub const ASSETS_DIR: &str = "assets";
pub const INDEX_FILE: &str = "index.sqlite";

/// Eindeutige, zeitlich sortierbare ID einer Aufnahme,
/// z. B. `20261016-174512-083-0`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CaptureId {
    id: String,
    year: i32,
    month: u32,
}

impl CaptureId {
    /// `seq` unterscheidet Aufnahmen innerhalb derselben Millisekunde.
    pub fn from_unix_millis(millis: i64, seq: u32) -> Self {
        let secs = millis.div_euclid(1000);
        let ms = millis.rem_euclid(1000);
        let days = secs.div_euclid(86_400);
        let sod = secs.rem_euclid(86_400);
        let (year, month, day) = civil_from_days(days);
        let id = format!(
            "{year:04}{month:02}{day:02}-{:02}{:02}{:02}-{ms:03}-{seq}",
            sod / 3600,
            (sod % 3600) / 60,
            sod % 60,
        );
        Self { id, year, month }
    }

    pub fn as_str(&self) -> &str {
        &self.id
    }
}

impl std::fmt::Display for CaptureId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.id)
    }
}

#[derive(Debug, Clone)]
pub struct Library {
    root: PathBuf,
}

impl Library {
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn index_path(&self) -> PathBuf {
        self.root.join(INDEX_FILE)
    }

    /// Ordner einer Aufnahme: `<root>/<jahr>/<monat>/<id>`.
    pub fn capture_dir(&self, id: &CaptureId) -> PathBuf {
        self.root
            .join(format!("{:04}", id.year))
            .join(format!("{:02}", id.month))
            .join(id.as_str())
    }
}

/// Tage seit 1970-01-01 → (Jahr, Monat, Tag), proleptischer gregorianischer
/// Kalender, UTC. Algorithmus nach Howard Hinnant („civil_from_days“).
fn civil_from_days(days: i64) -> (i32, u32, u32) {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let month = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let year = (yoe + era * 400 + i64::from(month <= 2)) as i32;
    (year, month, day)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn id_aus_zeitstempel() {
        // 2026-10-16 17:45:12.083 UTC
        let id = CaptureId::from_unix_millis(1_792_172_712_083, 0);
        assert_eq!(id.as_str(), "20261016-174512-083-0");
    }

    #[test]
    fn epoche_und_schaltjahr() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 2024-02-29
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    #[test]
    fn ordner_nach_jahr_und_monat() {
        let lib = Library::new("/lib");
        let id = CaptureId::from_unix_millis(1_792_172_712_083, 3);
        assert_eq!(
            lib.capture_dir(&id),
            PathBuf::from("/lib/2026/10/20261016-174512-083-3")
        );
    }
}
