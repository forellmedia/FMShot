//! Video-Editor (ADR 0011, Etappe 4–6): Spuren für die Vorschau nennen,
//! auf Wunsch neu zusammensetzen, GIF und Einzelbild. Die Logik liegt
//! in `grabmark-video` und `grabmark-media`; hier wird nur verdrahtet.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use grabmark_core::{
    AssetRef, Document, FaceTrack, InputTrack, LayerKind, RgbaImage, VideoDoc, VideoEncoding,
    WebcamBackground, WebcamFraming,
};
use grabmark_library::{CaptureRecord, Library, NewCapture};
use grabmark_media::{AudioInput, RecordFiles};
use grabmark_video::cursor::CURSOR_POINTS;
use grabmark_video::overlay::LayerOverlay;
use grabmark_video::timeline::{kept, output_duration};
use grabmark_video::{CursorTrack, ExportExtras, FrameSink, HeadPath};
use serde::Serialize;
use specta::Type;
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::DialogExt;

use crate::capture::CaptureState;
use crate::editor::CaptureInfo;

/// Laufende Exporte je Aufnahme – ein neuer Auftrag bricht den alten ab.
#[derive(Default)]
pub struct VideoEditState {
    running: Mutex<HashMap<String, Arc<AtomicBool>>>,
}

