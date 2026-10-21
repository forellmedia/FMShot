//! Dienste rund um gespeicherte Aufnahmen: Ablauf nach der Aufnahme,
//! Auto-Export, Aufbewahrung, Texterkennung für die Suche, Diagnose und
//! „Fehler melden“.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::mpsc::{self, Sender};
use std::time::Duration;

use fmshot_core::RgbaImage;
use fmshot_library::{CaptureQuery, CaptureRecord, Library};
use fmshot_overlay::{PreviewAction, PreviewPlacement};
use fmshot_settings::{AfterCapture, ImageFormat, PreviewPosition, export_file_name};
use serde::{Deserialize, Serialize};
use specta::Type;
use tauri::{AppHandle, Emitter, Manager};

use crate::capture::{CaptureState, Mode};
use crate::i18n::t;
use crate::settings;

/// Dienste, die im Hintergrund laufen.
pub struct Services {
    ocr_wakeup: Mutex<Option<Sender<()>>>,
    timings: Mutex<VecDeque<String>>,
}

impl Services {
    pub fn new() -> Self {
        Self {
            ocr_wakeup: Mutex::new(None),
            timings: Mutex::new(VecDeque::new()),
        }
    }
}

/// Messwert merken (Diagnose in den Einstellungen) und ins Log schreiben.
pub fn timing(app: &AppHandle, text: String) {
    log::info!("{text}");
    let s = app.state::<Services>();
    let mut t = s.timings.lock().unwrap_or_else(|e| e.into_inner());
    t.push_front(text);
    t.truncate(30);
}

/// Hintergrund-Threads starten: Texterkennung für die Suche, Aufbewahrung.
pub fn start(app: &AppHandle) {
    let (tx, rx) = mpsc::channel::<()>();
    *app.state::<Services>()
        .ocr_wakeup
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(tx);

    let ocr_app = app.clone();
    std::thread::spawn(move || {
        loop {
            index_pending(&ocr_app);
            // Auf neue Aufnahmen warten, spätestens alle 10 Minuten nachsehen.
            match rx.recv_timeout(Duration::from_secs(600)) {
                Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => {}
                Err(mpsc::RecvTimeoutError::Disconnected) => break,
            }
            // Mehrere Aufnahmen kurz hintereinander zusammenfassen.
            std::thread::sleep(Duration::from_secs(2));
            while rx.try_recv().is_ok() {}
        }
    });

    let purge_app = app.clone();
    std::thread::spawn(move || {
        loop {
            purge(&purge_app);
            std::thread::sleep(Duration::from_secs(6 * 3600));
        }
    });
}

/// Erkennt Text in Aufnahmen, die noch nicht durchsuchbar sind.
fn index_pending(app: &AppHandle) {
    let state = app.state::<CaptureState>();
    let Some(ocr) = state.ocr.as_ref() else {
        return;
    };
    loop {
        let ids = match state.library.pending_ocr(10) {
            Ok(ids) if !ids.is_empty() => ids,
            _ => return,
        };
        for id in ids {
            let text = state
                .library
                .read_original(&id)
                .ok()
                .and_then(|b| fmshot_render::decode_png(&b).ok())
                .and_then(|img| ocr.recognize(&img).ok())
                .map(|blocks| fmshot_ocr::join_text(&blocks))
                .unwrap_or_default();
            if let Err(e) = state.library.set_ocr_text(&id, &text) {
                log::error!("Suchindex {id}: {e}");
                return;
            }
        }
    }
}

fn purge(app: &AppHandle) {
    let Some(days) = settings::get(app).retention_days else {
        return;
    };
    let before = now_ms() - days as i64 * 86_400_000;
    match app.state::<CaptureState>().library.purge_older_than(before) {
        Ok(0) => {}
        Ok(n) => {
            log::info!("Aufbewahrung: {n} Aufnahmen gelöscht");
            let _ = app.emit("library-changed", ());
        }
        Err(e) => log::error!("Aufbewahrung: {e}"),
    }
}

fn now_ms() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

