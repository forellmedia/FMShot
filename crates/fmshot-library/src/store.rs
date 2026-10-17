use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};

use fmshot_core::{Document, RgbaImage};
use rusqlite::Connection;

use crate::encode::{encode_png, encode_webp, thumbnail};
use crate::index::{self, CaptureQuery, CaptureRecord};
use crate::{
    CaptureId, INDEX_FILE, THUMBNAIL_MAX, asset_file, document_file, month_dir, original_file,
    thumbnail_file,
};

#[derive(Debug, thiserror::Error)]
pub enum LibraryError {
    #[error("Dateifehler: {0}")]
    Io(#[from] io::Error),
    #[error("Index-Fehler: {0}")]
    Index(#[from] rusqlite::Error),
    #[error("Kodierung fehlgeschlagen: {0}")]
    Encode(String),
    #[error("Dokument: {0}")]
    Json(#[from] serde_json::Error),
    #[error("Aufnahme {0} nicht gefunden")]
    NotFound(String),
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
    /// Öffnet die Bibliothek, legt sie bei Bedarf an und bringt ältere
    /// Ablagen auf den aktuellen Stand.
    pub fn open(root: impl Into<PathBuf>) -> Result<Self> {
        let root = root.into();
        fs::create_dir_all(&root)?;
        let conn = index::open(&root.join(INDEX_FILE))?;
        migrate(&conn, &root)?;
        Ok(Self {
            root,
            index: Mutex::new(conn),
            seq: AtomicU32::new(0),
        })
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    /// Legt die Aufnahme an: Dokument, Original, Vorschaubild, Index-Eintrag.
    pub fn save_capture(&self, capture: &NewCapture) -> Result<CaptureRecord> {
        let (id, dir) = self.reserve_id(capture.created_at_ms)?;
        let id = id.to_string();

        let png = encode_png(&capture.image, capture.scale_factor)
            .map_err(|e| LibraryError::Encode(e.to_string()))?;
        write_atomic(&dir.join(original_file(&id)), &png)?;

        let doc = Document::from_capture_scaled(
            capture.image.width(),
            capture.image.height(),
            capture.scale_factor,
        );
        write_atomic(
            &dir.join(document_file(&id)),
            &serde_json::to_vec_pretty(&doc)?,
        )?;

        let thumb = encode_webp(&thumbnail(&capture.image, THUMBNAIL_MAX))
            .map_err(|e| LibraryError::Encode(e.to_string()))?;
        write_atomic(&dir.join(thumbnail_file(&id)), &thumb)?;

        let record = CaptureRecord {
            id,
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

    pub fn get(&self, id: &str) -> Result<CaptureRecord> {
        index::get(&self.lock(), &self.root, id)?.ok_or_else(|| LibraryError::NotFound(id.into()))
    }

    pub fn load_document(&self, id: &str) -> Result<Document> {
        let rec = self.get(id)?;
        Ok(serde_json::from_slice(&fs::read(rec.document_path())?)?)
    }

    /// Bytes der Originalaufnahme (PNG).
    pub fn read_original(&self, id: &str) -> Result<Vec<u8>> {
        Ok(fs::read(self.get(id)?.original_path())?)
    }

    /// Bytes eines hineingezogenen Bildes (PNG).
    pub fn read_asset(&self, id: &str, hash: &str) -> Result<Vec<u8>> {
        Ok(fs::read(self.get(id)?.asset_path(hash))?)
    }

    /// Speichert das Dokument und – falls übergeben – ein neues Vorschaubild.
    pub fn save_document(
        &self,
        id: &str,
        doc: &Document,
        preview: Option<&RgbaImage>,
    ) -> Result<()> {
        let rec = self.get(id)?;
        write_atomic(&rec.document_path(), &serde_json::to_vec_pretty(doc)?)?;
        if let Some(preview) = preview {
            let thumb = encode_webp(&thumbnail(preview, THUMBNAIL_MAX))
                .map_err(|e| LibraryError::Encode(e.to_string()))?;
            write_atomic(&rec.thumbnail_path(), &thumb)?;
        }
        Ok(())
    }

    /// Legt ein hineingezogenes Bild ab und gibt seinen Hash zurück.
    pub fn add_asset(&self, id: &str, png: &[u8]) -> Result<String> {
        let rec = self.get(id)?;
        let hash = fnv1a_hex(png);
        let path = rec.dir.join(asset_file(id, &hash));
        if !path.exists() {
            write_atomic(&path, png)?;
        }
        Ok(hash)
    }

    /// Aufnahmen, deren Text noch nicht erkannt wurde.
    pub fn pending_ocr(&self, limit: u32) -> Result<Vec<String>> {
        Ok(index::pending_ocr(&self.lock(), limit)?)
    }

    /// Erkannten Text für die Suche ablegen.
    pub fn set_ocr_text(&self, id: &str, text: &str) -> Result<()> {
        Ok(index::set_ocr_text(&self.lock(), id, text)?)
    }

    /// Galerie: Aufnahmen nach Text, Zeitraum, Art und Stern filtern.
    pub fn query(&self, q: &CaptureQuery) -> Result<Vec<CaptureRecord>> {
        Ok(index::query(&self.lock(), &self.root, q)?)
    }

    /// Volltextsuche über den erkannten Text; leere Suche = neueste.
    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<CaptureRecord>> {
        Ok(index::search(&self.lock(), &self.root, query, limit)?)
    }

    /// Wohin die Aufnahme zuletzt automatisch exportiert wurde.
    pub fn export_path(&self, id: &str) -> Result<Option<PathBuf>> {
        Ok(index::export_path(&self.lock(), id)?.map(PathBuf::from))
    }

    pub fn set_export_path(&self, id: &str, path: &Path) -> Result<()> {
        Ok(index::set_export_path(
            &self.lock(),
            id,
            &path.to_string_lossy(),
        )?)
    }

    /// Löscht Aufnahmen, die älter als `before_ms` sind und keinen Stern
    /// haben. Gibt die Anzahl zurück.
    pub fn purge_older_than(&self, before_ms: i64) -> Result<usize> {
        let ids = index::expired(&self.lock(), before_ms)?;
        for id in &ids {
            self.delete(id)?;
        }
        Ok(ids.len())
    }

    pub fn set_starred(&self, id: &str, starred: bool) -> Result<()> {
        Ok(index::set_starred(&self.lock(), id, starred)?)
    }

    /// Löscht alle Dateien der Aufnahme und den Index-Eintrag.
    pub fn delete(&self, id: &str) -> Result<()> {
        let rec = self.get(id)?;
        for entry in fs::read_dir(&rec.dir)? {
            let entry = entry?;
            let name = entry.file_name();
            let name = name.to_string_lossy();
            let belongs = name == document_file(id) || name.starts_with(&format!("{id}_"));
            if belongs {
                fs::remove_file(entry.path())?;
            }
        }
        index::delete(&self.lock(), id)?;
        Ok(())
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Connection> {
        // Eine Panik in einem anderen Thread macht die Verbindung nicht kaputt.
        self.index.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Findet eine freie ID; reserviert sie über das Dokument.
    fn reserve_id(&self, created_at_ms: i64) -> Result<(CaptureId, PathBuf)> {
        loop {
            let seq = self.seq.fetch_add(1, Ordering::Relaxed);
            let id = CaptureId::local(created_at_ms, seq);
            let dir = month_dir(&self.root, &id);
            fs::create_dir_all(&dir)?;
            let reserve = fs::OpenOptions::new()
                .write(true)
                .create_new(true)
                .open(dir.join(document_file(id.as_str())));
            match reserve {
                Ok(_) => return Ok((id, dir)),
                Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
                Err(e) => return Err(e.into()),
            }
        }
    }
}

/// Bringt eine ältere Bibliothek auf den aktuellen Stand.
fn migrate(conn: &Connection, root: &Path) -> Result<()> {
    let version = index::version(conn)?;
    if version < 2 {
        // v1: `<monat>/<id>/{doc.json, original.png, thumb.webp, assets/}`
        for (id, rel_dir) in index::all_dirs(conn)? {
            let old = root.join(&rel_dir);
            let Some(month) = Path::new(&rel_dir).parent() else {
                continue;
            };
            if old.is_dir() {
                let new = root.join(month);
                move_if_exists(&old.join("doc.json"), &new.join(document_file(&id)))?;
                move_if_exists(&old.join("original.png"), &new.join(original_file(&id)))?;
                move_if_exists(&old.join("thumb.webp"), &new.join(thumbnail_file(&id)))?;
                let assets = old.join("assets");
                if assets.is_dir() {
                    for a in fs::read_dir(&assets)? {
                        let a = a?.path();
                        if let Some(stem) = a.file_stem().and_then(|s| s.to_str()) {
                            move_if_exists(&a, &new.join(asset_file(&id, stem)))?;
                        }
                    }
                    let _ = fs::remove_dir(&assets);
                }
                let _ = fs::remove_dir(&old);
            }
            index::set_dir(conn, &id, &month.to_string_lossy())?;
        }
    }
    if version != index::SCHEMA_VERSION {
        index::set_version(conn, index::SCHEMA_VERSION)?;
    }
    Ok(())
}

fn move_if_exists(from: &Path, to: &Path) -> io::Result<()> {
    if from.exists() {
        fs::rename(from, to)?;
    }
    Ok(())
}

/// Schreibt erst in eine temporäre Datei und benennt dann um – so liegt nie
/// eine halbe Datei in der Bibliothek.
fn write_atomic(path: &Path, data: &[u8]) -> io::Result<()> {
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    fs::write(&tmp, data)?;
    fs::rename(&tmp, path)
}

/// Kurzer, stabiler Inhalts-Hash für Dateinamen (kein Sicherheitszweck).
fn fnv1a_hex(data: &[u8]) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in data {
        h ^= *b as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
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

    fn files(dir: &Path) -> Vec<String> {
        let mut v: Vec<String> = fs::read_dir(dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        v.sort();
        v
    }

    #[test]
    fn speichern_legt_dateien_flach_im_monatsordner_an() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::open(tmp.path()).unwrap();
        let rec = lib.save_capture(&capture(1_792_172_712_083)).unwrap();

        let id = &rec.id;
        assert_eq!(
            files(&rec.dir),
            vec![
                format!("{id}.json"),
                format!("{id}_original.png"),
                format!("{id}_thumb.webp"),
            ]
        );
        assert!(rec.dir.ends_with("2026/10"));
        let doc = lib.load_document(id).unwrap();
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

    #[test]
    fn loeschen_entfernt_nur_eigene_dateien() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::open(tmp.path()).unwrap();
        let a = lib.save_capture(&capture(1_792_172_712_083)).unwrap();
        let b = lib.save_capture(&capture(1_792_172_712_083)).unwrap();
        lib.add_asset(&a.id, b"bild").unwrap();
        lib.delete(&a.id).unwrap();
        assert_eq!(files(&b.dir).len(), 3);
        assert!(lib.get(&a.id).is_err());
    }

    #[test]
    fn volltextsuche_findet_erkannten_text() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::open(tmp.path()).unwrap();
        let a = lib.save_capture(&capture(1_000)).unwrap();
        let b = lib.save_capture(&capture(2_000)).unwrap();
        assert_eq!(
            lib.pending_ocr(10).unwrap(),
            vec![b.id.clone(), a.id.clone()]
        );
        lib.set_ocr_text(&a.id, "Fehlermeldung: Datei nicht gefunden")
            .unwrap();
        lib.set_ocr_text(&b.id, "Rechnung März").unwrap();
        assert!(lib.pending_ocr(10).unwrap().is_empty());
        let hits = lib.search("datei gef", 10).unwrap();
        assert_eq!(hits.iter().map(|r| &r.id).collect::<Vec<_>>(), vec![&a.id]);
        // Umlaute: „marz“ findet „März“.
        assert_eq!(lib.search("marz", 10).unwrap().len(), 1);
        assert_eq!(lib.search("", 10).unwrap().len(), 2);
        lib.delete(&a.id).unwrap();
        assert!(lib.search("datei", 10).unwrap().is_empty());
    }

    #[test]
    fn aufbewahrung_verschont_sterne() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::open(tmp.path()).unwrap();
        let old = lib.save_capture(&capture(1_000)).unwrap();
        let star = lib.save_capture(&capture(1_500)).unwrap();
        let new = lib.save_capture(&capture(9_000)).unwrap();
        lib.set_starred(&star.id, true).unwrap();
        assert_eq!(lib.purge_older_than(5_000).unwrap(), 1);
        assert!(lib.get(&old.id).is_err());
        assert!(lib.get(&star.id).is_ok());
        assert!(lib.get(&new.id).is_ok());
    }

    #[test]
    fn exportpfad_merken() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::open(tmp.path()).unwrap();
        let a = lib.save_capture(&capture(1_000)).unwrap();
        assert_eq!(lib.export_path(&a.id).unwrap(), None);
        lib.set_export_path(&a.id, Path::new("/tmp/x.png")).unwrap();
        assert_eq!(
            lib.export_path(&a.id).unwrap(),
            Some(PathBuf::from("/tmp/x.png"))
        );
    }

    #[test]
    fn alte_ordner_ablage_wird_umgezogen() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();
        // Bibliothek im Format v1 nachbauen
        let old = root.join("2026/10/20261016-200122-944-0");
        fs::create_dir_all(old.join("assets")).unwrap();
        fs::write(old.join("doc.json"), b"{}").unwrap();
        fs::write(old.join("original.png"), b"png").unwrap();
        fs::write(old.join("thumb.webp"), b"webp").unwrap();
        fs::write(old.join("assets/abc.png"), b"a").unwrap();
        {
            let conn = index::open(&root.join(INDEX_FILE)).unwrap();
            conn.execute(
                "INSERT INTO captures (id, created_at, mode, width, height, scale_factor, rel_dir, starred)
                 VALUES ('20261016-200122-944-0', 1, 'region', 1, 1, 2.0,
                 '2026/10/20261016-200122-944-0', 0)",
                [],
            )
            .unwrap();
            index::set_version(&conn, 1).unwrap();
        }

        let lib = Library::open(root).unwrap();
        let rec = lib.get("20261016-200122-944-0").unwrap();
        assert!(rec.dir.ends_with("2026/10"));
        assert_eq!(
            files(&rec.dir),
            vec![
                "20261016-200122-944-0.json",
                "20261016-200122-944-0_asset_abc.png",
                "20261016-200122-944-0_original.png",
                "20261016-200122-944-0_thumb.webp",
            ]
        );
    }

    #[test]
    fn galerie_filtert_nach_zeitraum_stern_text_und_sortiert() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::open(tmp.path()).unwrap();
        let tag = 86_400_000;
        let t0 = 1_792_000_000_000;
        let a = lib.save_capture(&capture(t0)).unwrap();
        let b = lib.save_capture(&capture(t0 + tag)).unwrap();
        let c = lib.save_capture(&capture(t0 + 2 * tag)).unwrap();
        lib.set_starred(&b.id, true).unwrap();
        lib.set_ocr_text(&c.id, "Rechnung März").unwrap();
        let ids = |q: CaptureQuery| -> Vec<String> {
            lib.query(&CaptureQuery { limit: 50, ..q })
                .unwrap()
                .into_iter()
                .map(|r| r.id.to_string())
                .collect()
        };
        let id = |r: &CaptureRecord| r.id.to_string();

        assert_eq!(ids(CaptureQuery::default()), vec![id(&c), id(&b), id(&a)]);
        assert_eq!(
            ids(CaptureQuery {
                oldest_first: true,
                ..Default::default()
            }),
            vec![id(&a), id(&b), id(&c)]
        );
        assert_eq!(
            ids(CaptureQuery {
                from_ms: Some(t0 + tag),
                to_ms: Some(t0 + 2 * tag),
                ..Default::default()
            }),
            vec![id(&b)]
        );
        assert_eq!(
            ids(CaptureQuery {
                starred_only: true,
                ..Default::default()
            }),
            vec![id(&b)]
        );
        assert_eq!(
            ids(CaptureQuery {
                text: "rech".into(),
                ..Default::default()
            }),
            vec![id(&c)]
        );
        assert!(
            ids(CaptureQuery {
                mode: Some("window".into()),
                ..Default::default()
            })
            .is_empty()
        );
    }
}
