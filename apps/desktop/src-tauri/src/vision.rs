//! Bildinhalt (ADR 0010): im Hintergrund für jede Aufnahme einen Vektor und
//! Stichwörter berechnen; in der Galerie danach suchen. Die Modelle werden
//! erst bei Bedarf geladen und nach einigen Minuten ohne Nutzung freigegeben.

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::mpsc::{self, Sender};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use grabmark_library::Library;
use grabmark_vision::{
    Embedding, ImageEncoder, ImageVectors, MODEL_ID, Tagger, TextEncoder, tags_version,
};
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Emitter, Manager};

use crate::capture::CaptureState;
use crate::settings;

/// Rechenkerne für ein Bild – die Erkennung soll nichts anderes bremsen.
const THREADS: usize = 2;
/// So lange bleiben die Modelle nach der letzten Nutzung im Speicher.
const KEEP_LOADED: Duration = Duration::from_secs(300);
/// Pause zwischen zwei Bildern beim Nachholen älterer Aufnahmen.
const PAUSE: Duration = Duration::from_millis(20);
/// Höchstens so viele Treffer nach Bildinhalt.
const MAX_HITS: usize = 1000;

/// Alle Bildvektoren (ID, Vektor).
type Vectors = Arc<Vec<(String, Embedding)>>;

pub struct VisionState {
    /// Ordner mit den Modelldateien; `None` = nicht mitgeliefert.
    model_dir: Option<PathBuf>,
    /// Zwischenspeicher für die Vektoren der Begriffe.
    cache_dir: Option<PathBuf>,
    wakeup: Mutex<Option<Sender<()>>>,
    tagger: Mutex<Option<Arc<Tagger>>>,
    text: Mutex<Option<(Arc<TextEncoder>, Instant)>>,
    /// Alle Bildvektoren für die Suche; neu laden, wenn `generation` wächst.
    vectors: Mutex<(u64, Vectors)>,
    generation: AtomicU64,
}

impl VisionState {
    pub fn new(app: &AppHandle) -> Self {
        let model_dir = find_model_dir(app);
        match &model_dir {
            Some(dir) => log::info!("Bildmodell: {}", dir.display()),
            None => log::warn!("Bildmodell fehlt – Suche nach Bildinhalt aus"),
        }
        Self {
            model_dir,
            cache_dir: Some(crate::paths::get().cache.join("vision")),
            wakeup: Mutex::new(None),
            tagger: Mutex::new(None),
            text: Mutex::new(None),
            vectors: Mutex::new((0, Arc::new(Vec::new()))),
            generation: AtomicU64::new(1),
        }
    }

    /// Textmodell, bei Bedarf geladen.
    fn text_encoder(&self, dir: &Path) -> Result<Arc<TextEncoder>, String> {
        let mut text = lock(&self.text);
        if let Some((enc, used)) = text.as_mut() {
            *used = Instant::now();
            return Ok(enc.clone());
        }
        let start = Instant::now();
        let enc = Arc::new(TextEncoder::load(dir, THREADS).map_err(|e| e.to_string())?);
        log::info!("Textmodell laden: {:?}", start.elapsed());
        *text = Some((enc.clone(), Instant::now()));
        Ok(enc)
    }

    /// Begriffe für Stichwörter und Suche: aus dem Zwischenspeicher oder
    /// einmal mit dem Textmodell berechnet.
    fn tagger(&self, dir: &Path) -> Result<Arc<Tagger>, String> {
        if let Some(t) = lock(&self.tagger).as_ref() {
            return Ok(t.clone());
        }
        let file = self.cache_dir.as_ref().map(|d| {
            let name: String = tags_version()
                .chars()
                .map(|c| if c.is_ascii_alphanumeric() { c } else { '-' })
                .collect();
            d.join(format!("terms-{name}.bin"))
        });
        let cached = file
            .as_ref()
            // Fehlt beim ersten Start oder nach neuer Begriffsliste – normal.
            .and_then(|f| {
                std::fs::read(f)
                    .inspect_err(|e| log::debug!("Begriffe-Zwischenspeicher: {e}"))
                    .ok()
            })
            .and_then(|b| {
                Tagger::from_bytes(&b)
                    .inspect_err(|e| log::warn!("Begriffe-Zwischenspeicher unlesbar: {e}"))
                    .ok()
            });
        let tagger = match cached {
            Some(t) => {
                log::debug!("Begriffe aus dem Zwischenspeicher");
                t
            }
            None => {
                let start = Instant::now();
                let t = Tagger::build(&*self.text_encoder(dir)?).map_err(|e| e.to_string())?;
                log::info!("Begriffe berechnen: {:?}", start.elapsed());
                if let Some(f) = &file {
                    if let Some(Err(e)) = f.parent().map(std::fs::create_dir_all) {
                        log::warn!("Begriffe zwischenspeichern: {e}");
                    }
                    if let Err(e) = std::fs::write(f, t.to_bytes()) {
                        log::warn!("Begriffe zwischenspeichern: {e}");
                    }
                }
                t
            }
        };
        let tagger = Arc::new(tagger);
        *lock(&self.tagger) = Some(tagger.clone());
        Ok(tagger)
    }

