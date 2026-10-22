//! Commands für Editor und Galerie. Die Logik liegt in `nabshot-library` und
//! `nabshot-render`; hier wird nur verdrahtet.

use std::collections::HashMap;
use std::sync::Mutex;
use std::time::Instant;

use nabshot_core::{AssetRef, Document, RgbaImage};
use nabshot_library::{CaptureRecord, Library};
use nabshot_render::{RenderOptions, Renderer};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

use crate::capture::{self, CaptureState};

/// Der native Renderer (Export, Zwischenablage, Vorschaubilder).
pub struct EditorState {
    renderer: Mutex<Renderer>,
}

impl EditorState {
    pub fn new() -> Self {
        Self {
            renderer: Mutex::new(Renderer::new()),
        }
    }
}

/// Eine Aufnahme für Galerie und Editor.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct CaptureInfo {
    pub id: String,
    /// Unix-Zeit in Millisekunden
    pub created_at: f64,
    pub mode: String,
    pub width: u32,
    pub height: u32,
    pub scale_factor: f32,
    pub starred: bool,
    /// Absolute Pfade – im Frontend über `convertFileSrc` laden.
    pub original_path: String,
    pub thumbnail_path: String,
    /// Monatsordner; hineingezogene Bilder liegen als `<id>_asset_<hash>.png` darin.
    pub dir: String,
    /// Nur bei Videos: Länge in Millisekunden und das abspielbare MP4.
    pub duration_ms: Option<f64>,
    pub video_path: Option<String>,
}

