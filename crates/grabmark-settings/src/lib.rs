//! Einstellungen von Grabmark. Gespeichert als `settings.json` im
//! Konfigurationsordner der App; fehlende Felder bekommen Standardwerte,
//! damit ältere Dateien nach Updates weiter funktionieren.

use std::collections::BTreeMap;
use std::fs;
use std::io;
use std::path::Path;

use grabmark_core::{
    CameraFormat, VideoEncoding, WebcamBackground, WebcamFraming, WebcamLayout, WebcamShape,
};
use grabmark_hotkeys::{Action, Platform, Shortcut, conflicts, default_bindings};
use serde::{Deserialize, Serialize};

/// Was nach einer Aufnahme passiert (Produkt: A, B, C).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum AfterCapture {
    /// Editor öffnen.
    Editor,
    /// Nur in die Zwischenablage.
    Clipboard,
    /// Schnellzugriff-Vorschau.
    Preview,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum PreviewPosition {
    /// Dort, wo die Auswahl losgelassen wurde.
    Mouse,
    /// Unten rechts auf dem Hauptmonitor.
    Corner,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum ScrollMode {
    Manual,
    Auto,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum ImageFormat {
    Png,
    Jpg,
}

/// Was bei einer Video-Aufnahme aufgenommen wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum VideoTarget {
    /// Bereich aufziehen.
    Region,
    /// Fenster anklicken.
    Window,
    /// Ganzer Bildschirm unter der Maus.
    Screen,
}

/// Live zeichnen während der Aufnahme: Wann es wieder aufhört.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum DrawMode {
    /// Bleibt an, bis es ausgeschaltet wird (Kürzel, Knopf, Esc, Rechtsklick).
    #[default]
    Sticky,
    /// Nach jeder Markierung zurück: Klicks gehen wieder an die Apps.
    Once,
}

/// Einstellungen der Video-Aufnahme (ADR 0011).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct VideoSettings {
    pub target: VideoTarget,
    pub microphone: bool,
    /// `None` = Standard-Mikrofon des Systems.
    pub microphone_id: Option<String>,
    pub system_audio: bool,
    /// 3-Sekunden-Countdown vor dem Start.
    pub countdown: bool,
    /// Bilder pro Sekunde: 24, 25, 30, 50 oder 60.
    pub fps: u32,
    /// Webcam zu Beginn eingeschaltet (während der Aufnahme umschaltbar).
    pub webcam: bool,
    /// `None` = Standard-Kamera des Systems.
    pub camera_id: Option<String>,
    /// Form, Grösse, Rahmen und Schatten der Webcam; die Lage gibt die
    /// Live-Blase vor.
    pub webcam_layout: WebcamLayout,
    /// Aufnahme nur mit der Webcam (Tab *Webcam*). Kamera, Mikrofon und
    /// Countdown gelten wie oben.
    pub camera: CameraSettings,
    /// Auflösung, Codec und Qualität des fertigen Videos.
    pub encoding: VideoEncoding,
    pub draw_mode: DrawMode,
}

/// Einstellungen für Videos nur mit der Webcam.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct CameraSettings {
    pub format: CameraFormat,
    pub framing: WebcamFraming,
    pub follow: bool,
    pub background: WebcamBackground,
    pub mirror: bool,
}

impl Default for CameraSettings {
    fn default() -> Self {
        Self {
            format: CameraFormat::Landscape,
            framing: WebcamFraming::Full,
            follow: true,
            background: WebcamBackground::Original,
            mirror: true,
        }
    }
}

impl CameraSettings {
    /// Webcam-Layout fürs Dokument (Rahmen, Schatten usw. gelten hier
    /// nicht; die Form gibt das Seitenverhältnis für Blase und Kopf vor).
    pub fn layout(&self) -> WebcamLayout {
        WebcamLayout {
            shape: match self.format {
                CameraFormat::Landscape => WebcamShape::Landscape,
                CameraFormat::Portrait => WebcamShape::Portrait,
                CameraFormat::Square => WebcamShape::Square,
            },
            corner_radius: 0.0,
            framing: self.framing,
            follow: self.follow,
            background: self.background,
            mirror: self.mirror,
            border: None,
            shadow: false,
            ..WebcamLayout::default()
        }
    }
}

impl Default for VideoSettings {
    fn default() -> Self {
        Self {
            target: VideoTarget::Region,
            microphone: true,
            microphone_id: None,
            system_audio: true,
            countdown: true,
            fps: 30,
            webcam: false,
            camera_id: None,
            webcam_layout: WebcamLayout::default(),
            camera: CameraSettings::default(),
            encoding: VideoEncoding::default(),
            draw_mode: DrawMode::default(),
        }
    }
}

/// Jede Aufnahme zusätzlich als Datei in einen Ordner schreiben.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct AutoExport {
    pub enabled: bool,
    /// `None` = Schreibtisch.
    pub folder: Option<String>,
    /// Platzhalter: `{date}`, `{time}`, `{id}`, `{mode}`.
    pub pattern: String,
    pub format: ImageFormat,
}

