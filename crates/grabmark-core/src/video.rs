//! Video-Teil eines Dokuments (ADR 0011): Webcam-Layout und Abschnitte.
//! Gerechnet wird damit in `grabmark-video`; hier stehen nur die Daten.

use serde::{Deserialize, Serialize};

use crate::geometry::{Color, Rect};

/// Alles, was ein Video über die aufgenommenen Spuren hinaus ausmacht.
/// Liegt in `<id>.json` neben den Spuren und bleibt bis zum Export änderbar.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct VideoDoc {
    /// Länge ohne Pausen.
    pub duration_ms: u32,
    /// Bilder pro Sekunde der Aufnahme (Export übernimmt sie).
    #[serde(default = "default_fps")]
    pub fps: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub webcam: Option<WebcamTrack>,
    /// Herausgeschnittene Stellen (Zeit der Aufnahme), sortiert und ohne
    /// Überlappung. Trimmen = Schnitt am Anfang bzw. Ende (Etappe 4).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub cuts: Vec<TimeRange>,
    /// Lautstärke je Tonspur (Etappe 4).
    #[serde(default)]
    pub audio: AudioMix,
    /// Ein- und Ausblenden am Anfang und Ende des fertigen Videos (Bild und
    /// Ton).
    #[serde(default, skip_serializing_if = "Fade::is_none")]
    pub fade: Fade,
    /// Eigener Mauszeiger und Klicks (Etappe 5).
    #[serde(default)]
    pub cursor: CursorStyle,
    /// Zoom-Abschnitte auf den Bildschirm (Zeit der Aufnahme, Etappe 5).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub zooms: Vec<ZoomSection>,
    /// Wann Ebenen (Pfeile, Text …) zu sehen sind (Etappe 6). Ebenen ohne
    /// Eintrag sind immer zu sehen.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub layer_times: Vec<LayerTime>,
    /// Gelöschte Klicks (Zeit der Aufnahme beim Tastendruck): kein Kreis,
    /// kein Auto-Zoom. Die Mausspur selbst bleibt unverändert.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hidden_clicks: Vec<u32>,
    /// Nur Webcam, ohne Bildschirm: Format des fertigen Videos. Die Webcam
    /// füllt dann das ganze Bild.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub camera_only: Option<CameraFormat>,
}

/// Seitenverhältnis einer Aufnahme nur mit Webcam.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum CameraFormat {
    /// Querformat 16:9.
    #[default]
    Landscape,
    /// Hochformat 9:16 (Reels, Shorts, Stories).
    Portrait,
    /// Quadrat 1:1.
    Square,
}

impl CameraFormat {
    /// Breite durch Höhe.
    pub fn aspect(self) -> f32 {
        match self {
            CameraFormat::Landscape => 16.0 / 9.0,
            CameraFormat::Portrait => 9.0 / 16.0,
            CameraFormat::Square => 1.0,
        }
    }
}

/// Wie das fertige Video kodiert wird. Die Aufnahme selbst bleibt in
/// voller Qualität; dies gilt beim Zusammensetzen (auch nach jeder Änderung
/// im Editor).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct VideoEncoding {
    pub resolution: VideoResolution,
    pub codec: VideoCodec,
    pub quality: VideoQuality,
}

/// Grösse des fertigen Videos: höchstens so viele Pixel auf der kürzeren
/// Seite (nie grösser als die Aufnahme).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum VideoResolution {
    /// Wie aufgenommen.
    #[default]
    Original,
    P2160,
    P1440,
    P1080,
    P720,
}

impl VideoResolution {
    /// Längste erlaubte kürzere Seite; `None` = wie aufgenommen.
    pub fn short_side(self) -> Option<u32> {
        match self {
            VideoResolution::Original => None,
            VideoResolution::P2160 => Some(2160),
            VideoResolution::P1440 => Some(1440),
            VideoResolution::P1080 => Some(1080),
            VideoResolution::P720 => Some(720),
        }
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum VideoCodec {
    /// Läuft überall (Standard).
    #[default]
    H264,
    /// Halb so gross bei gleicher Qualität; ältere Geräte und manche
    /// Webseiten können es nicht abspielen.
    Hevc,
}

/// Datenrate des fertigen Videos.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum VideoQuality {
    /// Halbe Datenrate – zum Verschicken.
    Small,
    #[default]
    Standard,
    /// Doppelte Datenrate – für feine Details und viel Bewegung.
    High,
}

/// Zeitspanne in ms (Zeit der Aufnahme), `start_ms < end_ms`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TimeRange {
    pub start_ms: u32,
    pub end_ms: u32,
}

/// Lautstärke je Tonspur: 0 = stumm bis 1 = wie aufgenommen. Die Punkte
/// verändern sie zusätzlich über die Zeit (Faktor, dazwischen linear).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct AudioMix {
    pub system: f32,
    pub microphone: f32,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub system_keys: Vec<VolumeKey>,
    #[serde(skip_serializing_if = "Vec::is_empty")]
    pub microphone_keys: Vec<VolumeKey>,
}

