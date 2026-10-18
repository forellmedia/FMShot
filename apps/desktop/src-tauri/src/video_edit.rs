//! Video-Editor (ADR 0011, Etappe 4–6): Spuren für die Vorschau nennen,
//! nach Änderungen neu zusammensetzen, GIF und Einzelbild. Die Logik liegt
//! in `fmshot-video` und `fmshot-media`; hier wird nur verdrahtet.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use fmshot_core::{
    AssetRef, Document, FaceTrack, InputTrack, LayerKind, RgbaImage, VideoDoc, WebcamFraming,
};
use fmshot_library::{CaptureRecord, Library, NewCapture};
use fmshot_media::{AudioInput, RecordFiles};
use fmshot_video::cursor::CURSOR_POINTS;
use fmshot_video::overlay::LayerOverlay;
use fmshot_video::timeline::{kept, output_duration};
use fmshot_video::{CursorTrack, ExportExtras, FrameSink, HeadPath};
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
        screen: rec.screen_path(),
        system_audio: existing(rec.system_audio_path()),
        microphone: existing(rec.microphone_path()),
        webcam: existing(rec.webcam_path()),
    }
}

/// Datei neben der Bildschirm-Spur, z. B. `_faces.json`.
fn sidecar(files: &RecordFiles, suffix: &str) -> PathBuf {
    let screen = files.screen.to_string_lossy();
    PathBuf::from(screen.replace("_screen.mp4", suffix))
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
                .and_then(|b| fmshot_render::decode_png(&b).map_err(err))
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
            let faces: FaceTrack = read_json(&sidecar(files, "_faces.json"))?;
            HeadPath::new(&faces, w.layout.follow)
        });
    let cursor = read_json::<InputTrack>(&sidecar(files, "_input.json"))
        .map(|t| CursorTrack::new(&t))
        .filter(|t| !t.is_empty());
    let overlay = layers
        .and_then(|(doc, assets)| LayerOverlay::new(doc, assets))
        .map(|o| Box::new(o) as Box<dyn fmshot_video::OverlaySource>);
    ExportExtras {
        head,
        segmenter: None,
        cursor,
        cursor_px: CURSOR_POINTS * scale.max(1.0),
        overlay,
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
) -> Result<fmshot_video::ExportStats, String> {
    let screen = fmshot_media::open_video(&files.screen).map_err(err)?;
    let webcam = match (&video.webcam, &files.webcam) {
        (Some(_), Some(path)) => match fmshot_media::open_video(path) {
            Ok(w) => Some(w),
            Err(e) => {
                log::warn!("Webcam lesen: {e}");
                None
            }
        },
        _ => None,
    };
    fmshot_video::export_with(video, screen, webcam, extras, sink, progress).map_err(err)
}

fn screen_size(files: &RecordFiles) -> Result<(u32, u32), String> {
    Ok(fmshot_video::output_size(
        fmshot_media::open_video(&files.screen).map_err(err)?.size(),
    ))
}

/// Setzt Bildschirm, Webcam, Zeiger und Ebenen auf der Grafikkarte zusammen
/// und mischt den Ton dazu (mit Schnitten und Lautstärke).
pub fn compose(
    files: &RecordFiles,
    video: &VideoDoc,
    extras: ExportExtras,
    out: &Path,
    progress: &mut dyn FnMut(f32) -> bool,
) -> Result<fmshot_video::ExportStats, String> {
    let size = screen_size(files)?;
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
    let mut sink = fmshot_media::create_mp4(
        out,
        size,
        video.fps,
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
    pub screen: String,
    pub webcam: Option<String>,
    pub system_audio: Option<String>,
    pub microphone: Option<String>,
    /// Mausspur (JSON, Etappe 5).
    pub input: Option<String>,
}

#[tauri::command]
#[specta::specta]
pub fn video_tracks(app: AppHandle, id: String) -> Result<VideoTracks, String> {
    let rec = library(&app).get(&id).map_err(err)?;
    let s = |p: Option<PathBuf>| p.map(|p| p.to_string_lossy().into_owned());
    let files = record_files(&rec);
    Ok(VideoTracks {
        screen: rec.screen_path().to_string_lossy().into_owned(),
        webcam: s(files.webcam),
        system_audio: s(files.system_audio),
        microphone: s(files.microphone),
        input: s(existing(rec.input_path())),
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

/// Nach Änderungen im Editor: Video neu zusammensetzen. Ersetzt
/// `<id>_video.mp4` erst, wenn alles fertig ist. Ein neuer Aufruf bricht
/// einen laufenden ab (dieser meldet dann `abgebrochen`).
#[tauri::command]
#[specta::specta]
pub async fn export_video(app: AppHandle, id: String) -> Result<CaptureInfo, String> {
    let cancel = begin_export(&app, &id);
    let task = {
        let (app, id, cancel) = (app.clone(), id.clone(), cancel.clone());
        tauri::async_runtime::spawn_blocking(move || {
            let lib = library(&app);
            let rec = lib.get(&id).map_err(err)?;
            let doc = lib.load_document(&id).map_err(err)?;
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
            let mut progress = progress_reporter(&app, &id, cancel);
            compose(&files, &video, extras, &temp, &mut progress)?;
            std::fs::rename(&temp, &target).map_err(err)?;
            let poster = fmshot_media::frame_at(&target, Duration::ZERO).map_err(err)?;
            lib.refresh_video(&id, &poster, output_duration(&video) as i64)
                .map_err(err)?;
            crate::services::refresh_export(&app, &id);
            let _ = app.emit("library-changed", ());
            Ok(CaptureInfo::from(&lib.get(&id).map_err(err)?))
        })
    };
    let result = task.await.map_err(err)?;
    end_export(&app, &id, &cancel);
    result
}

/// Fragt nach dem Ziel und schreibt ein GIF (`None` = abgebrochen).
#[tauri::command]
#[specta::specta]
pub async fn export_gif(app: AppHandle, id: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let Some(path) = app
            .dialog()
            .file()
            .set_file_name(format!("FMShot {id}.gif"))
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
        let mut sink = fmshot_video::GifSink::create(&path, screen_size(&files)?)?;
        let extras = extras(
            &files,
            &video,
            rec.scale_factor,
            Some((&doc, layer_assets(&rec, &doc))),
        );
        let mut progress = progress_reporter(&app, &id, Arc::new(AtomicBool::new(false)));
        compose_into(&files, &video, extras, &mut sink, &mut progress)?;
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
        let lib = library(&app);
        let rec = lib.get(&id).map_err(err)?;
        let at = Duration::from_secs_f64(output_ms.max(0.0) / 1000.0);
        let image = fmshot_media::frame_at(&rec.video_path(), at).map_err(err)?;
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