impl Default for AutoExport {
    fn default() -> Self {
        Self {
            enabled: false,
            folder: None,
            pattern: "Grabmark {date} {time}".into(),
            format: ImageFormat::Png,
        }
    }
}

/// Warngrenzen für den Speicherplatz in GB; 0 = nie warnen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct StorageLimits {
    /// Bibliothek (Aufnahmen, Videos, Index).
    pub library_gb: u32,
    /// Dateien, die der Auto-Export geschrieben hat.
    pub export_gb: u32,
}

impl Default for StorageLimits {
    fn default() -> Self {
        Self {
            library_gb: 10,
            export_gb: 10,
        }
    }
}

impl StorageLimits {
    /// Grenze in Bytes; `None` = keine.
    pub fn library_bytes(&self) -> Option<u64> {
        (self.library_gb > 0).then(|| u64::from(self.library_gb) * 1_000_000_000)
    }

    pub fn export_bytes(&self) -> Option<u64> {
        (self.export_gb > 0).then(|| u64::from(self.export_gb) * 1_000_000_000)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct Settings {
    /// `None` = Sprache des Systems; sonst `de` oder `en`.
    pub language: Option<String>,
    pub after_capture: AfterCapture,
    pub preview_position: PreviewPosition,
    /// Aufnahme immer sofort in die Zwischenablage legen.
    pub copy_to_clipboard: bool,
    pub scroll_mode: ScrollMode,
    /// Tastenkürzel im Textformat (`Primary+Shift+2`); leer = keines.
    pub hotkeys: BTreeMap<String, String>,
    /// macOS: ⌘⇧3/4/5 statt der Ausweichkürzel ⌘⌥3/4/5.
    pub replace_mac_system_shortcuts: bool,
    pub auto_export: AutoExport,
    /// Aufnahmen nach so vielen Tagen löschen (ausser ⭐); `None` = nie.
    pub retention_days: Option<u32>,
    /// Ab welcher Grösse Grabmark zum Aufräumen auffordert.
    pub storage: StorageLimits,
    /// Log-Dateien nach so vielen Tagen löschen (10, 30, 60 oder 360).
    pub log_retention_days: u32,
    /// Format beim Speichern aus dem Editor (⌘S).
    pub save_format: ImageFormat,
    /// Bildinhalt erkennen, damit die Suche auch „Wald“ oder „Hund“ findet
    /// (ADR 0010). Läuft im Hintergrund.
    pub content_search: bool,
    pub video: VideoSettings,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            language: None,
            after_capture: AfterCapture::Preview,
            preview_position: PreviewPosition::Mouse,
            copy_to_clipboard: true,
            scroll_mode: ScrollMode::Manual,
            hotkeys: BTreeMap::new(),
            replace_mac_system_shortcuts: false,
            auto_export: AutoExport::default(),
            retention_days: Some(90),
            storage: StorageLimits::default(),
            log_retention_days: 30,
            save_format: ImageFormat::Png,
            content_search: true,
            video: VideoSettings::default(),
        }
    }
}

/// Schlüssel einer Aktion in `hotkeys`.
pub fn action_key(action: Action) -> &'static str {
    match action {
        Action::CaptureScreen => "capture_screen",
        Action::CaptureRegion => "capture_region",
        Action::CaptureWindow => "capture_window",
        Action::CaptureScroll => "capture_scroll",
        Action::CaptureText => "capture_text",
        Action::CaptureVideo => "capture_video",
        Action::CaptureWebcam => "capture_webcam",
        Action::RecordDraw => "record_draw",
        Action::RecordPause => "record_pause",
        Action::OpenGallery => "open_gallery",
    }
}