/// Neue Aufnahme in Suche und Galerie bringen – ohne die Schritte nach
/// einer Aufnahme (z. B. für wiederhergestellte Videos).
pub fn index_capture(app: &AppHandle, rec: &CaptureRecord) {
    if let Some(tx) = app
        .state::<Services>()
        .ocr_wakeup
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .as_ref()
    {
        let _ = tx.send(());
    }
    crate::vision::wake(app);
    let _ = app.emit("capture-saved", rec.id.clone());
}

/// Nach jeder gespeicherten Aufnahme (läuft im Hintergrund-Thread).
pub fn after_capture(app: &AppHandle, rec: &CaptureRecord, image: &RgbaImage, mode: Mode) {
    let settings = settings::get(app);
    let library = app.state::<CaptureState>().library.clone();

    // Videos nicht: Sie sind gross und werden nur auf Wunsch zusammengesetzt.
    if settings.auto_export.enabled
        && !rec.is_video()
        && let Err(e) = export(app, &library, rec, Some(image))
    {
        log::error!("Auto-Export: {e}");
    }
    index_capture(app, rec);

    // Scroll-Aufnahmen will man fast immer gleich ansehen.
    let after = if matches!(mode, Mode::Scroll | Mode::ScrollAuto) {
        AfterCapture::Editor
    } else {
        settings.after_capture
    };
    match after {
        AfterCapture::Editor => {
            let _ = app.emit("open-capture", rec.id.clone());
            let a = app.clone();
            let _ = app.run_on_main_thread(move || crate::tray::show_main_window(&a));
        }
        AfterCapture::Clipboard => {
            let msg = t("capture.copied", &[]);
            let _ = app.run_on_main_thread(move || fmshot_overlay::show_toast(&msg));
        }
        AfterCapture::Preview => show_preview(app, rec, image, settings.preview_position),
    }
}

fn show_preview(
    app: &AppHandle,
    rec: &CaptureRecord,
    image: &RgbaImage,
    position: PreviewPosition,
) {
    let thumb = std::sync::Arc::new(fmshot_library::thumbnail(image, 520));
    let placement = match position {
        PreviewPosition::Corner => PreviewPlacement::Corner,
        PreviewPosition::Mouse => app
            .state::<CaptureState>()
            .capturer
            .cursor_position()
            .map(|(x, y)| PreviewPlacement::At(x as f64, y as f64))
            .unwrap_or(PreviewPlacement::Corner),
    };
    // Ein Video wird als Video in andere Apps gezogen.
    let file = if rec.is_video() {
        rec.video_path()
    } else {
        rec.original_path()
    }
    .to_string_lossy()
    .into_owned();
    let labels = [
        t("preview.copy", &[]),
        t("preview.save", &[]),
        t("preview.edit", &[]),
        t("preview.text", &[]),
    ];
    let (app2, id) = (app.clone(), rec.id.clone());
    let _ = app.run_on_main_thread(move || {
        let app3 = app2.clone();
        let labels_ref = [
            labels[0].as_str(),
            labels[1].as_str(),
            labels[2].as_str(),
            labels[3].as_str(),
        ];
        let result = fmshot_overlay::show_preview(
            thumb,
            &file,
            placement,
            labels_ref,
            Box::new(move |action| preview_action(&app3, &id, action)),
        );
        if let Err(e) = result {
            log::error!("Vorschau: {e}");
        }
    });
}