fn library(app: &AppHandle) -> Arc<Library> {
    app.state::<CaptureState>().library.clone()
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

fn existing(p: PathBuf) -> Option<PathBuf> {
    p.exists().then_some(p)
}

/// Spuren einer Aufnahme aus der Bibliothek.
pub fn record_files(rec: &CaptureRecord) -> RecordFiles {
    RecordFiles {
        screen: existing(rec.screen_path()),
        system_audio: existing(rec.system_audio_path()),
        microphone: existing(rec.microphone_path()),
        webcam: existing(rec.webcam_path()),
        faces: existing(rec.faces_path()),
    }
}

/// Datei neben den Spuren, z. B. `_input.json`.
fn sidecar(files: &RecordFiles, suffix: &str) -> Option<PathBuf> {
    if let Some(screen) = &files.screen {
        return Some(PathBuf::from(
            screen.to_string_lossy().replace("_screen.mp4", suffix),
        ));
    }
    let webcam = files.webcam.as_ref()?.to_string_lossy();
    Some(PathBuf::from(webcam.replace("_webcam.mp4", suffix)))
}

fn read_json<T: serde::de::DeserializeOwned>(path: &Path) -> Option<T> {
    serde_json::from_slice(&std::fs::read(path).ok()?).ok()
}

/// Bilder, die in die Ebenen hineingezogen wurden (`<id>_asset_<hash>.png`).
fn layer_assets(rec: &CaptureRecord, doc: &Document) -> HashMap<AssetRef, RgbaImage> {
    let mut assets = HashMap::new();
    for l in &doc.layers {
        if let LayerKind::Image {
            asset: AssetRef::Asset(hash),
            ..
        } = &l.kind
        {
            match std::fs::read(rec.asset_path(hash))
                .map_err(err)
                .and_then(|b| grabmark_render::decode_png(&b).map_err(err))
            {
                Ok(img) => {
                    assets.insert(AssetRef::Asset(hash.clone()), img);
                }
                Err(e) => log::warn!("Bild {hash}: {e}"),
            }
        }
    }
    assets
}

/// Was der Export über die Spuren hinaus braucht: Kopf, Zeiger, Ebenen.
/// `scale` = Pixel pro Bildschirmpunkt der Aufnahme.
pub fn extras(
    files: &RecordFiles,
    video: &VideoDoc,
    scale: f32,
    layers: Option<(&Document, HashMap<AssetRef, RgbaImage>)>,
) -> ExportExtras {
    let head = video
        .webcam
        .as_ref()
        .filter(|w| w.layout.framing == WebcamFraming::Head)
        .and_then(|w| {
            let faces: FaceTrack = read_json(files.faces.as_deref()?)?;
            HeadPath::new(&faces, w.layout.follow)
        });
    let segmenter = video
        .webcam
        .as_ref()
        .filter(|w| w.layout.background != WebcamBackground::Original)
        .and_then(|_| grabmark_media::segmenter());
    let cursor = sidecar(files, "_input.json")
        .and_then(|p| read_json::<InputTrack>(&p))
        .map(|t| CursorTrack::new(&t))
        .filter(|t| !t.is_empty());
    let overlay = layers
        .and_then(|(doc, assets)| LayerOverlay::new(doc, assets))
        .map(|o| Box::new(o) as Box<dyn grabmark_video::OverlaySource>);
    ExportExtras {
        head,
        segmenter,
        cursor,
        cursor_px: CURSOR_POINTS * scale.max(1.0),
        overlay,
        size: None,
    }
}

/// Spuren zu einem fertigen Bild zusammensetzen und in `sink` geben (MP4
/// oder GIF). `progress` bekommt 0 bis 1; `false` bricht ab.
fn compose_into(
    files: &RecordFiles,
    video: &VideoDoc,
    extras: ExportExtras,
    sink: &mut dyn FrameSink,
    progress: &mut dyn FnMut(f32) -> bool,
) -> Result<grabmark_video::ExportStats, String> {
    let screen: Box<dyn grabmark_video::FrameSource> = match &files.screen {
        Some(path) => grabmark_media::open_video(path).map_err(err)?,
        // Nur Webcam: eine leere Spur gibt die Grösse vor.
        None => Box::new(grabmark_video::BlankSource::new(frame_size(files, video)?)),
    };
    let webcam = match (&video.webcam, &files.webcam) {
        (Some(_), Some(path)) => match grabmark_media::open_video(path) {
            Ok(w) => Some(w),
            Err(e) => {
                log::warn!("Webcam lesen: {e}");
                None
            }
        },
        _ => None,
    };
    grabmark_video::export_with(video, screen, webcam, extras, sink, progress).map_err(err)
}

/// Grösse des fertigen Videos: wie der Bildschirm bzw. bei „nur Webcam“
/// aus Format und Kamerabild.
pub fn frame_size(files: &RecordFiles, video: &VideoDoc) -> Result<(u32, u32), String> {
    match (video.camera_only, &files.screen, &files.webcam) {
        (None, Some(screen), _) => Ok(grabmark_video::output_size(
            grabmark_media::open_video(screen).map_err(err)?.size(),
        )),
        (Some(format), _, Some(webcam)) => Ok(grabmark_video::camera_frame_size(
            format,
            grabmark_media::open_video(webcam).map_err(err)?.size(),
        )),
        _ => Err("Spur fehlt".into()),
    }
}

/// Setzt Bildschirm, Webcam, Zeiger und Ebenen auf der Grafikkarte zusammen
/// und mischt den Ton dazu (mit Schnitten und Lautstärke). Auflösung, Codec
/// und Qualität aus den Einstellungen (`encoding`).
pub fn compose(
    files: &RecordFiles,
    video: &VideoDoc,
    mut extras: ExportExtras,
    encoding: &VideoEncoding,
    out: &Path,
    progress: &mut dyn FnMut(f32) -> bool,
) -> Result<grabmark_video::ExportStats, String> {
    let size = grabmark_video::export_size(frame_size(files, video)?, encoding.resolution);
    extras.size = Some(size);
    let mut audio = Vec::new();
    if let Some(p) = &files.system_audio {
        audio.push(AudioInput {
            path: p.clone(),
            volume: video.audio.system,
        });
    }
    if let Some(p) = &files.microphone {
        audio.push(AudioInput {
            path: p.clone(),
            volume: video.audio.microphone,
        });
    }
    let mut sink = grabmark_media::create_mp4(
        out,
        size,
        video.fps,
        encoding,
        &audio,
        &kept(video),
        Duration::from_millis(output_duration(video) as u64),
    )
    .map_err(err)?;
    compose_into(files, video, extras, sink.as_mut(), progress)
}

/// Spuren eines Videos für die Vorschau im Editor (nur vorhandene).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct VideoTracks {
    /// `None` bei „nur Webcam“.
    pub screen: Option<String>,
    pub webcam: Option<String>,
    pub system_audio: Option<String>,
    pub microphone: Option<String>,
    /// Mausspur (JSON, Etappe 5).
    pub input: Option<String>,
    /// Kopfpositionen der Webcam (JSON, Etappe 3).
    pub faces: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub fn video_tracks(app: AppHandle, id: String) -> Result<VideoTracks, String> {
    let rec = library(&app).get(&id).map_err(err)?;
    let s = |p: Option<PathBuf>| p.map(|p| p.to_string_lossy().into_owned());
    let files = record_files(&rec);
    Ok(VideoTracks {
        screen: s(files.screen),
        webcam: s(files.webcam),
        system_audio: s(files.system_audio),
        microphone: s(files.microphone),
        input: s(existing(rec.input_path())),
        faces: s(files.faces),
    })
}

/// Fortschritt beim Zusammensetzen (Ereignis `video-export-progress`).
#[derive(Debug, Clone, Serialize, Type)]
pub struct ExportProgress {
    pub id: String,
    pub progress: f32,
}

/// Meldet den Fortschritt und bricht ab, sobald `cancel` gesetzt ist.
fn progress_reporter(
    app: &AppHandle,
    id: &str,
    cancel: Arc<AtomicBool>,
) -> impl FnMut(f32) -> bool {
    let (app, id) = (app.clone(), id.to_string());
    let mut shown = -1.0;
    move |p| {
        if p - shown >= 0.02 || p >= 1.0 {
            shown = p;
            let _ = app.emit(
                "video-export-progress",
                ExportProgress {
                    id: id.clone(),
                    progress: p,
                },
            );
        }
        !cancel.load(Ordering::Acquire)
    }
}

/// Neuer Export für `id`: bricht einen laufenden ab.
fn begin_export(app: &AppHandle, id: &str) -> Arc<AtomicBool> {
    let cancel = Arc::new(AtomicBool::new(false));
    let state = app.state::<VideoEditState>();
    let mut running = state.running.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(old) = running.insert(id.to_string(), cancel.clone()) {
        old.store(true, Ordering::Release);
    }
    cancel
}

fn end_export(app: &AppHandle, id: &str, cancel: &Arc<AtomicBool>) {
    let state = app.state::<VideoEditState>();
    let mut running = state.running.lock().unwrap_or_else(|e| e.into_inner());
    if running.get(id).is_some_and(|c| Arc::ptr_eq(c, cancel)) {
        running.remove(id);
    }
}

/// Video aus dem gespeicherten Dokument neu zusammensetzen. Ersetzt
/// `<id>_video.mp4` erst, wenn alles fertig ist, und merkt sich, aus
/// welchem Dokument es entstand. Bricht ab, sobald `cancel` gesetzt ist.
fn render_video(app: &AppHandle, id: &str, cancel: Arc<AtomicBool>) -> Result<(), String> {
    let lib = library(app);
    let rec = lib.get(id).map_err(err)?;
    let doc = lib.load_document(id).map_err(err)?;
    let video = doc.video.clone().ok_or("kein Video")?;
    let files = record_files(&rec);
    let target = rec.video_path();
    let temp = target.with_file_name(format!("{id}_render.mp4"));
    let extras = extras(
        &files,
        &video,
        rec.scale_factor,
        Some((&doc, layer_assets(&rec, &doc))),
    );
    let mut progress = progress_reporter(app, id, cancel);
    let encoding = crate::settings::get(app).video.encoding;
    let started = std::time::Instant::now();
    let stats = match compose(&files, &video, extras, &encoding, &temp, &mut progress) {
        Ok(stats) => stats,
        Err(e) => {
            let _ = std::fs::remove_file(&temp);
            return Err(e);
        }
    };
    crate::services::timing(
        app,
        format!(
            "Video {:.1} s: zusammengesetzt ({} Bilder, {}) in {:.0} ms",
            video.duration_ms as f64 / 1000.0,
            stats.frames,
            stats.adapter,
            started.elapsed().as_secs_f64() * 1000.0
        ),
    );
    std::fs::rename(&temp, &target).map_err(err)?;
    let poster = grabmark_media::frame_at(&target, Duration::ZERO).map_err(err)?;
    lib.refresh_video(id, &poster, output_duration(&video) as i64)
        .map_err(err)?;
    lib.mark_video_current(id, &doc).map_err(err)?;
    let _ = app.emit("library-changed", ());
    Ok(())
}

/// Vor Kopieren, Sichern usw.: das fertige Video zusammensetzen, falls es
/// nicht mehr zum Dokument passt. Läuft schon ein Auftrag, wird er ersetzt.
pub fn ensure_current(app: &AppHandle, id: &str) -> Result<(), String> {
    if library(app).video_is_current(id).unwrap_or(false) {
        return Ok(());
    }
    let cancel = begin_export(app, id);
    let result = render_video(app, id, cancel.clone());
    end_export(app, id, &cancel);
    result
}

/// Erst auf Wunsch (Kopieren, Sichern, Einzelbild): Video aus dem
/// Dokument zusammensetzen. `cancel_video_export` bricht ab (meldet dann
/// `abgebrochen`).
#[tauri::command]
#[specta::specta]
pub async fn export_video(app: AppHandle, id: String) -> Result<CaptureInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        ensure_current(&app, &id)?;
        Ok(CaptureInfo::from(&library(&app).get(&id).map_err(err)?))
    })
    .await
    .map_err(err)?
}

