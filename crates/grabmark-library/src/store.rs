use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicU32, Ordering};
use std::time::Instant;

use grabmark_core::{Document, RgbaImage};
use rusqlite::Connection;

use crate::encode::{encode_png, encode_webp, thumbnail};
use crate::index::{self, CaptureQuery, CaptureRecord};
use crate::{
    CaptureId, INDEX_FILE, THUMBNAIL_MAX, asset_file, checkpoint_file, document_file, faces_file,
    input_file, input_log_file, microphone_file, month_dir, original_file, screen_file,
    system_audio_file, thumbnail_file, video_file, webcam_file,
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

/// Platz für eine Video-Aufnahme, die gerade läuft. Die ID ist reserviert;
/// in die Galerie kommt sie erst mit [`Library::save_video`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VideoSlot {
    pub id: String,
    pub dir: PathBuf,
    pub created_at_ms: i64,
}

impl VideoSlot {
    pub fn screen_path(&self) -> PathBuf {
        self.dir.join(screen_file(&self.id))
    }

    pub fn system_audio_path(&self) -> PathBuf {
        self.dir.join(system_audio_file(&self.id))
    }

    pub fn microphone_path(&self) -> PathBuf {
        self.dir.join(microphone_file(&self.id))
    }

    pub fn webcam_path(&self) -> PathBuf {
        self.dir.join(webcam_file(&self.id))
    }

    pub fn video_path(&self) -> PathBuf {
        self.dir.join(video_file(&self.id))
    }

    pub fn faces_path(&self) -> PathBuf {
        self.dir.join(faces_file(&self.id))
    }

    pub fn input_path(&self) -> PathBuf {
        self.dir.join(input_file(&self.id))
    }

    pub fn checkpoint_path(&self) -> PathBuf {
        self.dir.join(checkpoint_file(&self.id))
    }