impl From<&CaptureRecord> for CaptureInfo {
    fn from(r: &CaptureRecord) -> Self {
        Self {
            id: r.id.clone(),
            created_at: r.created_at_ms as f64,
            mode: r.mode.clone(),
            width: r.width,
            height: r.height,
            scale_factor: r.scale_factor,
            starred: r.starred,
            original_path: r.original_path().to_string_lossy().into_owned(),
            thumbnail_path: r.thumbnail_path().to_string_lossy().into_owned(),
            dir: r.dir.to_string_lossy().into_owned(),
            duration_ms: r.duration_ms.map(|d| d as f64),
            video_path: r
                .is_video()
                .then(|| r.video_path().to_string_lossy().into_owned()),
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    Png,
    Jpg,
}

#[derive(Debug, Clone, Copy, Deserialize, Type)]
#[serde(rename_all = "lowercase")]
pub enum CaptureMode {
    Region,
    Window,
    Screen,
    Scroll,
    Text,
    Video,
    Webcam,
}

fn library(app: &AppHandle) -> std::sync::Arc<Library> {
    app.state::<CaptureState>().library.clone()
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Millisekunden seit `start` (fürs Log).
fn ms(start: Instant) -> f64 {
    start.elapsed().as_secs_f64() * 1000.0
}

/// Neueste Aufnahmen zuerst.
#[tauri::command]
#[specta::specta]
pub fn list_captures(app: AppHandle, limit: u32) -> Result<Vec<CaptureInfo>, String> {
    let start = Instant::now();
    let list: Vec<CaptureInfo> = library(&app)
        .recent(limit)
        .inspect_err(|e| log::error!("Aufnahmen auflisten: {e}"))
        .map_err(err)?
        .iter()
        .map(CaptureInfo::from)
        .collect();
    log::debug!("Aufnahmen auflisten: {} in {:.0} ms", list.len(), ms(start));
    Ok(list)
}

#[tauri::command]
#[specta::specta]
pub fn get_capture(app: AppHandle, id: String) -> Result<CaptureInfo, String> {
    Ok(CaptureInfo::from(
        &library(&app)
            .get(&id)
            .inspect_err(|e| log::error!("Aufnahme {id}: {e}"))
            .map_err(err)?,
    ))
}

#[tauri::command]
#[specta::specta]
pub fn load_document(app: AppHandle, id: String) -> Result<Document, String> {
    let start = Instant::now();
    let doc = library(&app)
        .load_document(&id)
        .inspect_err(|e| log::error!("Editor öffnen {id}: {e}"))
        .map_err(err)?;
    log::info!(
        "Editor öffnet {id}: {} Ebenen ({:.0} ms)",
        doc.layers.len(),
        ms(start)
    );
    Ok(doc)
}

/// Speichert das Dokument sofort; das Vorschaubild wird im Hintergrund neu
/// gezeichnet.
#[tauri::command]
#[specta::specta]
pub fn save_document(app: AppHandle, id: String, document: Document) -> Result<(), String> {
    let start = Instant::now();
    let lib = library(&app);
    lib.save_document(&id, &document, None)
        .inspect_err(|e| log::error!("Editor speichern {id}: {e}"))
        .map_err(err)?;
    log::info!(
        "Editor gespeichert {id}: {} Ebenen ({:.0} ms)",
        document.layers.len(),
        ms(start)
    );
    std::thread::spawn(move || {
        let start = Instant::now();
        let state = app.state::<EditorState>();
        let preview = render(
            &lib,
            &state,
            &id,
            &document,
            Some(nabshot_library::THUMBNAIL_MAX * 2),
        );
        match preview {
            Ok(img) => match lib.save_document(&id, &document, Some(&img)) {
                Ok(()) => log::debug!("Vorschaubild {id} neu gezeichnet ({:.0} ms)", ms(start)),
                Err(e) => log::error!("Vorschaubild {id}: {e}"),
            },
            Err(e) => log::error!("Vorschaubild {id}: {e}"),
        }
        crate::services::refresh_export(&app, &id);
    });
    Ok(())
}

/// Rechnet das Dokument flach und legt es in die Zwischenablage.
#[tauri::command]
#[specta::specta]
pub async fn copy_document(app: AppHandle, id: String, document: Document) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let start = Instant::now();
        let lib = library(&app);
        let state = app.state::<EditorState>();
        let result = render(&lib, &state, &id, &document, None).and_then(|img| {
            nabshot_clipboard::copy_image(&img, document.pixel_ratio)
                .map_err(err)
                .map(|()| (img.width(), img.height()))
        });
        match &result {
            Ok((w, h)) => log::info!(
                "Zwischenablage: Editor {id} kopiert, {w}×{h} ({:.0} ms)",
                ms(start)
            ),
            Err(e) => log::error!("Editor kopieren {id}: {e}"),
        }
        result.map(|_| ())
    })
    .await
    .map_err(err)?
}

/// Fragt nach dem Ziel und schreibt PNG oder JPG. Gibt den Pfad zurück
/// (`None` = abgebrochen).
#[tauri::command]
#[specta::specta]
pub async fn export_document(
    app: AppHandle,
    id: String,
    document: Document,
    format: ExportFormat,
) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let (ext, label) = match format {
            ExportFormat::Png => ("png", "PNG"),
            ExportFormat::Jpg => ("jpg", "JPEG"),
        };
        let Some(path) = app
            .dialog()
            .file()
            .set_file_name(format!("Nabshot {id}.{ext}"))
            .add_filter(label, &[ext])
            .blocking_save_file()
        else {
            log::info!("Editor exportieren {id}: abgebrochen");
            return Ok(None);
        };
        let start = Instant::now();
        let written = write_export(&app, &id, &document, format, path);
        match &written {
            Ok(path) => log::info!(
                "Editor exportiert {id} als {label}: {} ({:.0} ms)",
                path.display(),
                ms(start)
            ),
            Err(e) => log::error!("Editor exportieren {id}: {e}"),
        }
        Ok(Some(written?.to_string_lossy().into_owned()))
    })
    .await
    .map_err(err)?
}

