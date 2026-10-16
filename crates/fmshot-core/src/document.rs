use serde::{Deserialize, Serialize};

use crate::geometry::{Color, Point, Rect, Size};

/// Version des `doc.json`-Schemas. Bei inkompatiblen Änderungen erhöhen und
/// eine Migration ergänzen.
pub const SCHEMA_VERSION: u32 = 1;

/// Ein FMShot-Dokument: Zeichenfläche plus Ebenen, von unten nach oben.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Document {
    pub schema_version: u32,
    pub artboard: Artboard,
    /// Reihenfolge = Zeichenreihenfolge; das erste Element liegt ganz unten.
    pub layers: Vec<Layer>,
}

/// Der Bereich, der exportiert wird.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Artboard {
    pub rect: Rect,
    /// `None` = transparent.
    pub background: Option<Color>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct LayerId(pub u32);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Layer {
    pub id: LayerId,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub name: Option<String>,
    pub visible: bool,
    pub locked: bool,
    /// 0.0 bis 1.0
    pub opacity: f32,
    /// Drehung in Grad um den Mittelpunkt der Ebene.
    #[serde(default)]
    pub rotation: f32,
    pub kind: LayerKind,
}

/// Woher die Pixel einer Bild-Ebene kommen.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "source", content = "hash", rename_all = "snake_case")]
pub enum AssetRef {
    /// `original.png` der Aufnahme.
    Original,
    /// `assets/<hash>.png` – hineingezogenes Bild.
    Asset(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Stroke {
    pub color: Color,
    pub width: f32,
}

impl Default for Stroke {
    fn default() -> Self {
        Self {
            color: Color::ACCENT,
            width: 4.0,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LayerKind {
    Image {
        asset: AssetRef,
        /// Ziel-Rechteck auf der Leinwand (Position und Grösse nach Skalierung).
        bounds: Rect,
        /// Zuschnitt in Pixeln des Quellbilds; `None` = ganzes Bild.
        #[serde(default, skip_serializing_if = "Option::is_none")]
        crop: Option<Rect>,
        /// Pixelgrösse des Quellbilds.
        source_size: Size,
    },
    Rectangle {
        bounds: Rect,
        stroke: Stroke,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fill: Option<Color>,
        #[serde(default)]
        corner_radius: f32,
    },
    Ellipse {
        bounds: Rect,
        stroke: Stroke,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        fill: Option<Color>,
    },
    Line {
        from: Point,
        to: Point,
        stroke: Stroke,
    },
    Arrow {
        from: Point,
        to: Point,
        stroke: Stroke,
    },
}

impl LayerKind {
    /// Umschliessendes Rechteck ohne Drehung und ohne Strichbreite.
    pub fn bounds(&self) -> Rect {
        match self {
            LayerKind::Image { bounds, .. }
            | LayerKind::Rectangle { bounds, .. }
            | LayerKind::Ellipse { bounds, .. } => *bounds,
            LayerKind::Line { from, to, .. } | LayerKind::Arrow { from, to, .. } => Rect::new(
                from.x.min(to.x),
                from.y.min(to.y),
                (from.x - to.x).abs(),
                (from.y - to.y).abs(),
            ),
        }
    }
}

impl Document {
    /// Neues Dokument aus einer Aufnahme: Der Screenshot wird zur untersten,
    /// gesperrten Ebene, die Zeichenfläche hat genau seine Grösse.
    pub fn from_capture(width: u32, height: u32) -> Self {
        let size = Size::new(width as f32, height as f32);
        let bounds = Rect::new(0.0, 0.0, size.width, size.height);
        Self {
            schema_version: SCHEMA_VERSION,
            artboard: Artboard {
                rect: bounds,
                background: None,
            },
            layers: vec![Layer {
                id: LayerId(1),
                name: None,
                visible: true,
                locked: true,
                opacity: 1.0,
                rotation: 0.0,
                kind: LayerKind::Image {
                    asset: AssetRef::Original,
                    bounds,
                    crop: None,
                    source_size: size,
                },
            }],
        }
    }

    /// Nächste freie Ebenen-ID.
    pub fn next_layer_id(&self) -> LayerId {
        LayerId(self.layers.iter().map(|l| l.id.0).max().unwrap_or(0) + 1)
    }

    /// Fügt eine Ebene oben hinzu und gibt ihre ID zurück.
    pub fn push_layer(&mut self, kind: LayerKind) -> LayerId {
        let id = self.next_layer_id();
        self.layers.push(Layer {
            id,
            name: None,
            visible: true,
            locked: false,
            opacity: 1.0,
            rotation: 0.0,
            kind,
        });
        id
    }

    pub fn layer(&self, id: LayerId) -> Option<&Layer> {
        self.layers.iter().find(|l| l.id == id)
    }

    /// „An Inhalt anpassen“: Zeichenfläche umschliesst alle sichtbaren Ebenen.
    pub fn fit_artboard_to_content(&mut self) {
        let mut visible = self.layers.iter().filter(|l| l.visible);
        if let Some(first) = visible.next() {
            let bounds = visible.fold(first.kind.bounds(), |acc, l| acc.union(&l.kind.bounds()));
            self.artboard.rect = bounds;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aufnahme_wird_gesperrte_unterste_ebene() {
        let doc = Document::from_capture(800, 600);
        assert_eq!(doc.artboard.rect, Rect::new(0.0, 0.0, 800.0, 600.0));
        assert_eq!(doc.layers.len(), 1);
        assert!(doc.layers[0].locked);
        assert!(matches!(
            doc.layers[0].kind,
            LayerKind::Image {
                asset: AssetRef::Original,
                ..
            }
        ));
    }

    #[test]
    fn neue_ebenen_bekommen_fortlaufende_ids() {
        let mut doc = Document::from_capture(10, 10);
        let a = doc.push_layer(LayerKind::Line {
            from: Point::new(0.0, 0.0),
            to: Point::new(5.0, 5.0),
            stroke: Stroke::default(),
        });
        assert_eq!(a, LayerId(2));
        assert_eq!(doc.next_layer_id(), LayerId(3));
    }

    #[test]
    fn zeichenflaeche_passt_sich_an_inhalt_an() {
        let mut doc = Document::from_capture(100, 100);
        doc.push_layer(LayerKind::Rectangle {
            bounds: Rect::new(80.0, 50.0, 70.0, 10.0),
            stroke: Stroke::default(),
            fill: None,
            corner_radius: 0.0,
        });
        doc.fit_artboard_to_content();
        assert_eq!(doc.artboard.rect, Rect::new(0.0, 0.0, 150.0, 100.0));
    }

    #[test]
    fn json_hin_und_zurueck_bleibt_gleich() {
        let mut doc = Document::from_capture(320, 200);
        doc.push_layer(LayerKind::Arrow {
            from: Point::new(10.0, 10.0),
            to: Point::new(100.0, 50.0),
            stroke: Stroke::default(),
        });
        doc.push_layer(LayerKind::Image {
            asset: AssetRef::Asset("abc123".into()),
            bounds: Rect::new(0.0, 0.0, 50.0, 50.0),
            crop: Some(Rect::new(5.0, 5.0, 20.0, 20.0)),
            source_size: Size::new(64.0, 64.0),
        });
        let json = serde_json::to_string_pretty(&doc).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, back);
        assert!(json.contains("\"type\": \"arrow\""));
        assert!(json.contains("\"source\": \"asset\""));
    }
}