impl Default for AudioMix {
    fn default() -> Self {
        Self {
            system: 1.0,
            microphone: 1.0,
            system_keys: Vec::new(),
            microphone_keys: Vec::new(),
        }
    }
}

/// Lautstärke-Punkt (Keyframe) einer Tonspur, Zeit der Aufnahme. Vor dem
/// ersten und nach dem letzten Punkt gilt dessen Wert.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct VolumeKey {
    pub t_ms: u32,
    /// 0 = stumm bis 1 = volle Lautstärke der Spur.
    pub volume: f32,
}

/// Ein- und Ausblenden in Bildern des fertigen Videos (0 = aus).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct Fade {
    pub in_frames: u32,
    pub out_frames: u32,
}

impl Fade {
    pub fn is_none(&self) -> bool {
        self.in_frames == 0 && self.out_frames == 0
    }
}

/// Wie Grabmark den Mauszeiger zeichnet (aus `<id>_input.json`).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct CursorStyle {
    pub visible: bool,
    /// Grösse als Vielfaches der normalen Zeigergrösse (1 bis 3).
    pub size: f32,
    /// Klicks als Kreis hervorheben.
    pub highlight_clicks: bool,
    pub click_color: Color,
}

impl Default for CursorStyle {
    fn default() -> Self {
        Self {
            visible: true,
            size: 1.5,
            highlight_clicks: true,
            click_color: Color::ACCENT,
        }
    }
}

/// Der Bildschirm wird in dieser Zeit vergrössert (weich hinein und hinaus).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct ZoomSection {
    pub start_ms: u32,
    pub end_ms: u32,
    /// Mitte als Anteil des Bildschirms (0 bis 1).
    pub x: f32,
    pub y: f32,
    /// Vergrösserung (1 bis 4).
    pub scale: f32,
}

/// Sichtbarkeit einer Ebene des Dokuments im Video.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct LayerTime {
    pub layer: u32,
    pub start_ms: u32,
    pub end_ms: u32,
}

/// Mausspur (`<id>_input.json`), etwa 60 Proben je Sekunde.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct InputTrack {
    pub samples: Vec<InputSample>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct InputSample {
    /// Zeit im Video (ohne Pausen).
    pub t_ms: u32,
    /// Lage als Anteil des aufgenommenen Bereichs (0 bis 1, darf ausserhalb
    /// liegen).
    pub x: f32,
    pub y: f32,
    /// Maustaste gedrückt.
    pub down: bool,
}

fn default_fps() -> u32 {
    30
}

impl Default for VideoDoc {
    fn default() -> Self {
        Self {
            duration_ms: 0,
            fps: default_fps(),
            webcam: None,
            cuts: Vec::new(),
            audio: AudioMix::default(),
            fade: Fade::default(),
            cursor: CursorStyle::default(),
            zooms: Vec::new(),
            layer_times: Vec::new(),
            hidden_clicks: Vec::new(),
            camera_only: None,
        }
    }
}

/// Webcam-Spur: Lage, Form und wann sie wie zu sehen ist.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct WebcamTrack {
    pub layout: WebcamLayout,
    /// Ab `start_ms` gilt `mode`, sortiert nach Zeit. Vor dem ersten
    /// Abschnitt ist die Webcam ausgeblendet.
    pub sections: Vec<WebcamSection>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct WebcamSection {
    pub start_ms: u32,
    pub mode: WebcamMode,
}

/// Wie Webcam und Bildschirm zueinander stehen.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum WebcamMode {
    /// Webcam klein über dem Bildschirm (nach [`WebcamLayout`]).
    Overlay,
    /// Webcam füllt das Bild, der Bildschirm liegt klein in der Ecke.
    Large,
    /// Nur der Bildschirm.
    Hidden,
}

/// Lage und Aussehen der Webcam über dem Bildschirm.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(default)]
pub struct WebcamLayout {
    pub position: WebcamPosition,
    /// Breite als Anteil der Bildbreite (0.05 bis 0.6).
    pub size: f32,
    pub shape: WebcamShape,
    /// Eckenradius als Anteil der kürzeren Seite (0 bis 0.5). Beim Kreis
    /// ohne Wirkung.
    pub corner_radius: f32,
    /// `None` = ohne Rahmen (wird als `null` gespeichert, sonst gälte der
    /// Standardrahmen).
    pub border: Option<WebcamBorder>,
    pub shadow: bool,
    /// Spiegelbildlich wie in der Live-Blase.
    pub mirror: bool,
    /// Ganzes Kamerabild oder nur der Kopf (Etappe 3).
    pub framing: WebcamFraming,
    /// Beim Kopfausschnitt: dem Kopf weich folgen. Aus = einmal
    /// ausgerichtet, dann fest.
    pub follow: bool,
    pub background: WebcamBackground,
}

