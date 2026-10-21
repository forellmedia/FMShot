//! Ebenen im Video (ADR 0011, Etappe 6): Pfeile, Text, Rahmen … aus dem
//! Dokument, jede nur in ihrem Zeitbereich. `grabmark-render` zeichnet sie
//! auf eine durchsichtige Fläche in Grösse des Videos; der Compositor legt
//! sie oben auf. Neu gezeichnet wird nur, wenn sich ändert, was sichtbar ist.

use std::collections::HashMap;

use grabmark_core::{AssetRef, Document, LayerKind, RgbaImage};
use grabmark_render::{RenderOptions, Renderer};

use crate::edit::layer_visible;
use crate::{OverlaySource, VideoFrame};

/// Die Aufnahme selbst ist im Dokument eine Bild-Ebene; im Video liegt an
/// ihrer Stelle das Video.
pub fn is_base_layer(kind: &LayerKind) -> bool {
    matches!(
        kind,
        LayerKind::Image {
            asset: AssetRef::Original,
            ..
        }
    )
}

/// Zeichnet die Ebenen eines Video-Dokuments zu jeder Zeit.
pub struct LayerOverlay {
    renderer: Renderer,
    doc: Document,
    assets: HashMap<AssetRef, RgbaImage>,
    /// Sichtbare Ebenen beim letzten Bild.
    last: Option<Vec<u32>>,
}

impl LayerOverlay {
    /// `None`, wenn es ausser der Aufnahme keine Ebenen gibt.
    pub fn new(doc: &Document, assets: HashMap<AssetRef, RgbaImage>) -> Option<Self> {
        let mut doc = doc.clone();
        doc.layers.retain(|l| !is_base_layer(&l.kind));
        if doc.layers.iter().all(|l| !l.visible) {
            return None;
        }
        Some(Self {
            renderer: Renderer::new(),
            doc,
            assets,
            last: None,
        })
    }

    fn visible_at(&self, source_ms: f64) -> Vec<u32> {
        let video = self.doc.video.clone().unwrap_or_default();
        self.doc
            .layers
            .iter()
            .filter(|l| l.visible && layer_visible(&video, l.id.0, source_ms))
            .map(|l| l.id.0)
            .collect()
    }
}

impl OverlaySource for LayerOverlay {
    fn frame(&mut self, source_ms: f64) -> Option<Option<VideoFrame>> {
        let ids = self.visible_at(source_ms);
        if self.last.as_ref() == Some(&ids) {
            return None;
        }
        self.last = Some(ids.clone());
        if ids.is_empty() {
            return Some(None);
        }
        let mut doc = self.doc.clone();
        for l in &mut doc.layers {
            l.visible = ids.contains(&l.id.0);
        }
        let opts = RenderOptions {
            background: false,
            ..RenderOptions::full(&doc)
        };
        let image = match self.renderer.render_with(&doc, &self.assets, &opts) {
            Ok(img) => img,
            Err(e) => {
                log::warn!("Ebenen zeichnen: {e}");
                return Some(None);
            }
        };
        let (width, height) = (image.width(), image.height());
        let mut data = image.into_data();
        // RGBA → BGRA
        for px in data.as_chunks_mut::<4>().0 {
            px.swap(0, 2);
        }
        Some(Some(VideoFrame {
            pts: std::time::Duration::ZERO,
            width,
            height,
            stride: width as usize * 4,
            data,
        }))
    }
}

#[cfg(test)]
mod tests {
    use grabmark_core::{Color, LayerTime, Rect, Stroke, VideoDoc};

    use super::*;

    fn doc() -> Document {
        let mut d = Document::from_capture(40, 20);
        let id = d.push_layer(LayerKind::Rectangle {
            bounds: Rect::new(5.0, 5.0, 10.0, 10.0),
            stroke: Stroke {
                color: Color::ACCENT,
                width: 2.0,
                ..Stroke::default()
            },
            fill: None,
            corner_radius: 0.0,
        });
        d.video = Some(VideoDoc {
            duration_ms: 5000,
            layer_times: vec![LayerTime {
                layer: id.0,
                start_ms: 1000,
                end_ms: 2000,
            }],
            ..VideoDoc::default()
        });
        d
    }

    #[test]
    fn nur_ohne_aufnahme_und_nur_zur_rechten_zeit() {
        let mut o = LayerOverlay::new(&doc(), HashMap::new()).unwrap();
        // Vor dem Zeitbereich: nichts zu sehen
        assert_eq!(o.frame(500.0), Some(None));
        // Unverändert: kein neues Bild
        assert_eq!(o.frame(600.0), None);
        let img = o.frame(1500.0).unwrap().unwrap();
        assert_eq!((img.width, img.height), (40, 20));
        // Rand des Rechtecks gezeichnet, Rest durchsichtig
        let alpha = |x: usize, y: usize| img.data[(y * 40 + x) * 4 + 3];
        assert!(alpha(5, 10) > 0);
        assert_eq!(alpha(30, 2), 0);
        assert_eq!(o.frame(2500.0), Some(None));
    }

    #[test]
    fn ohne_ebenen_kein_overlay() {
        assert!(LayerOverlay::new(&Document::from_capture(40, 20), HashMap::new()).is_none());
    }
}