    pub fn input_log_path(&self) -> PathBuf {
        self.dir.join(input_log_file(&self.id))
    }
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
        let started = Instant::now();
        let root = root.into();
        fs::create_dir_all(&root).inspect_err(|e| {
            log::error!("Bibliothek {} anlegen fehlgeschlagen: {e}", root.display())
        })?;
        let conn = index::open(&root.join(INDEX_FILE)).inspect_err(|e| {
            log::error!("Index in {} öffnen fehlgeschlagen: {e}", root.display())
        })?;
        migrate(&conn, &root)
            .inspect_err(|e| log::error!("Bibliothek migrieren fehlgeschlagen: {e}"))?;
        match index::count(&conn) {
            Ok(n) => log::info!(
                "Bibliothek {} geöffnet: {n} Aufnahmen in {} ms",
                root.display(),
                started.elapsed().as_millis()
            ),
            Err(e) => log::warn!("Bibliothek geöffnet, Anzahl unbekannt: {e}"),
        }
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
        let started = Instant::now();
        let result = self.save_capture_inner(capture);
        match &result {
            Ok(rec) => log::info!(
                "Aufnahme {} gespeichert: {}×{} ({}) in {} ms",
                rec.id,
                rec.width,
                rec.height,
                rec.mode,
                started.elapsed().as_millis()
            ),
            Err(e) => log::error!("Aufnahme speichern fehlgeschlagen: {e}"),
        }
        result
    }

    fn save_capture_inner(&self, capture: &NewCapture) -> Result<CaptureRecord> {
        let (id, dir) = self.reserve_id(capture.created_at_ms)?;
        let id = id.to_string();

        let t = Instant::now();
        let png = encode_png(&capture.image, capture.scale_factor)
            .map_err(|e| LibraryError::Encode(e.to_string()))?;
        log::debug!(
            "PNG für {id}: {} KB in {} ms",
            png.len() / 1024,
            t.elapsed().as_millis()
        );
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

        let t = Instant::now();
        let thumb = encode_webp(&thumbnail(&capture.image, THUMBNAIL_MAX))
            .map_err(|e| LibraryError::Encode(e.to_string()))?;
        log::debug!(
            "Vorschaubild für {id}: {} KB in {} ms",
            thumb.len() / 1024,
            t.elapsed().as_millis()
        );
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
            duration_ms: None,
        };
        index::insert(&self.lock(), &self.root, &record)?;
        Ok(record)
    }

    /// Reserviert eine ID für ein Video, das gleich aufgenommen wird.
    pub fn reserve_video(&self, created_at_ms: i64) -> Result<VideoSlot> {
        let (id, dir) = self.reserve_id(created_at_ms)?;
        log::debug!("Video-ID {id} reserviert");
        Ok(VideoSlot {
            id: id.to_string(),
            dir,
            created_at_ms,
        })
    }

    /// Nimmt ein fertiges Video in die Galerie auf. `poster` ist ein
    /// Standbild daraus (Vorschau, Texterkennung, Bildinhalt), `doc` das
    /// Dokument in voller Grösse (das fertige Video kann kleiner sein) mit
    /// Video-Teil und Ebenen.
    pub fn save_video(
        &self,
        slot: &VideoSlot,
        poster: &RgbaImage,
        scale_factor: f32,
        doc: Document,
    ) -> Result<CaptureRecord> {
        let started = Instant::now();
        let result = self.save_video_inner(slot, poster, scale_factor, doc);
        match &result {
            Ok(rec) => log::info!(
                "Video {} gespeichert: {}×{}, {} ms lang, in {} ms",
                rec.id,
                rec.width,
                rec.height,
                rec.duration_ms.unwrap_or(0),
                started.elapsed().as_millis()
            ),
            Err(e) => log::error!("Video {} speichern fehlgeschlagen: {e}", slot.id),
        }
        result
    }

    fn save_video_inner(
        &self,
        slot: &VideoSlot,
        poster: &RgbaImage,
        scale_factor: f32,
        doc: Document,
    ) -> Result<CaptureRecord> {
        let id = slot.id.clone();
        let duration_ms = doc.video.as_ref().map_or(0, |v| v.duration_ms) as i64;
        let frame = (
            doc.artboard.rect.width.round() as u32,
            doc.artboard.rect.height.round() as u32,
        );
        let png =
            encode_png(poster, scale_factor).map_err(|e| LibraryError::Encode(e.to_string()))?;
        write_atomic(&slot.dir.join(original_file(&id)), &png)?;
        write_atomic(
            &slot.dir.join(document_file(&id)),
            &serde_json::to_vec_pretty(&doc)?,
        )?;
        let thumb = encode_webp(&thumbnail(poster, THUMBNAIL_MAX))
            .map_err(|e| LibraryError::Encode(e.to_string()))?;
        write_atomic(&slot.dir.join(thumbnail_file(&id)), &thumb)?;
        let record = CaptureRecord {
            id,
            created_at_ms: slot.created_at_ms,
            mode: "video".into(),
            width: frame.0,
            height: frame.1,
            scale_factor,
            dir: slot.dir.clone(),
            starred: false,
            duration_ms: Some(duration_ms),
        };
        index::insert(&self.lock(), &self.root, &record)?;
        Ok(record)
    }

    /// Nach dem neuen Zusammensetzen im Editor: Standbild, Vorschaubild und
    /// Länge nachführen.
    pub fn refresh_video(&self, id: &str, poster: &RgbaImage, duration_ms: i64) -> Result<()> {
        log::debug!("Video {id}: Standbild und Länge ({duration_ms} ms) nachführen");
        let rec = self.get(id)?;
        let png = encode_png(poster, rec.scale_factor)
            .map_err(|e| LibraryError::Encode(e.to_string()))?;
        write_atomic(&rec.original_path(), &png)?;
        let thumb = encode_webp(&thumbnail(poster, THUMBNAIL_MAX))
            .map_err(|e| LibraryError::Encode(e.to_string()))?;
        write_atomic(&rec.thumbnail_path(), &thumb)?;
        Ok(index::set_duration(&self.lock(), id, duration_ms)?)
    }

    /// Verwirft ein abgebrochenes Video samt Reservierung.
    pub fn discard_video(&self, slot: &VideoSlot) {
        let removed = remove_files_of(&slot.dir, &slot.id);
        log::info!("Video {} verworfen ({removed} Dateien)", slot.id);
    }

    /// Videos, deren Aufnahme nie fertig wurde (Absturz, Beenden): leerer
    /// Platzhalter `<id>.json` ohne Index-Eintrag, aber mit Bildschirm- oder
    /// Webcam-Spur. Platzhalter ohne Spur werden dabei still entfernt. Nur
    /// beim Start aufrufen – sonst zählt eine laufende Aufnahme mit.
    pub fn unfinished_videos(&self) -> Vec<VideoSlot> {
        let started = Instant::now();
        let mut found = Vec::new();
        let mut placeholders = 0;
        let years = match fs::read_dir(&self.root) {
            Ok(y) => y,
            Err(e) => {
                log::warn!("Bibliothek {} nicht lesbar: {e}", self.root.display());
                return found;
            }
        };
        for year in years.flatten().filter(|e| e.path().is_dir()) {
            let months = match fs::read_dir(year.path()) {
                Ok(m) => m,
                Err(e) => {
                    log::warn!("{} nicht lesbar: {e}", year.path().display());
                    continue;
                }
            };
            for month in months.flatten().filter(|e| e.path().is_dir()) {
                let dir = month.path();
                let entries = match fs::read_dir(&dir) {
                    Ok(e) => e,
                    Err(e) => {
                        log::warn!("{} nicht lesbar: {e}", dir.display());
                        continue;
                    }
                };
                for entry in entries.flatten() {
                    let name = entry.file_name().to_string_lossy().into_owned();
                    let Some(id) = name.strip_suffix(".json").filter(|id| !id.contains('_')) else {
                        continue;
                    };
                    let Ok(meta) = entry.metadata() else { continue };
                    if meta.len() > 0 || self.get(id).is_ok() {
                        continue;
                    }
                    let created_at_ms = meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map_or(0, |d| d.as_millis() as i64);
                    let slot = VideoSlot {
                        id: id.to_string(),
                        dir: dir.clone(),
                        created_at_ms,
                    };
                    let has_track = [slot.screen_path(), slot.webcam_path()]
                        .iter()
                        .any(|p| fs::metadata(p).is_ok_and(|m| m.len() > 0));
                    if has_track {
                        log::warn!("Unfertiges Video gefunden: {}", slot.id);
                        found.push(slot);
                    } else {
                        placeholders += 1;
                        remove_files_of(&slot.dir, &slot.id);
                    }
                }
            }
        }
        found.sort_by(|a, b| a.id.cmp(&b.id));
        log::debug!(
            "Unfertige Videos gesucht in {} ms: {} gefunden, {placeholders} leere Platzhalter entfernt",
            started.elapsed().as_millis(),
            found.len()
        );
        found
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

    /// Merkt sich, aus welchem Dokument das fertige Video entstand.
    pub fn mark_video_current(&self, id: &str, doc: &Document) -> Result<()> {
        let rec = self.get(id)?;
        fs::write(rec.video_stamp_path(), document_stamp(doc)?)?;
        Ok(())
    }

    /// Passt das fertige Video noch zum gespeicherten Dokument? Sonst muss
    /// es vor dem Kopieren oder Sichern neu zusammengesetzt werden.
    pub fn video_is_current(&self, id: &str) -> Result<bool> {
        let rec = self.get(id)?;
        let stamp = match fs::read_to_string(rec.video_stamp_path()) {
            Ok(s) => s,
            Err(e) => {
                if e.kind() != io::ErrorKind::NotFound {
                    log::warn!("Video {id}: Fingerabdruck nicht lesbar: {e}");
                }
                return Ok(false);
            }
        };
        Ok(stamp == document_stamp(&self.load_document(id)?)?)
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
        let started = Instant::now();
        let rec = self.get(id)?;
        let json = serde_json::to_vec_pretty(doc)?;
        write_atomic(&rec.document_path(), &json)
            .inspect_err(|e| log::error!("Dokument {id} speichern fehlgeschlagen: {e}"))?;
        if let Some(preview) = preview {
            let thumb = encode_webp(&thumbnail(preview, THUMBNAIL_MAX))
                .map_err(|e| LibraryError::Encode(e.to_string()))?;
            write_atomic(&rec.thumbnail_path(), &thumb)?;
        }
        log::debug!(
            "Dokument {id} gespeichert: {} KB, {} Ebenen, Vorschau={} in {} ms",
            json.len() / 1024,
            doc.layers.len(),
            preview.is_some(),
            started.elapsed().as_millis()
        );
        Ok(())
    }

    /// Legt ein hineingezogenes Bild ab und gibt seinen Hash zurück.
    pub fn add_asset(&self, id: &str, png: &[u8]) -> Result<String> {
        let rec = self.get(id)?;
        let hash = fnv1a_hex(png);
        let path = rec.dir.join(asset_file(id, &hash));
        if !path.exists() {
            write_atomic(&path, png)?;
            log::debug!("Bild {hash} zu {id} abgelegt ({} KB)", png.len() / 1024);
        }
        Ok(hash)
    }

    /// Aufnahmen, deren Text noch nicht erkannt wurde.
    pub fn pending_ocr(&self, limit: u32) -> Result<Vec<String>> {
        Ok(index::pending_ocr(&self.lock(), limit)?)
    }

    /// Erkannten Text für die Suche ablegen.
    pub fn set_ocr_text(&self, id: &str, text: &str) -> Result<()> {
        // Nur die Länge – erkannter Text gehört nicht ins Protokoll.
        log::debug!("Text für {id} abgelegt ({} Zeichen)", text.chars().count());
        Ok(index::set_ocr_text(&self.lock(), id, text)?)
    }

    /// Aufnahmen ohne Bildinhalt-Vektor des Modells `model` (ADR 0010).
    pub fn pending_embeddings(&self, model: &str, limit: u32) -> Result<Vec<String>> {
        Ok(index::pending_embeddings(&self.lock(), model, limit)?)
    }

    /// Vektor ablegen; ein leerer Vektor heisst „nicht berechenbar“. Die
    /// Bibliothek kennt das Modell nicht, nur seine Kennung.
    pub fn set_embedding(&self, id: &str, model: &str, vector: &[u8]) -> Result<()> {
        if vector.is_empty() {
            log::debug!("Bildinhalt für {id} ({model}) nicht berechenbar – leer abgelegt");
        }
        Ok(index::set_embedding(&self.lock(), id, model, vector)?)
    }

    /// Alle Vektoren des Modells als (ID, Bytes).
    pub fn embeddings(&self, model: &str) -> Result<Vec<(String, Vec<u8>)>> {
        Ok(index::embeddings(&self.lock(), model)?)
    }

    /// Vektoren ohne aktuelle Stichwörter (`version` = Begriffsliste).
    pub fn pending_tags(
        &self,
        model: &str,
        version: &str,
        limit: u32,
    ) -> Result<Vec<(String, Vec<u8>)>> {
        Ok(index::pending_tags(&self.lock(), model, version, limit)?)
    }

    /// Stichwörter (Schlüssel, Wert) einer Aufnahme ersetzen.
    pub fn set_tags(&self, id: &str, version: &str, tags: &[(&str, f32)]) -> Result<()> {
        log::debug!("{} Stichwörter für {id} (Liste {version})", tags.len());
        Ok(index::set_tags(&self.lock(), id, version, tags)?)
    }

    /// Stichwörter je Aufnahme, bestes zuerst; Aufnahmen ohne fehlen.
    pub fn tags(&self, ids: &[String]) -> Result<std::collections::HashMap<String, Vec<String>>> {
        Ok(index::tags(&self.lock(), ids)?)
    }

    /// (ID, Stichwort) aller Aufnahmen, die zu den Filtern passen (ohne
    /// Begrenzung) – für die Anzahl je Schnellfilter.
    pub fn tagged(&self, q: &CaptureQuery) -> Result<Vec<(String, String)>> {
        Ok(index::tagged(&self.lock(), q)?)
    }

    /// IDs, deren erkannter Text zur Suche passt.
    pub fn text_matches(&self, text: &str) -> Result<Vec<String>> {
        Ok(index::text_matches(&self.lock(), text)?)
    }

    /// (erledigt, alle) Aufnahmen für das Modell.
    pub fn embedding_progress(&self, model: &str) -> Result<(u32, u32)> {
        Ok(index::embedding_progress(&self.lock(), model)?)
    }

    /// Galerie: Aufnahmen nach Text, Zeitraum, Art und Stern filtern.
    pub fn query(&self, q: &CaptureQuery) -> Result<Vec<CaptureRecord>> {
        let started = Instant::now();
        let found = index::query(&self.lock(), &self.root, q)
            .inspect_err(|e| log::error!("Galerie abfragen fehlgeschlagen: {e}"))?;
        log::debug!(
            "Galerie: {} Aufnahmen in {} ms",
            found.len(),
            started.elapsed().as_millis()
        );
        Ok(found)
    }

    /// Volltextsuche über den erkannten Text; leere Suche = neueste.
    pub fn search(&self, query: &str, limit: u32) -> Result<Vec<CaptureRecord>> {
        let started = Instant::now();
        // Den Suchtext selbst nicht protokollieren.
        let found = index::search(&self.lock(), &self.root, query, limit)
            .inspect_err(|e| log::error!("Suche fehlgeschlagen: {e}"))?;
        log::debug!(
            "Suche: {} Treffer in {} ms",
            found.len(),
            started.elapsed().as_millis()
        );
        Ok(found)
    }

    /// Wohin die Aufnahme zuletzt automatisch exportiert wurde.
    pub fn export_path(&self, id: &str) -> Result<Option<PathBuf>> {
        Ok(index::export_path(&self.lock(), id)?.map(PathBuf::from))
    }

    pub fn set_export_path(&self, id: &str, path: &Path) -> Result<()> {
        log::debug!("Aufnahme {id} exportiert nach {}", path.display());
        Ok(index::set_export_path(
            &self.lock(),
            id,
            &path.to_string_lossy(),
        )?)
    }

    /// Löscht Aufnahmen, die älter als `before_ms` sind und keinen Stern
    /// haben. Gibt die Anzahl zurück.
    pub fn purge_older_than(&self, before_ms: i64) -> Result<usize> {
        let started = Instant::now();
        let ids = index::expired(&self.lock(), before_ms)?;
        for id in &ids {
            self.delete(id)
                .inspect_err(|e| log::error!("Aufräumen bei {id} abgebrochen: {e}"))?;
        }
        if !ids.is_empty() {
            log::info!(
                "Aufbewahrung: {} alte Aufnahmen gelöscht in {} ms",
                ids.len(),
                started.elapsed().as_millis()
            );
        }
        Ok(ids.len())
    }

    pub fn set_starred(&self, id: &str, starred: bool) -> Result<()> {
        log::debug!("Aufnahme {id}: Stern={starred}");
        Ok(index::set_starred(&self.lock(), id, starred)?)
    }

    /// Löscht alle Dateien der Aufnahme und den Index-Eintrag.
    pub fn delete(&self, id: &str) -> Result<()> {
        let started = Instant::now();
        let rec = self.get(id)?;
        let mut removed = 0;
        let result = (|| -> Result<()> {
            for entry in fs::read_dir(&rec.dir)? {
                let entry = entry?;
                if belongs_to(&entry.file_name().to_string_lossy(), id) {
                    fs::remove_file(entry.path())?;
                    removed += 1;
                }
            }
            index::delete(&self.lock(), id)?;
            Ok(())
        })();
        match &result {
            Ok(()) => log::info!(
                "Aufnahme {id} gelöscht ({removed} Dateien) in {} ms",
                started.elapsed().as_millis()
            ),
            Err(e) => {
                log::error!("Aufnahme {id} löschen fehlgeschlagen nach {removed} Dateien: {e}")
            }
        }
        result
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
        let started = Instant::now();
        let dirs = index::all_dirs(conn)?;
        log::info!(
            "Bibliothek migrieren v{version} → v2: {} Aufnahmen in flache Monatsordner",
            dirs.len()
        );
        // v1: `<monat>/<id>/{doc.json, original.png, thumb.webp, assets/}`
        for (id, rel_dir) in dirs {
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
                    remove_dir_logged(&assets);
                }
                remove_dir_logged(&old);
            }
            index::set_dir(conn, &id, &month.to_string_lossy())?;
        }
        log::info!(
            "Migration auf v2 fertig in {} ms",
            started.elapsed().as_millis()
        );
    }
    if version != index::SCHEMA_VERSION {
        log::info!("Index-Schema v{version} → v{}", index::SCHEMA_VERSION);
        index::set_version(conn, index::SCHEMA_VERSION)?;
    }
    Ok(())
}

