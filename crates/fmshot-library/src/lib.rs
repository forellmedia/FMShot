//! Die Bibliothek aller Aufnahmen (ADR 0004).
//!
//! Alle Dateien eines Monats liegen flach in einem Ordner, zusammengehalten
//! durch die ID am Anfang des Dateinamens:
//!
//! ```text
//! Library/
//!   index.sqlite
//!   2026/10/
//!     20261016-220122-944-0.json            Dokument (Ebenen)
//!     20261016-220122-944-0_original.png    Originalaufnahme, unverändert
//!     20261016-220122-944-0_thumb.webp      Vorschaubild
//!     20261016-220122-944-0_asset_<hash>.png  hineingezogene Bilder
//! ```
//!
//! Suche, Auto-Export und Aufbewahrung folgen in Meilenstein 6.

mod encode;
mod index;
mod store;

use std::path::{Path, PathBuf};

pub use encode::{encode_jpeg, encode_png, thumbnail};
pub use index::{CaptureQuery, CaptureRecord};
pub use store::{Library, LibraryError, NewCapture};

/// Längste Kante der Vorschaubilder in Pixeln.
pub const THUMBNAIL_MAX: u32 = 320;
pub const INDEX_FILE: &str = "index.sqlite";

/// Eindeutige, zeitlich sortierbare ID einer Aufnahme in Ortszeit,
/// z. B. `20261016-224512-083-0`.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct CaptureId {
    id: String,
    year: i32,
    month: u32,
}

impl CaptureId {
    /// ID in der Zeitzone des Systems. `seq` unterscheidet Aufnahmen
    /// innerhalb derselben Millisekunde.
    pub fn local(millis: i64, seq: u32) -> Self {
        let offset = chrono::DateTime::from_timestamp_millis(millis)
            .map(|utc| utc.with_timezone(&chrono::Local).offset().local_minus_utc() as i64)
            .unwrap_or(0);
        Self::with_offset(millis, offset, seq)
    }

    /// ID mit festem Versatz zu UTC in Sekunden (vor allem für Tests).
    pub fn with_offset(millis: i64, offset_secs: i64, seq: u32) -> Self {
        let local = millis + offset_secs * 1000;
        let secs = local.div_euclid(1000);
        let ms = local.rem_euclid(1000);
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

    /// Monatsordner relativ zur Bibliothek, z. B. `2026/10`.
    pub fn month_dir(&self) -> PathBuf {
        PathBuf::from(format!("{:04}", self.year)).join(format!("{:02}", self.month))
    }
}

impl std::fmt::Display for CaptureId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.id)
    }
}

/// Dateinamen einer Aufnahme im Monatsordner.
pub fn document_file(id: &str) -> String {
    format!("{id}.json")
}

pub fn original_file(id: &str) -> String {
    format!("{id}_original.png")
}

pub fn thumbnail_file(id: &str) -> String {
    format!("{id}_thumb.webp")
}

pub fn asset_file(id: &str, hash: &str) -> String {
    format!("{id}_asset_{hash}.png")
}

impl CaptureRecord {
    pub fn document_path(&self) -> PathBuf {
        self.dir.join(document_file(&self.id))
    }

    pub fn original_path(&self) -> PathBuf {
        self.dir.join(original_file(&self.id))
    }

    pub fn thumbnail_path(&self) -> PathBuf {
        self.dir.join(thumbnail_file(&self.id))
    }

    pub fn asset_path(&self, hash: &str) -> PathBuf {
        self.dir.join(asset_file(&self.id, hash))
    }
}

/// Monatsordner einer Aufnahme.
pub fn month_dir(root: &Path, id: &CaptureId) -> PathBuf {
    root.join(id.month_dir())
}

/// Tage seit 1970-01-01 → (Jahr, Monat, Tag), proleptischer gregorianischer
/// Kalender. Algorithmus nach Howard Hinnant („civil_from_days“).
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
    fn id_aus_zeitstempel_in_ortszeit() {
        // 2026-10-16 17:45:12.083 UTC = 19:45:12 in Zürich (Sommerzeit)
        let id = CaptureId::with_offset(1_792_172_712_083, 2 * 3600, 0);
        assert_eq!(id.as_str(), "20261016-194512-083-0");
    }

    #[test]
    fn monatswechsel_durch_zeitzone() {
        // 2026-10-31 23:30 UTC = 2026-11-01 00:30 bei +1 h
        let ms = 1_793_489_400_000;
        let id = CaptureId::with_offset(ms, 3600, 0);
        assert_eq!(id.month_dir(), PathBuf::from("2026/11"));
    }

    #[test]
    fn epoche_und_schaltjahr() {
        assert_eq!(civil_from_days(0), (1970, 1, 1));
        // 2024-02-29
        assert_eq!(civil_from_days(19_782), (2024, 2, 29));
    }

    #[test]
    fn dateinamen_im_monatsordner() {
        let id = CaptureId::with_offset(1_792_172_712_083, 0, 3);
        assert_eq!(
            month_dir(Path::new("/lib"), &id),
            PathBuf::from("/lib/2026/10")
        );
        assert_eq!(
            original_file(id.as_str()),
            "20261016-174512-083-3_original.png"
        );
        assert_eq!(
            asset_file(id.as_str(), "ab12"),
            "20261016-174512-083-3_asset_ab12.png"
        );
    }
}