/// Passt das fertige Video zum gespeicherten Dokument?
#[tauri::command]
#[specta::specta]
pub fn video_is_current(app: AppHandle, id: String) -> bool {
    library(&app).video_is_current(&id).unwrap_or(false)
}

/// Laufendes Zusammensetzen (Video oder GIF) abbrechen.
#[tauri::command]
#[specta::specta]
pub fn cancel_video_export(app: AppHandle, id: String) {
    let state = app.state::<VideoEditState>();
    let running = state.running.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(cancel) = running.get(&id) {
        cancel.store(true, Ordering::Release);
    }
}

/// Fragt nach dem Ziel und schreibt ein GIF (`None` = abgebrochen).
#[tauri::command]
#[specta::specta]
pub async fn export_gif(app: AppHandle, id: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = app
            .dialog()
            .file()
            .set_file_name(format!("Grabmark {id}.gif"))
            .add_filter("GIF", &["gif"])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = path.into_path().map_err(err)?;
        let lib = library(&app);
        let rec = lib.get(&id).map_err(err)?;
        let doc = lib.load_document(&id).map_err(err)?;
        let video = doc.video.clone().ok_or("kein Video")?;
        let files = record_files(&rec);
        let mut sink = grabmark_video::GifSink::create(&path, frame_size(&files, &video)?)?;
        let extras = extras(
            &files,
            &video,
            rec.scale_factor,
            Some((&doc, layer_assets(&rec, &doc))),
        );
        let cancel = begin_export(&app, &id);
        let mut progress = progress_reporter(&app, &id, cancel.clone());
        let result = compose_into(&files, &video, extras, &mut sink, &mut progress);
        end_export(&app, &id, &cancel);
        if let Err(e) = result {
            drop(sink);
            let _ = std::fs::remove_file(&path);
            return Err(e);
        }
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
    .map_err(err)?
}

/// Bild an der Stelle `output_ms` des fertigen Videos als neue Aufnahme in
/// die Bibliothek.
#[tauri::command]
#[specta::specta]
pub async fn video_snapshot(
    app: AppHandle,
    id: String,
    output_ms: f64,
) -> Result<CaptureInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        ensure_current(&app, &id)?;
        let lib = library(&app);
        let rec = lib.get(&id).map_err(err)?;
        let at = Duration::from_secs_f64(output_ms.max(0.0) / 1000.0);
        let image = grabmark_media::frame_at(&rec.video_path(), at).map_err(err)?;
        let capture = NewCapture {
            image,
            scale_factor: rec.scale_factor,
            mode: "screen".into(),
            created_at_ms: now_ms(),
        };
        let saved = lib.save_capture(&capture).map_err(err)?;
        let _ = app.emit("capture-saved", saved.id.clone());
        Ok(CaptureInfo::from(&saved))
    })
    .await
    .map_err(err)?
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}