/// Gehört die Datei zur Aufnahme `id`?
/// Fingerabdruck eines Dokuments (FNV-1a über das JSON) – stabil über
/// Programmversionen, anders als `DefaultHasher`.
fn document_stamp(doc: &Document) -> Result<String> {
    let json = serde_json::to_vec(doc)?;
    let hash = json.iter().fold(0xcbf2_9ce4_8422_2325_u64, |h, &b| {
        (h ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3)
    });
    Ok(format!("{hash:016x}"))
}

fn belongs_to(name: &str, id: &str) -> bool {
    name == document_file(id) || name.starts_with(&format!("{id}_"))
}

/// Löscht alle Dateien der Aufnahme, so gut es geht. Gibt die Anzahl zurück.
fn remove_files_of(dir: &Path, id: &str) -> usize {
    let entries = match fs::read_dir(dir) {
        Ok(e) => e,
        Err(e) => {
            log::warn!("{} nicht lesbar: {e}", dir.display());
            return 0;
        }
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        if belongs_to(&entry.file_name().to_string_lossy(), id) {
            match fs::remove_file(entry.path()) {
                Ok(()) => removed += 1,
                Err(e) => log::warn!("{} löschen fehlgeschlagen: {e}", entry.path().display()),
            }
        }
    }
    removed
}