fn preview_action(app: &AppHandle, id: &str, action: PreviewAction) {
    let (app, id) = (app.clone(), id.to_string());
    match action {
        PreviewAction::Edit => {
            let _ = app.emit("open-capture", id);
            crate::tray::show_main_window(&app);
        }
        PreviewAction::Close | PreviewAction::Dragged => {}
        PreviewAction::Copy => {
            std::thread::spawn(move || {
                let msg = match copy_flattened(&app, &id) {
                    Ok(()) => t("editor.copied", &[]),
                    Err(e) => e,
                };
                let _ = app.run_on_main_thread(move || fmshot_overlay::show_toast(&msg));
            });
        }
        PreviewAction::Save => {
            std::thread::spawn(move || {
                let library = app.state::<CaptureState>().library.clone();
                let msg = match library
                    .get(&id)
                    .map_err(|e| e.to_string())
                    .and_then(|rec| export(&app, &library, &rec, None))
                {
                    Ok(path) => t("preview.saved", &[("path", &path.to_string_lossy())]),
                    Err(e) => e,
                };
                let _ = app.run_on_main_thread(move || fmshot_overlay::show_toast(&msg));
            });
        }
        PreviewAction::Text => {
            std::thread::spawn(move || {
                let state = app.state::<CaptureState>();
                let msg = match (state.ocr.as_ref(), state.library.read_original(&id)) {
                    (Some(ocr), Ok(bytes)) => fmshot_render::decode_png(&bytes)
                        .map_err(|e| e.to_string())
                        .and_then(|img| ocr.recognize(&img).map_err(|e| e.to_string()))
                        .and_then(|blocks| {
                            let text = fmshot_ocr::join_text(&blocks);
                            if text.is_empty() {
                                return Ok(t("ocr.nothing", &[]));
                            }
                            fmshot_clipboard::copy_text(&text).map_err(|e| e.to_string())?;
                            Ok(t(
                                "ocr.copied",
                                &[("count", &text.chars().count().to_string())],
                            ))
                        })
                        .unwrap_or_else(|e| e),
                    _ => t("ocr.unavailable", &[]),
                };
                let _ = app.run_on_main_thread(move || fmshot_overlay::show_toast(&msg));
            });
        }
    }
}

/// Fertiges Bild (mit Markierungen) einer Aufnahme.
pub fn flattened(app: &AppHandle, library: &Library, id: &str) -> Result<RgbaImage, String> {
    let doc = library.load_document(id).map_err(|e| e.to_string())?;
    crate::editor::render_native(app, library, id, &doc)
}

fn copy_flattened(app: &AppHandle, id: &str) -> Result<(), String> {
    let library = app.state::<CaptureState>().library.clone();
    let rec = library.get(id).map_err(|e| e.to_string())?;
    if rec.is_video() {
        crate::video_edit::ensure_current(app, id)?;
        return fmshot_clipboard::copy_file(&rec.video_path()).map_err(|e| e.to_string());
    }
    let doc = library.load_document(id).map_err(|e| e.to_string())?;
    let img = flattened(app, &library, id)?;
    fmshot_clipboard::copy_image(&img, doc.pixel_ratio).map_err(|e| e.to_string())
}

/// Schreibt ein Bild in den Export-Ordner (Videos nicht). Bei wiederholtem
/// Export (z. B. nach Änderungen im Editor) wird dieselbe Datei überschrieben.
pub fn export(
    app: &AppHandle,
    library: &Library,
    rec: &CaptureRecord,
    image: Option<&RgbaImage>,
) -> Result<PathBuf, String> {
    let settings = settings::get(app).auto_export;
    let path = match library.export_path(&rec.id).ok().flatten() {
        Some(p) => p,
        None => {
            let folder = match &settings.folder {
                Some(f) => PathBuf::from(f),
                None => app.path().desktop_dir().map_err(|e| e.to_string())?,
            };
            let name = export_file_name(&settings.pattern, &rec.id, &rec.mode, settings.format);
            unique(&folder.join(name))
        }
    };
    let owned;
    let img = match image {
        Some(i) => i,
        None => {
            owned = flattened(app, library, &rec.id)?;
            &owned
        }
    };
    let bytes = if path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("jpg"))
    {
        fmshot_library::encode_jpeg(img, 90)?
    } else {
        fmshot_library::encode_png(img, rec.scale_factor).map_err(|e| e.to_string())?
    };
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    }
    std::fs::write(&path, bytes).map_err(|e| e.to_string())?;
    library
        .set_export_path(&rec.id, &path)
        .map_err(|e| e.to_string())?;
    let _ = settings.format == ImageFormat::Png;
    Ok(path)
}

/// Hängt „ (2)“, „ (3)“ … an, falls die Datei schon existiert.
fn unique(path: &Path) -> PathBuf {
    if !path.exists() {
        return path.to_path_buf();
    }
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("FMShot");
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("png");
    (2..)
        .map(|n| path.with_file_name(format!("{stem} ({n}).{ext}")))
        .find(|p| !p.exists())
        .unwrap_or_else(|| path.to_path_buf())
}

