//! Video-Editor (ADR 0011): Lage von Bildschirm, Webcam, Zeiger und Klicks,
//! Zeitachse und Bearbeiten – dieselbe Rechnung wie beim Export
//! (`grabmark-video`). Ein- und Ausgabe als JSON; `doc_json` ist der
//! Video-Teil des Dokuments.

use grabmark_core::{
    FaceTrack, InputTrack, Size, VideoDoc, WebcamFraming, WebcamMode, ZoomSection,
};
use grabmark_video::audio::{self, AudioTrack};
use grabmark_video::cursor::{ARROW_INNER, ARROW_OUTER, CursorTrack, auto_zooms};
use grabmark_video::layout::{Placement, SceneExtras, Source};
use grabmark_video::{HeadPath, edit, timeline};
use serde::Deserialize;
use serde_json::{Value, json};
use wasm_bindgen::prelude::*;

fn placement(source: Source, p: &Placement) -> Value {
    json!({
        "source": source.name(),
        "rect": [p.rect.x, p.rect.y, p.rect.width, p.rect.height],
        "crop": [p.crop.x, p.crop.y, p.crop.width, p.crop.height],
        "radius": p.radius,
        "mirror": p.mirror,
        "border": p.border.map(|(w, c)| json!({"width": w, "color": [c.r, c.g, c.b, c.a]})),
        "shadow": p.shadow,
        "opacity": p.opacity,
    })
}

/// Bearbeitungsschritt (Zeiten in Ausgabe-Zeit).
#[derive(Deserialize)]
#[serde(tag = "op", rename_all = "camelCase")]
enum Op {
    Cut {
        start: f64,
        end: f64,
    },
    Trim {
        start: f64,
        end: f64,
    },
    Uncut,
    WebcamMode {
        start: f64,
        end: f64,
        mode: WebcamMode,
    },
    AddZoom {
        t: f64,
        x: f32,
        y: f32,
    },
    SetZoom {
        index: usize,
        zoom: Option<ZoomSection>,
    },
    /// Zoom-Abschnitte aus den Klicks neu berechnen (ersetzt alle).
    AutoZoom,
    /// Klick löschen (`source` = Zeit der Aufnahme, siehe `clickSources`).
    HideClick {
        source: u32,
    },
    /// Alle gelöschten Klicks wieder zeigen.
    ShowClicks,
    LayerTime {
        layer: u32,
        range: Option<(f64, f64)>,
    },
    /// Lautstärke-Punkt setzen; `index` = vorhandenen verschieben.
    VolumeKey {
        track: String,
        index: Option<usize>,
        t: f64,
        volume: f32,
    },
    RemoveVolumeKey {
        track: String,
        index: usize,
    },
    /// Alle Punkte einer Spur löschen.
    ClearVolumeKeys {
        track: String,
    },
    /// Ein-/Ausblenden in Bildern; fehlend = unverändert.
    Fade {
        fade_in: Option<u32>,
        fade_out: Option<u32>,
    },
}

fn track(name: &str) -> Result<AudioTrack, JsError> {
    AudioTrack::parse(name).ok_or_else(|| JsError::new(&format!("Tonspur {name}?")))
}

/// Vorschau eines Videos im Editor: hält Dokument und Mausspur, damit pro
/// Bild nur noch gerechnet wird.
#[wasm_bindgen]
pub struct VideoPreview {
    doc: VideoDoc,
    cursor: CursorTrack,
    faces: Option<FaceTrack>,
    /// Kopfausschnitt, wie ihn der Export rechnet; mit dem Wert von
    /// „Kopf folgen“, für den er gilt.
    head: Option<(bool, HeadPath)>,
}

#[wasm_bindgen]
impl VideoPreview {
    #[wasm_bindgen(constructor)]
    #[allow(clippy::new_without_default)]
    pub fn new() -> Self {
        Self {
            doc: VideoDoc::default(),
            cursor: CursorTrack::default(),
            faces: None,
            head: None,
        }
    }

    #[wasm_bindgen(js_name = setDoc)]
    pub fn set_doc(&mut self, doc_json: &str) -> Result<(), JsError> {
        self.doc = serde_json::from_str(doc_json)?;
        self.update_head();
        Ok(())
    }

    /// Kopfpositionen der Webcam (`<id>_faces.json`); leer = keine.
    #[wasm_bindgen(js_name = setFaces)]
    pub fn set_faces(&mut self, faces_json: &str) -> Result<(), JsError> {
        self.faces = if faces_json.is_empty() {
            None
        } else {
            Some(serde_json::from_str(faces_json)?)
        };
        self.head = None;
        self.update_head();
        Ok(())
    }

