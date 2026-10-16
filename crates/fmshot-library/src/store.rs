use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use fmshot_core::{Document, RgbaImage};
use rusqlite::Connection;

use crate::encode::{encode_png, encode_webp, thumbnail};
use crate::index::{self, CaptureRecord};
use crate::{
    CaptureId, DOCUMENT_FILE, INDEX_FILE, ORIGINAL_FILE, THUMBNAIL_FILE, THUMBNAIL_MAX, capture_dir,
};

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("Dateifehler: {0}")]
    Io(#[from] io::Error),
    #[error("Index-Fehler: {0}")]
    Index(#[from] rusqlite::Error),
    #[error("Kodierung fehlgeschlagen: {0}")]
    Encode(String),
    #[error("doc.json: {0}")]
    Json(#[from] serde_json::Error),
}

pub type Result<T> = std::result::Result<T, LibraryError>;

/// Eine frische Aufnahme, die gespeichert werden soll.
#[derive(Debug, Clone)]
pub struct NewCapture {
    pub image: RgbaImage,
    pub scale_factor: f32,
    /// z. B. `region`, `screen`
    pub mode: String,
    /// Unix-Zeit in Millisekunden
    pub created_at_ms: i64,
}

/// Die Bibliothek aller Aufnahmen. Darf zwischen Threads geteilt werden
/// (`Arc<Library>`); Schreiben ins Dateisystem ist blockierend und gehört in
/// einen Hintergrund-Thread.
pub struct Library {
    root: PathBuf,
    index: Mutex<Connection>,
    seq: AtomicU32,
}

impl Library {
    /// Öffnet die Bibliothek und legt sie bei Bedarf an.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        let conn = index::open(&root.join(INDEX_FILE))?;
        Ok(Self {
            root,
            index: Mutex::new(conn),
            seq: AtomicU32::new(0),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Legt die Aufnahme an: Ordner, `original.png`, `doc.json`,
    /// `thumb.webp` und Eintrag im Index.
    pub fn save_capture(&self, capture: &NewCapture) -> Result<CaptureRecord> {
        let (id, dir) = self.new_capture_dir(capture.created_at_ms)?;

        let png = encode_png(&capture.image, capture.scale_factor)
            .map_err(|e| LibraryError::Encode(e.to_string()))?;
        write_atomic(&dir.join(ORIGINAL_FILE), &png)?;

        let doc = Document::from_capture(capture.image.width(), capture.image.height());
        write_atomic(&dir.join(DOCUMENT_FILE), &serde_json::to_vec_pretty(&doc)?)?;

        let thumb = encode_webp(&thumbnail(&capture.image, THUMBNAIL_MAX))
            .map_err(|e| LibraryError::Encode(e.to_string()))?;
        write_atomic(&dir.join(THUMBNAIL_FILE), &thumb)?;

        let record = CaptureRecord {
            id: id.to_string(),
            created_at_ms: capture.created_at_ms,
            mode: capture.mode.clone(),
            width: capture.image.width(),
            height: capture.image.height(),
            scale_factor: capture.scale_factor,
            dir,
            starred: false,
        };
        index::insert(&self.lock(), &self.root, &record)?;
        Ok(record)
    }

    /// Neueste Aufnahmen zuerst.
    pub fn recent(&self, limit: u32) -> Result<Vec<CaptureRecord>> {
        Ok(index::recent(&self.lock(), &self.root, limit)?)
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        // Ein Panik in einem anderen Thread macht die Verbindung nicht kaputt.
        self.index.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Findet eine freie ID und legt den Ordner an.
    fn new_capture_dir(&self, created_at_ms: i64) -> Result<(CaptureId, PathBuf)> {
        loop {
            let seq = self.seq.fetch_add(1, Ordering::Relaxed);
            let id = CaptureId::from_unix_millis(created_at_ms, seq);
            let dir = capture_dir(&self.root, &id);
            if let Some(parent) = dir.parent() {
                fs::create_dir_all(parent)?;
            }
            match fs::create_dir(&dir) {
                Ok(()) => return Ok((id, dir)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        }
    }
}

/// Schreibt erst in eine temporäre Datei und benennt dann um – so liegt nie
/// eine halbe Datei in der Bibliothek.
fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    let tmp = path.with_extension("tmp");
    fs::write(&tmp, data)?;
    fs::rename(&tmp, path)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn capture(ms: i64) -> NewCapture {
        NewCapture {
            image: RgbaImage::filled(640, 400, [200, 10, 10, 255]),
            scale_factor: 2.0,
            mode: "region".into(),
            created_at_ms: ms,
        }
    }

    #[test]
    fn speichern_legt_alle_dateien_und_index_an() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::open(tmp.path()).unwrap();
        let rec = lib.save_capture(&capture(1_792_172_712_083)).unwrap();

        for f in [ORIGINAL_FILE, DOCUMENT_FILE, THUMBNAIL_FILE] {
            assert!(rec.dir.join(f).is_file(), "{f} fehlt");
        }
        assert!(rec.dir.starts_with(tmp.path().join("2026/10")));
        let doc: Document =
            serde_json::from_slice(&fs::read(rec.dir.join(DOCUMENT_FILE)).unwrap()).unwrap();
        assert_eq!(doc.artboard.rect.width, 640.0);

        assert_eq!(lib.recent(10).unwrap(), vec![rec]);
    }

    #[test]
    fn gleiche_millisekunde_gibt_verschiedene_ids() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::open(tmp.path()).unwrap();
        let a = lib.save_capture(&capture(1_000)).unwrap();
        let b = lib.save_capture(&capture(1_000)).unwrap();
        assert_ne!(a.id, b.id);
        assert_eq!(lib.recent(10).unwrap().len(), 2);
    }

    #[test]
    fn index_ueberlebt_neues_oeffnen() {
        let tmp = tempfile::tempdir().unwrap();
        Library::open(tmp.path())
            .unwrap()
            .save_capture(&capture(5_000))
            .unwrap();
        let again = Library::open(tmp.path()).unwrap();
        assert_eq!(again.recent(1).unwrap().len(), 1);
    }
}