/// Nach dem Speichern im Editor: exportierte Datei aktualisieren.
pub fn refresh_export(app: &AppHandle, id: &str) {
    let library = app.state::<CaptureState>().library.clone();
    if !settings::get(app).auto_export.enabled {
        return;
    }
    if let Ok(Some(_)) = library.export_path(id)
        && let Ok(rec) = library.get(id)
        && !rec.is_video()
        && let Err(e) = export(app, &library, &rec, None)
    {
        log::error!("Auto-Export aktualisieren: {e}");
    }
}

// --- Commands ---------------------------------------------------------------

#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct Diagnostics {
    pub version: String,
    pub platform: String,
    pub library: String,
    pub log_file: String,
    /// Letzte Messwerte, neueste zuerst.
    pub timings: Vec<String>,
}

#[tauri::command]
#[specta::specta]
pub fn get_diagnostics(app: AppHandle) -> Diagnostics {
    let s = app.state::<Services>();
    Diagnostics {
        version: app.package_info().version.to_string(),
        platform: format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
        library: app
            .state::<CaptureState>()
            .library
            .root()
            .to_string_lossy()
            .into_owned(),
        log_file: log_file(&app)
            .map(|p| p.to_string_lossy().into_owned())
            .unwrap_or_default(),
        timings: s
            .timings
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .iter()
            .cloned()
            .collect(),
    }
}

/// Filter der Galerie (siehe `fmshot_library::CaptureQuery`).
#[derive(Debug, Clone, Deserialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GalleryQuery {
    pub text: String,
    /// Unix-Millisekunden, inklusive.
    pub from_ms: Option<f64>,
    /// Unix-Millisekunden, exklusive.
    pub to_ms: Option<f64>,
    pub mode: Option<String>,
    pub starred_only: bool,
    pub oldest_first: bool,
    pub limit: u32,
    /// Schnellfilter, z. B. `animals` (siehe `content_groups`).
    pub group: Option<String>,
}

/// Eine Aufnahme in der Galerie mit Stichwörtern und Grund des Treffers.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GalleryItem {
    pub capture: crate::editor::CaptureInfo,
    /// Stichwörter (Schlüssel für `tag.<key>`), bestes zuerst.
    pub tags: Vec<String>,
    /// Warum die Aufnahme zur Suche passt: `text` (erkannter Text),
    /// `content` (Bildinhalt) oder ein Stichwort-Schlüssel.
    pub matched: Vec<String>,
}

/// Ein Schnellfilter mit der Zahl passender Aufnahmen.
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GroupCount {
    /// Schlüssel für `group.<key>`.
    pub group: String,
    pub count: u32,
}

/// Ergebnis der Galerie: Aufnahmen und die Schnellfilter, die darin
/// vorkommen (ohne den gewählten Schnellfilter gezählt).
#[derive(Debug, Clone, Serialize, Type)]
#[serde(rename_all = "camelCase")]
pub struct GalleryResult {
    pub items: Vec<GalleryItem>,
    pub groups: Vec<GroupCount>,
}

/// Galerie: Aufnahmen nach Text (erkannt oder Bildinhalt), Zeitraum, Art,
/// Stern und Schnellfilter.
#[tauri::command]
#[specta::specta]
pub async fn query_captures(app: AppHandle, query: GalleryQuery) -> Result<GalleryResult, String> {
    tauri::async_runtime::spawn_blocking(move || gallery(&app, query))
        .await
        .map_err(|e| e.to_string())?
}