    /// Kopfausschnitt nur neu rechnen, wenn er gebraucht wird und sich
    /// „Kopf folgen“ geändert hat.
    fn update_head(&mut self) {
        let layout = self
            .doc
            .webcam
            .as_ref()
            .map(|w| &w.layout)
            .filter(|l| l.framing == WebcamFraming::Head);
        let (Some(layout), Some(faces)) = (layout, &self.faces) else {
            return;
        };
        if self.head.as_ref().is_some_and(|(f, _)| *f == layout.follow) {
            return;
        }
        self.head = HeadPath::new(faces, layout.follow).map(|p| (layout.follow, p));
    }

    /// Mausspur (`<id>_input.json`); leer = keine.
    #[wasm_bindgen(js_name = setInput)]
    pub fn set_input(&mut self, input_json: &str) -> Result<(), JsError> {
        self.cursor = if input_json.is_empty() {
            CursorTrack::default()
        } else {
            CursorTrack::new(&serde_json::from_str::<InputTrack>(input_json)?)
        };
        Ok(())
    }

    #[wasm_bindgen(js_name = hasInput)]
    pub fn has_input(&self) -> bool {
        !self.cursor.is_empty()
    }

    /// Was zur Zeit `output_ms` (fertiges Video) wo liegt: Quellzeit und
    /// Ebenen von unten nach oben. `cam_w = 0` = keine Webcam. `cursor_px` =
    /// Zeigerhöhe in Pixeln der Bildschirm-Spur.
    #[allow(clippy::too_many_arguments)]
    pub fn scene(
        &self,
        output_ms: f64,
        frame_w: f32,
        frame_h: f32,
        screen_w: f32,
        screen_h: f32,
        cam_w: f32,
        cam_h: f32,
        cursor_px: f32,
    ) -> String {
        let source_ms = timeline::source_time(&self.doc, output_ms);
        let camera = (cam_w > 0.0 && cam_h > 0.0).then(|| Size::new(cam_w, cam_h));
        let scene = grabmark_video::scene_full(
            &self.doc,
            source_ms,
            Size::new(frame_w, frame_h),
            Size::new(screen_w, screen_h),
            camera,
            &SceneExtras {
                head: self.head.as_ref().map(|(_, p)| p),
                cursor: Some(&self.cursor),
                cursor_px,
            },
        );
        let layers: Vec<Value> = scene.layers.iter().map(|(s, p)| placement(*s, p)).collect();
        json!({ "sourceMs": source_ms, "layers": layers }).to_string()
    }

    /// Zeit des fertigen Videos → Zeit der Aufnahme.
    #[wasm_bindgen(js_name = sourceTime)]
    pub fn source_time(&self, output_ms: f64) -> f64 {
        timeline::source_time(&self.doc, output_ms)
    }

    /// Zeit der Aufnahme → Zeit des fertigen Videos.
    #[wasm_bindgen(js_name = outputTime)]
    pub fn output_time(&self, source_ms: f64) -> f64 {
        timeline::output_time(&self.doc, source_ms)
    }

    /// Länge des fertigen Videos in ms.
    pub fn duration(&self) -> u32 {
        timeline::output_duration(&self.doc)
    }

    /// Liegt die Quellzeit in einem Schnitt? Dann dessen Ende, sonst -1.
    #[wasm_bindgen(js_name = skipCut)]
    pub fn skip_cut(&self, source_ms: f64) -> f64 {
        timeline::skip_cut(&self.doc, source_ms).unwrap_or(-1.0)
    }

    /// Sichtbare Klicks in Ausgabe-Zeit (für die Zeitleiste): `[t, t, …]`.
    #[wasm_bindgen(js_name = clickTimes)]
    pub fn click_times(&self) -> Vec<f64> {
        self.cursor
            .visible_clicks(&self.doc.hidden_clicks)
            .map(|c| timeline::output_time(&self.doc, c.t_ms as f64))
            .collect()
    }

    /// Dieselben Klicks in Zeit der Aufnahme (zum Löschen).
    #[wasm_bindgen(js_name = clickSources)]
    pub fn click_sources(&self) -> Vec<u32> {
        self.cursor
            .visible_clicks(&self.doc.hidden_clicks)
            .map(|c| c.t_ms)
            .collect()
    }