#[derive(Debug, thiserror::Error)]
pub enum SettingsError {
    #[error("Einstellungen konnten nicht gelesen/geschrieben werden: {0}")]
    Io(#[from] io::Error),
    #[error("Einstellungen sind beschädigt: {0}")]
    Json(#[from] serde_json::Error),
}

impl Settings {
    /// Lädt die Einstellungen; fehlt die Datei, gelten die Standardwerte.
    pub fn load(path: &Path) -> Result<Self, SettingsError> {
        match fs::read(path) {
            Ok(bytes) => Ok(serde_json::from_slice(&bytes)?),
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(Self::default()),
            Err(e) => Err(e.into()),
        }
    }

    pub fn save(&self, path: &Path) -> Result<(), SettingsError> {
        if let Some(dir) = path.parent() {
            fs::create_dir_all(dir)?;
        }
        let tmp = path.with_extension("json.tmp");
        fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        fs::rename(tmp, path)?;
        Ok(())
    }

    /// Die tatsächlich gültigen Tastenkürzel: Standard + eigene Belegung.
    /// Eine leere eigene Belegung schaltet das Kürzel ab.
    pub fn effective_hotkeys(&self, platform: Platform) -> Vec<(Action, Shortcut)> {
        let defaults = default_bindings(platform, self.replace_mac_system_shortcuts);
        let mut result = Vec::new();
        for action in Action::ALL {
            match self.hotkeys.get(action_key(action)) {
                Some(s) if s.is_empty() => {}
                Some(s) => {
                    if let Ok(sc) = s.parse::<Shortcut>() {
                        result.push((action, sc));
                    }
                }
                None => result.extend(defaults.iter().filter(|(a, _)| *a == action).copied()),
            }
        }
        result
    }

    /// Doppelt belegte Kürzel (für die Warnung in den Einstellungen).
    pub fn hotkey_conflicts(&self, platform: Platform) -> Vec<(Action, Action)> {
        conflicts(&self.effective_hotkeys(platform))
            .into_iter()
            .map(|(a, b, _)| (a, b))
            .collect()
    }
}

/// Dateiname für den Auto-Export aus dem Muster.
pub fn export_file_name(pattern: &str, id: &str, mode: &str, format: ImageFormat) -> String {
    // ID: JJJJMMTT-hhmmss-ms-n
    let date = id
        .get(0..8)
        .map(|d| format!("{}-{}-{}", &d[0..4], &d[4..6], &d[6..8]));
    let time = id
        .get(9..15)
        .map(|t| format!("{}.{}.{}", &t[0..2], &t[2..4], &t[4..6]));
    let name = pattern
        .replace("{date}", date.as_deref().unwrap_or(""))
        .replace("{time}", time.as_deref().unwrap_or(""))
        .replace("{id}", id)
        .replace("{mode}", mode);
    // Zeichen, die in Dateinamen auf einem der Systeme verboten sind.
    let clean: String = name
        .chars()
        .map(|c| if "/\\:*?\"<>|".contains(c) { '-' } else { c })
        .collect();
    let clean = clean.trim();
    let base = if clean.is_empty() { id } else { clean };
    let ext = match format {
        ImageFormat::Png => "png",
        ImageFormat::Jpg => "jpg",
    };
    format!("{base}.{ext}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fehlende_datei_gibt_standard() {
        let tmp = tempfile::tempdir().unwrap();
        let s = Settings::load(&tmp.path().join("settings.json")).unwrap();
        assert_eq!(s, Settings::default());
        assert_eq!(s.after_capture, AfterCapture::Preview);
    }

    #[test]
    fn speichern_und_laden() {
        let tmp = tempfile::tempdir().unwrap();
        let path = tmp.path().join("cfg/settings.json");
        let mut s = Settings::default();
        s.auto_export.enabled = true;
        s.retention_days = Some(30);
        s.hotkeys
            .insert("capture_region".into(), "Primary+Alt+R".into());
        s.save(&path).unwrap();
        assert_eq!(Settings::load(&path).unwrap(), s);
    }

    #[test]
    fn alte_datei_mit_wenigen_feldern() {
        let s: Settings = serde_json::from_str(r#"{"after_capture":"editor"}"#).unwrap();
        assert_eq!(s.after_capture, AfterCapture::Editor);
        assert!(s.copy_to_clipboard);
        assert_eq!(s.retention_days, Some(90));
        assert_eq!(s.storage.library_bytes(), Some(10_000_000_000));
    }

    #[test]
    fn speichergrenze_null_heisst_nie() {
        let limits = StorageLimits {
            library_gb: 0,
            export_gb: 2,
        };
        assert_eq!(limits.library_bytes(), None);
        assert_eq!(limits.export_bytes(), Some(2_000_000_000));
    }

    #[test]
    fn eigene_kuerzel_ersetzen_standard() {
        let mut s = Settings::default();
        s.hotkeys
            .insert("capture_region".into(), "Primary+Alt+R".into());
        s.hotkeys.insert("capture_text".into(), String::new());
        let hk = s.effective_hotkeys(Platform::MacOs);
        let region: Vec<_> = hk
            .iter()
            .filter(|(a, _)| *a == Action::CaptureRegion)
            .collect();
        assert_eq!(region.len(), 1);
        assert_eq!(region[0].1.to_string(), "Primary+Alt+R");
        assert!(!hk.iter().any(|(a, _)| *a == Action::CaptureText));
    }

    #[test]
    fn konflikt_wird_gemeldet() {
        let mut s = Settings::default();
        s.hotkeys
            .insert("capture_text".into(), "Primary+Shift+2".into());
        assert_eq!(
            s.hotkey_conflicts(Platform::MacOs),
            vec![(Action::CaptureRegion, Action::CaptureText)]
        );
    }

    #[test]
    fn dateiname_aus_muster() {
        let name = export_file_name(
            "Grabmark {date} {time}",
            "20261016-224512-083-0",
            "region",
            ImageFormat::Png,
        );
        assert_eq!(name, "Grabmark 2026-10-16 22.45.12.png");
        assert_eq!(
            export_file_name(
                "{mode}/{id}",
                "20261016-224512-083-0",
                "region",
                ImageFormat::Jpg
            ),
            "region-20261016-224512-083-0.jpg"
        );
    }
}