/// Leeren Ordner der alten Ablage entfernen; bleibt er stehen, ist das kein Fehler.
fn remove_dir_logged(dir: &Path) {
    if let Err(e) = fs::remove_dir(dir) {
        log::debug!("Alter Ordner {} bleibt: {e}", dir.display());
    }
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
    use grabmark_core::VideoDoc;

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
    fn video_kommt_mit_dauer_in_die_galerie_und_wird_ganz_geloescht() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::open(tmp.path()).unwrap();
        let slot = lib.reserve_video(1_792_172_712_083).unwrap();
        fs::write(slot.screen_path(), b"mp4").unwrap();
        fs::write(slot.microphone_path(), b"m4a").unwrap();
        fs::write(slot.webcam_path(), b"mp4").unwrap();
        fs::write(slot.video_path(), b"mp4").unwrap();
        let poster = RgbaImage::filled(320, 200, [0, 0, 0, 255]);
        let video = VideoDoc {
            duration_ms: 12_345,
            fps: 30,
            webcam: None,
            ..VideoDoc::default()
        };
        let mut doc = Document::from_capture_scaled(640, 400, 2.0);
        doc.video = Some(video.clone());
        let rec = lib.save_video(&slot, &poster, 2.0, doc).unwrap();

        assert!(rec.is_video());
        assert_eq!(rec.video_path(), slot.video_path());
        assert_eq!(rec.webcam_path(), slot.webcam_path());
        assert_eq!(lib.get(&rec.id).unwrap().duration_ms, Some(12_345));
        assert_eq!(lib.load_document(&rec.id).unwrap().video, Some(video));

        lib.delete(&rec.id).unwrap();
        assert!(files(&slot.dir).is_empty());
    }

    #[test]
    fn abgebrochene_aufnahmen_werden_gefunden_leere_platzhalter_entfernt() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::open(tmp.path()).unwrap();
        // Abgestürzt mitten in der Aufnahme: Platzhalter + Bildschirm-Spur.
        let crashed = lib.reserve_video(1_792_172_712_083).unwrap();
        fs::write(crashed.screen_path(), b"mp4").unwrap();
        fs::write(crashed.checkpoint_path(), b"{}").unwrap();
        // Abgestürzt vor dem ersten Bild: nur der Platzhalter.
        let empty = lib.reserve_video(1_792_172_712_084).unwrap();
        // Fertig gespeichert: kein Kandidat.
        let done = lib.reserve_video(1_792_172_712_085).unwrap();
        fs::write(done.video_path(), b"mp4").unwrap();
        let mut doc = Document::from_capture_scaled(640, 400, 2.0);
        doc.video = Some(VideoDoc::default());
        let poster = RgbaImage::filled(320, 200, [0, 0, 0, 255]);
        lib.save_video(&done, &poster, 2.0, doc).unwrap();
        // Ein Bild im selben Ordner bleibt unberührt.
        let image = lib.save_capture(&capture(1_792_172_712_086)).unwrap();

        let found = lib.unfinished_videos();
        assert_eq!(
            found.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
            [crashed.id.as_str()]
        );
        assert_eq!(found[0].dir, crashed.dir);
        assert!(found[0].created_at_ms > 0);
        assert!(!empty.dir.join(document_file(&empty.id)).exists());
        assert!(lib.get(&done.id).is_ok());
        assert!(lib.get(&image.id).is_ok());
    }

    #[test]
    fn fertiges_video_ist_nach_aenderung_veraltet() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::open(tmp.path()).unwrap();
        let slot = lib.reserve_video(1_792_172_712_083).unwrap();
        fs::write(slot.video_path(), b"mp4").unwrap();
        let mut doc = Document::from_capture_scaled(640, 400, 2.0);
        doc.video = Some(VideoDoc::default());
        let poster = RgbaImage::filled(320, 200, [0, 0, 0, 255]);
        let rec = lib.save_video(&slot, &poster, 2.0, doc).unwrap();

        // Ohne Merker weiss niemand, woraus das Video entstand.
        assert!(!lib.video_is_current(&rec.id).unwrap());
        let doc = lib.load_document(&rec.id).unwrap();
        lib.mark_video_current(&rec.id, &doc).unwrap();
        assert!(lib.video_is_current(&rec.id).unwrap());

        let mut changed = doc.clone();
        changed.video.as_mut().unwrap().fps = 60;
        lib.save_document(&rec.id, &changed, None).unwrap();
        assert!(!lib.video_is_current(&rec.id).unwrap());

        lib.delete(&rec.id).unwrap();
        assert!(files(&slot.dir).is_empty());
    }

    #[test]
    fn abgebrochenes_video_hinterlaesst_nichts() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::open(tmp.path()).unwrap();
        let slot = lib.reserve_video(1_000).unwrap();
        fs::write(slot.screen_path(), b"mp4").unwrap();
        lib.discard_video(&slot);
        assert!(files(&slot.dir).is_empty());
        assert!(lib.recent(10).unwrap().is_empty());
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
    fn vektoren_je_modell_mit_fortschritt_und_loeschen() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::open(tmp.path()).unwrap();
        let a = lib.save_capture(&capture(1_000)).unwrap();
        let b = lib.save_capture(&capture(2_000)).unwrap();
        assert_eq!(
            lib.pending_embeddings("m1", 10).unwrap(),
            vec![b.id.clone(), a.id.clone()]
        );
        assert_eq!(lib.embedding_progress("m1").unwrap(), (0, 2));

        lib.set_embedding(&b.id, "m1", &[1, 2, 3, 4]).unwrap();
        lib.set_embedding(&a.id, "m1", &[]).unwrap(); // nicht berechenbar
        assert!(lib.pending_embeddings("m1", 10).unwrap().is_empty());
        assert_eq!(lib.embedding_progress("m1").unwrap(), (2, 2));
        assert_eq!(
            lib.embeddings("m1").unwrap(),
            vec![(b.id.clone(), vec![1, 2, 3, 4])]
        );

        // Neues Modell: alles neu berechnen; der Eintrag wird ersetzt.
        assert_eq!(lib.pending_embeddings("m2", 10).unwrap().len(), 2);
        lib.set_embedding(&b.id, "m2", &[9]).unwrap();
        assert!(lib.embeddings("m1").unwrap().is_empty());

        // Gelöschte Aufnahmen hinterlassen keinen Vektor, auch nachträglich nicht.
        lib.delete(&b.id).unwrap();
        lib.set_embedding(&b.id, "m2", &[9]).unwrap();
        assert!(lib.embeddings("m2").unwrap().is_empty());
        assert_eq!(lib.embedding_progress("m2").unwrap(), (0, 1));
    }

    #[test]
    fn stichwoerter_und_suche_nach_bildinhalt() {
        let tmp = tempfile::tempdir().unwrap();
        let lib = Library::open(tmp.path()).unwrap();
        let wald = lib.save_capture(&capture(1_000)).unwrap();
        let hund = lib.save_capture(&capture(2_000)).unwrap();
        let text = lib.save_capture(&capture(3_000)).unwrap();
        for r in [&wald, &hund] {
            lib.set_embedding(&r.id, "m", &[1]).unwrap();
        }
        lib.set_ocr_text(&text.id, "Der Wald im Herbst").unwrap();

        let pending = lib.pending_tags("m", "v1", 10).unwrap();
        assert_eq!(pending.len(), 2);
        lib.set_tags(&wald.id, "v1", &[("forest", -2.0), ("snow", -3.0)])
            .unwrap();
        lib.set_tags(&hund.id, "v1", &[("dog", -1.0)]).unwrap();
        assert!(lib.pending_tags("m", "v1", 10).unwrap().is_empty());
        assert_eq!(lib.pending_tags("m", "v2", 10).unwrap().len(), 2);

        let tags = lib.tags(&[wald.id.clone(), text.id.clone()]).unwrap();
        assert_eq!(tags[&wald.id], vec!["forest", "snow"]);
        assert!(!tags.contains_key(&text.id));
        assert_eq!(lib.text_matches("wald").unwrap(), vec![text.id.clone()]);

        let ids = |q: CaptureQuery| -> Vec<String> {
            lib.query(&CaptureQuery { limit: 50, ..q })
                .unwrap()
                .into_iter()
                .map(|r| r.id)
                .collect()
        };
        // Text ODER Bildinhalt ODER Stichwort
        assert_eq!(
            ids(CaptureQuery {
                text: "Wald".into(),
                content_ids: vec![hund.id.clone()],
                text_tags: vec!["forest".into()],
                ..Default::default()
            }),
            vec![text.id.clone(), hund.id.clone(), wald.id.clone()]
        );
        // Schnellfilter: nur mit Stichwort aus der Gruppe
        assert_eq!(
            ids(CaptureQuery {
                any_tags: vec!["dog".into(), "cat".into()],
                ..Default::default()
            }),
            vec![hund.id.clone()]
        );

        // Stichwörter der gefilterten Aufnahmen (ohne Begrenzung)
        let mut pairs = lib
            .tagged(&CaptureQuery {
                from_ms: Some(1_500),
                ..Default::default()
            })
            .unwrap();
        pairs.sort();
        assert_eq!(pairs, vec![(hund.id.clone(), "dog".to_string())]);

        // Neuer Vektor macht die Stichwörter ungültig; Löschen räumt auf.
        lib.set_embedding(&wald.id, "m", &[2]).unwrap();
        assert_eq!(lib.pending_tags("m", "v1", 10).unwrap().len(), 1);
        lib.delete(&hund.id).unwrap();
        assert!(lib.tags(std::slice::from_ref(&hund.id)).unwrap().is_empty());
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