/// Zeichnet das Dokument und schreibt es an den gewählten Ort.
fn write_export(
    app: &AppHandle,
    id: &str,
    document: &Document,
    format: ExportFormat,
    path: tauri_plugin_dialog::FilePath,
) -> Result<std::path::PathBuf, String> {
    let path = path.into_path().map_err(err)?;
    let lib = library(app);
    let state = app.state::<EditorState>();
    let img = render(&lib, &state, id, document, None)?;
    let bytes = match format {
        ExportFormat::Png => {
            nabshot_library::encode_png(&img, document.pixel_ratio).map_err(err)?
        }
        ExportFormat::Jpg => nabshot_library::encode_jpeg(&img, 90)?,
    };
    std::fs::write(&path, bytes).map_err(err)?;
    Ok(path)
}

/// Legt das Video als Datei in die Zwischenablage (vorher, falls nötig,
/// neu zusammengesetzt).
#[tauri::command]
#[specta::specta]
pub async fn copy_video(app: AppHandle, id: String) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let start = Instant::now();
        let result = crate::video_edit::ensure_current(&app, &id).and_then(|()| {
            let rec = library(&app).get(&id).map_err(err)?;
            nabshot_clipboard::copy_file(&rec.video_path()).map_err(err)
        });
        match &result {
            Ok(()) => log::info!("Zwischenablage: Video {id} kopiert ({:.0} ms)", ms(start)),
            Err(e) => log::error!("Video kopieren {id}: {e}"),
        }
        result
    })
    .await
    .map_err(err)?
}

/// Fragt nach dem Ziel und kopiert das Video dorthin (`None` = abgebrochen).
#[tauri::command]
#[specta::specta]
pub async fn save_video_as(app: AppHandle, id: String) -> Result<Option<String>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        crate::video_edit::ensure_current(&app, &id)
            .inspect_err(|e| log::error!("Video sichern {id}: {e}"))?;
        let rec = library(&app)
            .get(&id)
            .inspect_err(|e| log::error!("Video sichern {id}: {e}"))
            .map_err(err)?;
        let Some(path) = app
            .dialog()
            .file()
            .set_file_name(format!("Nabshot {id}.mp4"))
            .add_filter("MP4", &["mp4"])
            .blocking_save_file()
        else {
            log::info!("Video sichern {id}: abgebrochen");
            return Ok(None);
        };
        let start = Instant::now();
        let path = path.into_path().map_err(err)?;
        std::fs::copy(rec.video_path(), &path)
            .inspect_err(|e| log::error!("Video sichern {id}: {e}"))
            .map_err(err)?;
        log::info!(
            "Video {id} gesichert: {} ({:.0} ms)",
            path.display(),
            ms(start)
        );
        Ok(Some(path.to_string_lossy().into_owned()))
    })
    .await
    .map_err(err)?
}

/// Ein hineingezogenes Bild, abgelegt bei der Aufnahme.
#[derive(Debug, Clone, Serialize, Type)]
pub struct AssetInfo {
    pub hash: String,
    pub width: u32,
    pub height: u32,
}

/// Legt ein Bild (PNG, JPEG oder WebP) bei der Aufnahme ab – als PNG.
fn store_asset(app: &AppHandle, id: &str, bytes: &[u8]) -> Result<AssetInfo, String> {
    store_asset_inner(app, id, bytes).inspect_err(|e| log::error!("Bild einfügen {id}: {e}"))
}

fn store_asset_inner(app: &AppHandle, id: &str, bytes: &[u8]) -> Result<AssetInfo, String> {
    let start = Instant::now();
    let img = nabshot_render::decode_image(bytes).map_err(err)?;
    let png = if bytes.starts_with(b"\x89PNG") {
        bytes.to_vec()
    } else {
        nabshot_library::encode_png(&img, 1.0).map_err(err)?
    };
    let hash = library(app).add_asset(id, &png).map_err(err)?;
    log::info!(
        "Bild eingefügt in {id}: {}×{}, {} kB ({:.0} ms)",
        img.width(),
        img.height(),
        png.len() / 1024,
        ms(start)
    );
    Ok(AssetInfo {
        hash,
        width: img.width(),
        height: img.height(),
    })
}