fn gallery(app: &AppHandle, query: GalleryQuery) -> Result<GalleryResult, String> {
    let library = app.state::<CaptureState>().library.clone();
    let has_text = !query.text.trim().is_empty();
    let content_ids = if has_text {
        crate::vision::search(app, &query.text)
    } else {
        Vec::new()
    };
    let text_tags = if has_text {
        crate::vision::tags_for_text(&query.text)
    } else {
        Vec::new()
    };
    let q = CaptureQuery {
        text: query.text.clone(),
        content_ids: content_ids.clone(),
        text_tags: text_tags.clone(),
        any_tags: query
            .group
            .as_deref()
            .map(crate::vision::group_tags)
            .unwrap_or_default(),
        from_ms: query.from_ms.map(|v| v as i64),
        to_ms: query.to_ms.map(|v| v as i64),
        mode: query.mode,
        starred_only: query.starred_only,
        oldest_first: query.oldest_first,
        limit: query.limit,
    };
    let records = library.query(&q).map_err(|e| e.to_string())?;
    let groups = if crate::settings::get(app).content_search {
        let all = CaptureQuery {
            any_tags: Vec::new(),
            ..q.clone()
        };
        crate::vision::group_counts(&library.tagged(&all).map_err(|e| e.to_string())?)
            .into_iter()
            .map(|(group, count)| GroupCount { group, count })
            .collect()
    } else {
        Vec::new()
    };
    let ids: Vec<String> = records.iter().map(|r| r.id.clone()).collect();
    let mut tags = library.tags(&ids).map_err(|e| e.to_string())?;
    let by_text: std::collections::HashSet<String> = if has_text {
        library
            .text_matches(&query.text)
            .map_err(|e| e.to_string())?
            .into_iter()
            .collect()
    } else {
        Default::default()
    };
    let by_content: std::collections::HashSet<&String> = content_ids.iter().collect();
    let items = records
        .iter()
        .map(|r| {
            let tags = tags.remove(&r.id).unwrap_or_default();
            let mut matched = Vec::new();
            if by_text.contains(&r.id) {
                matched.push("text".to_string());
            }
            matched.extend(tags.iter().filter(|t| text_tags.contains(t)).cloned());
            if by_content.contains(&r.id) && matched.is_empty() {
                matched.push("content".to_string());
            }
            GalleryItem {
                capture: crate::editor::CaptureInfo::from(r),
                tags,
                matched,
            }
        })
        .collect();
    Ok(GalleryResult { items, groups })
}

/// Volltextsuche über den erkannten Text.
#[tauri::command]
#[specta::specta]
pub fn search_captures(
    app: AppHandle,
    query: String,
) -> Result<Vec<crate::editor::CaptureInfo>, String> {
    Ok(app
        .state::<CaptureState>()
        .library
        .search(&query, 500)
        .map_err(|e| e.to_string())?
        .iter()
        .map(crate::editor::CaptureInfo::from)
        .collect())
}

fn log_file(app: &AppHandle) -> Option<PathBuf> {
    let dir = app.path().app_log_dir().ok()?;
    let name = format!("{}.log", app.package_info().name);
    Some(dir.join(name))
}

/// „Fehler melden“: öffnet ein vorausgefülltes GitHub-Issue im Browser.
/// Der Nutzer sieht alles und schickt es selbst ab – ohne Screenshots.
#[tauri::command]
#[specta::specta]
pub fn report_bug(app: AppHandle) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let d = get_diagnostics(app.clone());
    let log_tail = log_file(&app)
        .and_then(|p| std::fs::read_to_string(p).ok())
        .map(|s| {
            let lines: Vec<&str> = s.lines().collect();
            lines[lines.len().saturating_sub(40)..].join("\n")
        })
        .unwrap_or_default();
    let body = format!(
        "### Was ist passiert?\n\n\n### Was hättest du erwartet?\n\n\n### Umgebung\n- FMShot {}\n- {}\n\n### Log (letzte Zeilen)\n```text\n{}\n```\n",
        d.version, d.platform, log_tail
    );
    let url = format!(
        "https://github.com/forellmedia/FMShot/issues/new?labels=fehler&title={}&body={}",
        urlencode("Fehler: "),
        urlencode(&body.chars().take(5500).collect::<String>())
    );
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len() * 3);
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(b as char)
            }
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn url_kodierung() {
        assert_eq!(urlencode("a b/ü"), "a%20b%2F%C3%BC");
    }

    #[test]
    fn eindeutiger_dateiname() {
        let tmp = std::env::temp_dir().join(format!("fmshot-test-{}", std::process::id()));
        std::fs::create_dir_all(&tmp).unwrap();
        let p = tmp.join("Bild.png");
        assert_eq!(unique(&p), p);
        std::fs::write(&p, b"x").unwrap();
        assert_eq!(unique(&p), tmp.join("Bild (2).png"));
        std::fs::remove_dir_all(&tmp).unwrap();
    }
}
