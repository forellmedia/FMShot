//! Commands für Editor und Galerie. Die Logik liegt in `fmshot-library` und
//! `fmshot-render`; hier wird nur verdrahtet.

use std::collections::HashMap;
use std::sync::Mutex;

use fmshot_core::{AssetRef, Document, RgbaImage};
use fmshot_library::{CaptureRecord, Library};
use fmshot_render::{RenderOptions, Renderer};
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
}

fn library(app: &AppHandle) -> std::sync::Arc<Library> {
    app.state::<CaptureState>().library.clone()
}

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// Neueste Aufnahmen zuerst.
#[tauri::command]
#[specta::specta]
pub fn list_captures(app: AppHandle, limit: u32) -> Result<Vec<CaptureInfo>, String> {
    Ok(library(&app)
        .recent(limit)
        .map_err(err)?
        .iter()
        .map(CaptureInfo::from)
        .collect())
}

#[tauri::command]
#[specta::specta]
pub fn get_capture(app: AppHandle, id: String) -> Result<CaptureInfo, String> {
    Ok(CaptureInfo::from(&library(&app).get(&id).map_err(err)?))
}

#[tauri::command]
#[specta::specta]
pub fn load_document(app: AppHandle, id: String) -> Result<Document, String> {
    library(&app).load_document(&id).map_err(err)
}

/// Speichert das Dokument sofort; das Vorschaubild wird im Hintergrund neu
/// gezeichnet.
#[tauri::command]
#[specta::specta]
pub fn save_document(app: AppHandle, id: String, document: Document) -> Result<(), String> {
    let lib = library(&app);
    lib.save_document(&id, &document, None).map_err(err)?;
    std::thread::spawn(move || {
        let state = app.state::<EditorState>();
        let preview = render(
            &lib,
            &state,
            &id,
            &document,
            Some(fmshot_library::THUMBNAIL_MAX * 2),
        );
        match preview {
            Ok(img) => {
                if let Err(e) = lib.save_document(&id, &document, Some(&img)) {
                    log::error!("Vorschaubild {id}: {e}");
                }
            }
            Err(e) => log::error!("Vorschaubild {id}: {e}"),
        }
    });
    Ok(())
}

/// Rechnet das Dokument flach und legt es in die Zwischenablage.
#[tauri::command]
#[specta::specta]
pub async fn copy_document(app: AppHandle, id: String, document: Document) -> Result<(), String> {
    tauri::async_runtime::spawn_blocking(move || {
        let lib = library(&app);
        let state = app.state::<EditorState>();
        let img = render(&lib, &state, &id, &document, None)?;
        fmshot_clipboard::copy_image(&img, document.pixel_ratio).map_err(err)
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
            .set_file_name(format!("FMShot {id}.{ext}"))
            .add_filter(label, &[ext])
            .blocking_save_file()
        else {
            return Ok(None);
        };
        let path = path.into_path().map_err(err)?;
        let lib = library(&app);
        let state = app.state::<EditorState>();
        let img = render(&lib, &state, &id, &document, None)?;
        let bytes = match format {
            ExportFormat::Png => {
                fmshot_library::encode_png(&img, document.pixel_ratio).map_err(err)?
            }
            ExportFormat::Jpg => fmshot_library::encode_jpeg(&img, 90)?,
        };
        std::fs::write(&path, bytes).map_err(err)?;
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
    let img = fmshot_render::decode_image(bytes).map_err(err)?;
    let png = if bytes.starts_with(b"\x89PNG") {
        bytes.to_vec()
    } else {
        fmshot_library::encode_png(&img, 1.0).map_err(err)?
    };
    let hash = library(app).add_asset(id, &png).map_err(err)?;
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
        let bytes = std::fs::read(&path).map_err(err)?;
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
        let lib = library(&app);
        let doc = lib.load_document(&source_id).map_err(err)?;
        let state = app.state::<EditorState>();
        let img = render(&lib, &state, &source_id, &doc, None)?;
        let png = fmshot_library::encode_png(&img, 1.0).map_err(err)?;
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
        let state = app.state::<CaptureState>();
        let ocr = state
            .ocr
            .as_ref()
            .ok_or_else(|| crate::i18n::t("ocr.unavailable", &[]))?;
        let bytes = state.library.read_original(&id).map_err(err)?;
        let img = fmshot_render::decode_png(&bytes).map_err(err)?;
        Ok(ocr
            .recognize(&img)
            .map_err(err)?
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
    fmshot_clipboard::copy_text(&text).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn delete_capture(app: AppHandle, id: String) -> Result<(), String> {
    library(&app).delete(&id).map_err(err)
}

#[tauri::command]
#[specta::specta]
pub fn set_starred(app: AppHandle, id: String, starred: bool) -> Result<(), String> {
    library(&app).set_starred(&id, starred).map_err(err)
}

/// Zeigt die Originaldatei im Finder / Explorer.
#[tauri::command]
#[specta::specta]
pub fn reveal_capture(app: AppHandle, id: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let rec = library(&app).get(&id).map_err(err)?;
    app.opener()
        .reveal_item_in_dir(rec.original_path())
        .map_err(err)
}

/// Aufnahme aus der Oberfläche starten (z. B. Knopf im Editor).
#[tauri::command]
#[specta::specta]
pub fn start_capture(app: AppHandle, mode: CaptureMode) {
    let mode = match mode {
        CaptureMode::Region => capture::Mode::Region,
        CaptureMode::Window => capture::Mode::Window,
        CaptureMode::Screen => capture::Mode::Screen,
    };
    // Fenster erst verstecken, damit es nicht im Bild ist.
    crate::tray::hide_main_window(&app);
    let app2 = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_millis(150));
        capture::start(&app2, mode);
    });
}

/// Zeichnet das Dokument nativ. `max` begrenzt die Kantenlänge (Vorschau).
fn render(
    lib: &Library,
    state: &State<'_, EditorState>,
    id: &str,
    doc: &Document,
    max: Option<u32>,
) -> Result<RgbaImage, String> {
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
            fmshot_render::decode_png(&bytes).map_err(err)?,
        );
    }
    let opts = match max {
        Some(max) => RenderOptions::fit(doc, max),
        None => RenderOptions::full(doc),
    };
    let mut renderer = state.renderer.lock().unwrap_or_else(|e| e.into_inner());
    renderer.render_with(doc, &assets, &opts).map_err(err)
}