/// Bild aus der Zwischenablage o. Ä. (Bytes).
#[tauri::command]
#[specta::specta]
pub async fn add_asset(app: AppHandle, id: String, bytes: Vec<u8>) -> Result<AssetInfo, String> {
    tauri::async_runtime::spawn_blocking(move || store_asset(&app, &id, &bytes))
        .await
        .map_err(err)?
}

/// Bild aus einer Datei (z. B. aus dem Finder hineingezogen).
#[tauri::command]
#[specta::specta]
pub async fn add_asset_file(app: AppHandle, id: String, path: String) -> Result<AssetInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let bytes = std::fs::read(&path)
            .inspect_err(|e| log::error!("Bild einfügen {id}: Datei lesen: {e}"))
            .map_err(err)?;
        store_asset(&app, &id, &bytes)
    })
    .await
    .map_err(err)?
}

/// Eine andere Aufnahme (fertig gezeichnet) als Bild-Ebene übernehmen.
#[tauri::command]
#[specta::specta]
pub async fn add_asset_from_capture(
    app: AppHandle,
    id: String,
    source_id: String,
) -> Result<AssetInfo, String> {
    tauri::async_runtime::spawn_blocking(move || {
        log::debug!("Aufnahme {source_id} als Bild in {id} übernehmen");
        let lib = library(&app);
        let png = lib
            .load_document(&source_id)
            .map_err(err)
            .and_then(|doc| {
                let state = app.state::<EditorState>();
                render(&lib, &state, &source_id, &doc, None)
            })
            .and_then(|img| nabshot_library::encode_png(&img, 1.0).map_err(err))
            .inspect_err(|e| log::error!("Aufnahme {source_id} übernehmen: {e}"))?;
        store_asset(&app, &id, &png)
    })
    .await
    .map_err(err)?
}