    fn vectors(&self, library: &Library) -> Result<Vectors, String> {
        let generation = self.generation.load(Ordering::Acquire);
        let mut cache = lock(&self.vectors);
        if cache.0 != generation {
            let start = Instant::now();
            let mut broken = 0;
            let all: Vec<(String, Embedding)> = library
                .embeddings(MODEL_ID)
                .map_err(|e| e.to_string())?
                .into_iter()
                .filter_map(|(id, b)| match ImageVectors::from_bytes(&b) {
                    Ok(v) => Some((id, v.whole)),
                    Err(_) => {
                        broken += 1;
                        None
                    }
                })
                .collect();
            if broken > 0 {
                log::warn!("Bildvektoren: {broken} unlesbar");
            }
            log::debug!(
                "Bildvektoren geladen: {} in {:?}",
                all.len(),
                start.elapsed()
            );
            *cache = (generation, Arc::new(all));
        }
        Ok(cache.1.clone())
    }

    /// Textmodell und Bildvektoren freigeben, wenn sie länger nicht gebraucht
    /// wurden.
    fn release_idle(&self) {
        let mut text = lock(&self.text);
        if text
            .as_ref()
            .is_some_and(|(_, used)| used.elapsed() > KEEP_LOADED)
        {
            *text = None;
            *lock(&self.vectors) = (0, Arc::new(Vec::new()));
            log::info!("Textmodell freigegeben");
        }
    }
}

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Im App-Paket unter `models/siglip2`; beim Entwickeln im Workspace.
fn find_model_dir(app: &AppHandle) -> Option<PathBuf> {
    let bundled = app
        .path()
        .resource_dir()
        .ok()
        .map(|d| d.join("models/siglip2"));
    let dev = cfg!(debug_assertions)
        .then(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../models/siglip2"));
    [bundled, dev]
        .into_iter()
        .flatten()
        .find(|d| d.join(grabmark_vision::VISION_FILE).is_file())
}

/// Hintergrund-Thread starten.
pub fn start(app: &AppHandle) {
    let (tx, rx) = mpsc::channel::<()>();
    let state = app.state::<VisionState>();
    *lock(&state.wakeup) = Some(tx);
    let Some(dir) = state.model_dir.clone() else {
        return;
    };
    let app = app.clone();
    std::thread::spawn(move || {
        log::debug!("Bildinhalt: Hintergrund-Thread läuft");
        let mut encoder: Option<(ImageEncoder, Instant)> = None;
        loop {
            if settings::get(&app).content_search {
                index_pending(&app, &dir, &mut encoder);
            } else if encoder.take().is_some() {
                log::info!("Bildmodell freigegeben (Erkennung aus)");
            }
            // Auf neue Aufnahmen warten; zwischendurch Unbenutztes freigeben.
            match rx.recv_timeout(Duration::from_secs(60)) {
                Ok(()) => {
                    // Mehrere Aufnahmen kurz hintereinander zusammenfassen.
                    std::thread::sleep(Duration::from_secs(2));
                    while rx.try_recv().is_ok() {}
                }
                Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            if encoder
                .as_ref()
                .is_some_and(|(_, used)| used.elapsed() > KEEP_LOADED)
            {
                encoder = None;
                log::info!("Bildmodell freigegeben");
            }
            app.state::<VisionState>().release_idle();
        }
    });
}

/// Neue Aufnahme oder geänderte Einstellung: nachsehen, ob es Arbeit gibt.
pub fn wake(app: &AppHandle) {
    if let Some(tx) = lock(&app.state::<VisionState>().wakeup).as_ref()
        && let Err(e) = tx.send(())
    {
        log::debug!("Bildinhalt wecken: {e}");
    }
}

fn index_pending(app: &AppHandle, dir: &Path, encoder: &mut Option<(ImageEncoder, Instant)>) {
    let library = app.state::<CaptureState>().library.clone();
    let state = app.state::<VisionState>();
    let mut changed = false;
    let started = Instant::now();
    let (mut done, mut failed) = (0u32, 0u32);
    loop {
        if !settings::get(app).content_search {
            break;
        }
        let ids = match library.pending_embeddings(MODEL_ID, 10) {
            Ok(ids) if !ids.is_empty() => ids,
            Ok(_) => break,
            Err(e) => {
                log::error!("Bildinhalt: {e}");
                break;
            }
        };
        if encoder.is_none() {
            let start = Instant::now();
            match ImageEncoder::load(dir, THREADS) {
                Ok(e) => *encoder = Some((e, Instant::now())),
                Err(e) => {
                    log::error!("Bildmodell: {e}");
                    return;
                }
            }
            crate::services::timing(app, format!("Bildmodell laden: {:?}", start.elapsed()));
        }
        log::debug!("Bildinhalt: {} Aufnahmen in diesem Durchgang", ids.len());
        let Some((enc, used)) = encoder.as_mut() else {
            return;
        };
        for id in ids {
            let one = Instant::now();
            let vector = match embed(&library, enc, &id) {
                Ok(v) => {
                    log::debug!("Bildinhalt {id}: {:?}", one.elapsed());
                    done += 1;
                    v
                }
                Err(e) => {
                    // Leerer Vektor: nicht immer wieder versuchen.
                    log::warn!("Bildinhalt {id}: {e}");
                    failed += 1;
                    Vec::new()
                }
            };
            if let Err(e) = library.set_embedding(&id, MODEL_ID, &vector) {
                log::error!("Bildinhalt {id}: {e}");
                return;
            }
            changed = true;
            *used = Instant::now();
            std::thread::sleep(PAUSE);
        }
        progress(app, &library);
    }
    if changed {
        state.generation.fetch_add(1, Ordering::Release);
    }
    if done > 0 || failed > 0 {
        log::info!(
            "Bildinhalt: {done} Aufnahmen erkannt, {failed} fehlgeschlagen ({:.1} s)",
            started.elapsed().as_secs_f64()
        );
    }
    tag_pending(app, &library, dir);
}

/// Stichwörter für Vektoren ohne aktuelle Begriffsliste.
fn tag_pending(app: &AppHandle, library: &Library, dir: &Path) {
    let version = tags_version();
    let mut tagged = 0;
    let started = Instant::now();
    loop {
        let pending = match library.pending_tags(MODEL_ID, &version, 100) {
            Ok(p) if !p.is_empty() => p,
            Ok(_) => break,
            Err(e) => {
                log::error!("Stichwörter: {e}");
                return;
            }
        };
        let tagger = match app.state::<VisionState>().tagger(dir) {
            Ok(t) => t,
            Err(e) => {
                log::error!("Stichwörter: {e}");
                return;
            }
        };
        for (id, bytes) in pending {
            let tags = ImageVectors::from_bytes(&bytes)
                .map(|v| tagger.tags(&v))
                .unwrap_or_else(|e| {
                    log::warn!("Stichwörter {id}: Vektor unlesbar: {e}");
                    Vec::new()
                });
            if let Err(e) = library.set_tags(&id, &version, &tags) {
                log::error!("Stichwörter {id}: {e}");
                return;
            }
            tagged += 1;
        }
    }
    if tagged > 0 {
        log::info!(
            "Stichwörter für {tagged} Aufnahmen ({:?})",
            started.elapsed()
        );
        if let Err(e) = app.emit("library-changed", ()) {
            log::debug!("Ereignis library-changed: {e}");
        }
    }
}

fn progress(app: &AppHandle, library: &Library) {
    match library.embedding_progress(MODEL_ID) {
        Ok((done, total)) => {
            log::debug!("Bildinhalt: {done} von {total}");
            if let Err(e) = app.emit(
                "content-index-progress",
                ContentSearchStatus {
                    available: true,
                    done,
                    total,
                },
            ) {
                log::debug!("Ereignis content-index-progress: {e}");
            }
        }
        Err(e) => log::warn!("Bildinhalt-Fortschritt: {e}"),
    }
}

/// Ganzes Bild und Ausschnitte. Die Ausschnitte brauchen das Original – im
/// Vorschaubild (320 Pixel) wären sie zu klein; fehlt es, reicht dieses.
fn embed(library: &Library, encoder: &ImageEncoder, id: &str) -> Result<Vec<u8>, String> {
    let rec = library.get(id).map_err(|e| e.to_string())?;
    let image = [rec.original_path(), rec.thumbnail_path()]
        .iter()
        .find_map(|p| {
            let bytes = std::fs::read(p).ok()?;
            grabmark_render::decode_image(&bytes).ok()
        })
        .ok_or("Bild nicht lesbar")?;
    Ok(encoder
        .embed_all(&image)
        .map_err(|e| e.to_string())?
        .to_bytes())
}

/// Aufnahmen, deren Bildinhalt zum Suchtext passt (bester zuerst). Leer,
/// wenn die Erkennung aus ist oder das Modell fehlt.
pub fn search(app: &AppHandle, text: &str) -> Vec<String> {
    let state = app.state::<VisionState>();
    let (Some(dir), true) = (&state.model_dir, settings::get(app).content_search) else {
        return Vec::new();
    };
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    let run = || -> Result<Vec<String>, String> {
        let start = Instant::now();
        let tagger = state.tagger(dir)?;
        let query = state
            .text_encoder(dir)?
            .embed(text)
            .map_err(|e| e.to_string())?;
        let vectors = state.vectors(&app.state::<CaptureState>().library)?;
        let hits = tagger.search(&query, vectors.iter().map(|(id, e)| (id.as_str(), e)));
        // Ohne Suchtext – der ist privat.
        log::debug!(
            "Suche nach Bildinhalt: {} Treffer in {} Bildern, {:?}",
            hits.len(),
            vectors.len(),
            start.elapsed()
        );
        Ok(hits
            .into_iter()
            .take(MAX_HITS)
            .map(|(id, _)| id.to_string())
            .collect())
    };
    run().unwrap_or_else(|e| {
        log::error!("Suche nach Bildinhalt: {e}");
        Vec::new()
    })
}

/// Stichwörter, deren Name (deutsch oder englisch) zum Suchtext passt –
/// „Boot“ findet so alles mit dem Stichwort Boot, „Tiere“ alles aus der
/// Gruppe Tiere.
pub fn tags_for_text(text: &str) -> Vec<String> {
    let words: Vec<String> = text
        .split_whitespace()
        .map(str::to_lowercase)
        .filter(|w| w.chars().count() >= 3)
        .collect();
    let whole = text.trim().to_lowercase();
    let matches = |key: &str| {
        crate::i18n::all(key)
            .iter()
            .map(|l| l.to_lowercase())
            .any(|label| {
                label == whole
                    || words.iter().any(|w| {
                        label == *w || (w.chars().count() >= 4 && label.starts_with(w.as_str()))
                    })
            })
    };
    grabmark_vision::terms::TERMS
        .iter()
        .filter(|term| {
            matches(&format!("tag.{}", term.key)) || matches(&format!("group.{}", term.group))
        })
        .map(|term| term.key.to_string())
        .collect()
}

/// Anzahl Aufnahmen je Schnellfilter aus (ID, Stichwort); nur Gruppen mit
/// Treffern, in der festen Reihenfolge.
pub fn group_counts(tagged: &[(String, String)]) -> Vec<(String, u32)> {
    use std::collections::{HashMap, HashSet};
    let group_of: HashMap<&str, &str> = grabmark_vision::terms::TERMS
        .iter()
        .map(|t| (t.key, t.group))
        .collect();
    let mut ids: HashMap<&str, HashSet<&str>> = HashMap::new();
    for (id, term) in tagged {
        if let Some(group) = group_of.get(term.as_str()) {
            ids.entry(group).or_default().insert(id);
        }
    }
    grabmark_vision::terms::GROUPS
        .iter()
        .filter_map(|g| ids.get(g).map(|s| (g.to_string(), s.len() as u32)))
        .collect()
}

/// Stichwörter einer Gruppe (Schnellfilter).
pub fn group_tags(group: &str) -> Vec<String> {
    grabmark_vision::terms::in_group(group)
        .map(|t| t.key.to_string())
        .collect()
}

/// Stand der Erkennung für Einstellungen und Galerie.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct ContentSearchStatus {
    /// Modell ist mitgeliefert.
    pub available: bool,
    pub done: u32,
    pub total: u32,
}

#[tauri::command]
#[specta::specta]
pub fn content_search_status(app: AppHandle) -> Result<ContentSearchStatus, String> {
    let available = app.state::<VisionState>().model_dir.is_some();
    let (done, total) = app
        .state::<CaptureState>()
        .library
        .embedding_progress(MODEL_ID)
        .inspect_err(|e| log::error!("Bildinhalt-Stand: {e}"))
        .map_err(|e| e.to_string())?;
    Ok(ContentSearchStatus {
        available,
        done,
        total,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn gruppen_zaehlen_jede_aufnahme_einmal() {
        let tagged: Vec<(String, String)> = [
            ("a", "dog"),
            ("a", "cat"),
            ("b", "cat"),
            ("c", "code"),
            ("d", "logo"),
        ]
        .iter()
        .map(|(i, t)| (i.to_string(), t.to_string()))
        .collect();
        assert_eq!(
            group_counts(&tagged),
            vec![("animals".to_string(), 2), ("code".to_string(), 1)]
        );
    }
}