    /// Lautstärke einer Tonspur (`system`/`mic`) zur Zeit des fertigen
    /// Videos, mit Punkten und Blenden (0–1).
    pub fn gain(&self, track: &str, output_ms: f64) -> f32 {
        AudioTrack::parse(track).map_or(1.0, |t| audio::gain_at(&self.doc, t, output_ms))
    }

    /// Deckkraft des Bilds durch Ein- und Ausblenden (0–1).
    pub fn fade(&self, output_ms: f64) -> f32 {
        audio::fade_at(&self.doc, output_ms)
    }

    /// Länge von Ein- und Ausblenden in ms: `[ein, aus]`.
    #[wasm_bindgen(js_name = fadeMs)]
    pub fn fade_ms(&self) -> Vec<f64> {
        let (a, b) = audio::fade_ms(&self.doc);
        vec![a, b]
    }

    /// Lautstärke-Punkte einer Spur in Ausgabe-Zeit: `[t, v, t, v, …]`.
    /// Punkte in Schnitten liegen an der Stelle direkt danach.
    #[wasm_bindgen(js_name = volumeKeys)]
    pub fn volume_keys(&self, track: &str) -> Vec<f64> {
        let Some(t) = AudioTrack::parse(track) else {
            return Vec::new();
        };
        t.keys(&self.doc)
            .iter()
            .flat_map(|k| {
                [
                    timeline::output_time(&self.doc, k.t_ms as f64),
                    k.volume as f64,
                ]
            })
            .collect()
    }

    /// Lautstärkekurve fürs Zeichnen: `[t, v, …]` an allen Knickstellen, in
    /// Ausgabe-Zeit (ohne Blenden – die zeigt die Video-Zeile).
    #[wasm_bindgen(js_name = volumeCurve)]
    pub fn volume_curve(&self, track: &str) -> Vec<f64> {
        let Some(t) = AudioTrack::parse(track) else {
            return Vec::new();
        };
        let doc = VideoDoc {
            fade: Default::default(),
            ..self.doc.clone()
        };
        audio::volume_ramps(&doc, t)
            .iter()
            .flat_map(|r| [r.start_ms, r.from as f64, r.end_ms, r.to as f64])
            .collect()
    }

    /// Wendet einen Bearbeitungsschritt an und gibt den neuen Video-Teil
    /// zurück (das Objekt selbst bleibt beim alten, bis `setDoc`).
    pub fn edit(&self, op_json: &str) -> Result<String, JsError> {
        let doc = &self.doc;
        let op: Op = serde_json::from_str(op_json)?;
        let out = match op {
            Op::Cut { start, end } => edit::cut(doc, start, end),
            Op::Trim { start, end } => edit::trim(doc, start, end),
            Op::Uncut => edit::uncut(doc),
            Op::WebcamMode { start, end, mode } => edit::set_webcam_mode(doc, start, end, mode),
            Op::AddZoom { t, x, y } => edit::add_zoom(doc, t, x, y),
            Op::SetZoom { index, zoom } => edit::set_zoom(doc, index, zoom),
            Op::AutoZoom => VideoDoc {
                zooms: auto_zooms(&self.cursor, doc.duration_ms, &doc.hidden_clicks),
                ..doc.clone()
            },
            Op::LayerTime { layer, range } => edit::set_layer_time(doc, layer, range),
            Op::HideClick { source } => edit::hide_click(doc, source),
            Op::ShowClicks => edit::show_clicks(doc),
            Op::VolumeKey {
                track: name,
                index,
                t,
                volume,
            } => audio::set_volume_key(doc, track(&name)?, index, t, volume).0,
            Op::RemoveVolumeKey { track: name, index } => {
                audio::remove_volume_key(doc, track(&name)?, index)
            }
            Op::ClearVolumeKeys { track: name } => {
                let mut out = doc.clone();
                track(&name)?.keys_mut(&mut out).clear();
                out
            }
            Op::Fade { fade_in, fade_out } => audio::set_fade(doc, fade_in, fade_out),
        };
        Ok(serde_json::to_string(&out)?)
    }
}

/// Umriss des Mauszeigers (Höhe 1, Spitze bei 0,0): `[aussen…, innen…]` als
/// flache Liste x, y – dieselbe Form wie im Export.
#[wasm_bindgen(js_name = cursorShape)]
pub fn cursor_shape() -> Vec<f32> {
    ARROW_OUTER
        .iter()
        .chain(ARROW_INNER.iter())
        .flat_map(|&(x, y)| [x, y])
        .collect()
}