/// Ein erkannter Textblock in Pixeln der Originalaufnahme.
#[derive(Debug, Clone, Serialize, Type)]
pub struct OcrBlock {
    pub text: String,
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

/// Texterkennung auf der Originalaufnahme.
#[tauri::command]
#[specta::specta]
pub async fn recognize_text(app: AppHandle, id: String) -> Result<Vec<OcrBlock>, String> {
    tauri::async_runtime::spawn_blocking(move || {
        let start = Instant::now();
        let state = app.state::<CaptureState>();
        let ocr = state.ocr.as_ref().ok_or_else(|| {
            log::warn!("Texterkennung {id}: nicht verfügbar");
            crate::i18n::t("ocr.unavailable", &[])
        })?;
        let blocks = state
            .library
            .read_original(&id)
            .map_err(err)
            .and_then(|bytes| nabshot_render::decode_png(&bytes).map_err(err))
            .and_then(|img| ocr.recognize(&img).map_err(err))
            .inspect_err(|e| log::error!("Texterkennung {id}: {e}"))?;
        // Nur die Anzahl – der erkannte Text ist privat.
        log::info!(
            "Texterkennung {id}: {} Blöcke ({:.0} ms)",
            blocks.len(),
            ms(start)
        );
        Ok(blocks
            .into_iter()
            .map(|b| OcrBlock {
                text: b.text,
                x: b.bounds.x,
                y: b.bounds.y,
                width: b.bounds.width,
                height: b.bounds.height,
            })
            .collect())
    })
    .await
    .map_err(err)?
}

#[tauri::command]
#[specta::specta]
pub fn copy_text(text: String) -> Result<(), String> {
    match nabshot_clipboard::copy_text(&text) {
        Ok(()) => {
            log::info!("Zwischenablage: Text kopiert");
            Ok(())
        }
        Err(e) => {
            log::error!("Zwischenablage (Text): {e}");
            Err(err(e))
        }
    }
}

/// Änderungszähler der Zwischenablage (`None` = gibt es hier nicht).
#[tauri::command]
#[specta::specta]
pub fn clipboard_change_count() -> Option<u32> {
    nabshot_clipboard::change_count()
}

#[tauri::command]
#[specta::specta]
pub fn delete_capture(app: AppHandle, id: String) -> Result<(), String> {
    match library(&app).delete(&id) {
        Ok(()) => {
            log::info!("Aufnahme {id} gelöscht");
            Ok(())
        }
        Err(e) => {
            log::error!("Aufnahme {id} löschen: {e}");
            Err(err(e))
        }
    }
}

#[tauri::command]
#[specta::specta]
pub fn set_starred(app: AppHandle, id: String, starred: bool) -> Result<(), String> {
    log::info!("Aufnahme {id}: Stern {starred}");
    library(&app)
        .set_starred(&id, starred)
        .inspect_err(|e| log::error!("Stern {id}: {e}"))
        .map_err(err)
}

/// Zeigt die Originaldatei im Finder / Explorer.
#[tauri::command]
#[specta::specta]
pub fn reveal_capture(app: AppHandle, id: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    log::info!("Aufnahme {id} im Dateimanager zeigen");
    let rec = library(&app)
        .get(&id)
        .inspect_err(|e| log::error!("Zeigen {id}: {e}"))
        .map_err(err)?;
    app.opener()
        .reveal_item_in_dir(rec.original_path())
        .inspect_err(|e| log::error!("Zeigen {id}: {e}"))
        .map_err(err)
}

/// Aufnahme aus der Oberfläche starten (z. B. Knopf im Editor).
#[tauri::command]
#[specta::specta]
pub fn start_capture(app: AppHandle, mode: CaptureMode) {
    log::info!("Aufnahme aus der Oberfläche: {mode:?}");
    let mode = match mode {
        CaptureMode::Region => capture::Mode::Region,
        CaptureMode::Window => capture::Mode::Window,
        CaptureMode::Screen => capture::Mode::Screen,
        CaptureMode::Scroll => capture::Mode::scroll(&app),
        CaptureMode::Text => capture::Mode::Text,
        CaptureMode::Video => capture::Mode::Video,
        CaptureMode::Webcam => capture::Mode::Webcam,
    };
    // Fenster erst verstecken, damit es nicht im Bild ist.
    crate::tray::hide_main_window(&app);
    crate::tray::hide_popover(&app);
    let app2 = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        capture::start(&app2, mode);
    });
}

/// Fertiges Bild in voller Grösse (für Dienste ausserhalb des Editors).
pub fn render_native(
    app: &AppHandle,
    lib: &Library,
    id: &str,
    doc: &Document,
) -> Result<RgbaImage, String> {
    render(lib, &app.state::<EditorState>(), id, doc, None)
}

/// Zeichnet das Dokument nativ. `max` begrenzt die Kantenlänge (Vorschau).
fn render(
    lib: &Library,
    state: &State<'_, EditorState>,
    id: &str,
    doc: &Document,
    max: Option<u32>,
) -> Result<RgbaImage, String> {
    let start = Instant::now();
    let mut assets: HashMap<AssetRef, RgbaImage> = HashMap::new();
    for asset in doc.assets() {
        if assets.contains_key(asset) {
            continue;
        }
        let bytes = match asset {
            AssetRef::Original => lib.read_original(id),
            AssetRef::Asset(hash) => lib.read_asset(id, hash),
        }
        .map_err(err)?;
        assets.insert(
            asset.clone(),
            nabshot_render::decode_png(&bytes).map_err(err)?,
        );
    }
    let opts = match max {
        Some(max) => RenderOptions::fit(doc, max),
        None => RenderOptions::full(doc),
    };
    let mut renderer = state.renderer.lock().unwrap_or_else(|e| e.into_inner());
    let img = renderer.render_with(doc, &assets, &opts).map_err(err)?;
    log::debug!(
        "Zeichnen {id}: {}×{}, {} Ebenen ({:.0} ms)",
        img.width(),
        img.height(),
        doc.layers.len(),
        ms(start)
    );
    Ok(img)
}