impl Default for WebcamLayout {
    fn default() -> Self {
        Self {
            position: WebcamPosition::BottomRight,
            size: 0.2,
            shape: WebcamShape::Circle,
            corner_radius: 0.12,
            border: Some(WebcamBorder::default()),
            shadow: true,
            mirror: true,
            framing: WebcamFraming::Full,
            follow: true,
            background: WebcamBackground::Original,
        }
    }
}

/// Welcher Teil des Kamerabilds zu sehen ist.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum WebcamFraming {
    /// Ganzes Kamerabild (mittig auf die Form zugeschnitten).
    Full,
    /// Ausschnitt um den erkannten Kopf (mit Schultern).
    Head,
}

/// Was mit dem Hintergrund hinter der Person geschieht.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum WebcamBackground {
    Original,
    /// Weichgezeichnet, die Person bleibt scharf.
    Blur,
    /// Entfernt: Über dem Bildschirm schwebt nur die Person.
    Remove,
}

/// Erkannte Gesichter der Webcam (`<id>_faces.json`), etwa 10 je Sekunde.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct FaceTrack {
    pub samples: Vec<FaceSample>,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct FaceSample {
    /// Zeit im Video (ohne Pausen).
    pub t_ms: u32,
    /// Grösstes Gesicht als Anteil des Kamerabilds (0 bis 1, Ursprung oben
    /// links, nicht gespiegelt); `None` = keins erkannt.
    pub face: Option<Rect>,
}

/// Ecke oder frei verschoben.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum WebcamPosition {
    TopLeft,
    TopRight,
    BottomLeft,
    BottomRight,
    /// Mittelpunkt als Anteil von Breite und Höhe (0 bis 1).
    Free {
        x: f32,
        y: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(rename_all = "snake_case")]
pub enum WebcamShape {
    Circle,
    Square,
    /// Querformat 16:9.
    Landscape,
    /// Hochformat 9:16.
    Portrait,
}

impl WebcamShape {
    /// Breite / Höhe.
    pub fn aspect(self) -> f32 {
        match self {
            Self::Circle | Self::Square => 1.0,
            Self::Landscape => 16.0 / 9.0,
            Self::Portrait => 9.0 / 16.0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct WebcamBorder {
    /// Breite als Anteil der kürzeren Seite der Webcam.
    pub width: f32,
    pub color: Color,
}

impl Default for WebcamBorder {
    fn default() -> Self {
        Self {
            width: 0.025,
            color: Color::WHITE,
        }
    }
}

/// Zwischenstand einer laufenden Aufnahme (`<id>_recording.json`): was nicht
/// in den Spuren steht. Nach einem Absturz entsteht daraus wieder das
/// Dokument (ADR 0012). Wird bei jeder Änderung neu geschrieben.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct RecordingCheckpoint {
    pub fps: u32,
    /// Pixel je Punkt (Bildschirm-Skalierung; 1 bei „nur Webcam“).
    pub scale: f32,
    pub camera_only: Option<CameraFormat>,
    /// Startlage der Webcam (aus den Einstellungen).
    pub webcam_layout: WebcamLayout,
    /// Webcam-Abschnitte bis jetzt.
    pub webcam_sections: Vec<WebcamSection>,
    /// Fertige Live-Zeichnungen.
    pub drawings: Vec<CheckpointDrawing>,
}

impl Default for RecordingCheckpoint {
    fn default() -> Self {
        Self {
            fps: default_fps(),
            scale: 1.0,
            camera_only: None,
            webcam_layout: WebcamLayout::default(),
            webcam_sections: Vec::new(),
            drawings: Vec::new(),
        }
    }
}

/// Eine Live-Zeichnung im Zwischenstand: Ebene und wann sie zu sehen ist.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CheckpointDrawing {
    pub kind: crate::LayerKind,
    pub opacity: f32,
    pub start_ms: u32,
    pub end_ms: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout_ueberlebt_json() {
        let doc = VideoDoc {
            duration_ms: 12_000,
            fps: 30,
            webcam: Some(WebcamTrack {
                layout: WebcamLayout {
                    position: WebcamPosition::Free { x: 0.3, y: 0.7 },
                    ..WebcamLayout::default()
                },
                sections: vec![
                    WebcamSection {
                        start_ms: 0,
                        mode: WebcamMode::Overlay,
                    },
                    WebcamSection {
                        start_ms: 4_000,
                        mode: WebcamMode::Large,
                    },
                ],
            }),
            ..VideoDoc::default()
        };
        let json = serde_json::to_string(&doc).unwrap();
        assert!(json.contains(r#""type":"free""#));
        assert_eq!(serde_json::from_str::<VideoDoc>(&json).unwrap(), doc);
    }

    #[test]
    fn fehlende_felder_bekommen_standardwerte() {
        let layout: WebcamLayout = serde_json::from_str(r#"{"shape":"square"}"#).unwrap();
        assert_eq!(layout.shape, WebcamShape::Square);
        assert_eq!(layout.position, WebcamPosition::BottomRight);
        assert!(layout.shadow);
    }
}
