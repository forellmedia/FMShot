use serde::{Deserialize, Serialize};

use crate::geometry::{Color, Point, Rect, Size};

/// Version des Dokument-Schemas. Bei inkompatiblen Änderungen erhöhen und
/// eine Migration ergänzen.
pub const SCHEMA_VERSION: u32 = 1;

/// Ein FMShot-Dokument: Zeichenfläche plus Ebenen, von unten nach oben.
///
/// Koordinaten sind Pixel des Originalbilds (bei Retina also physische
/// Pixel). `pixel_ratio` sagt, wie viele davon auf einen Bildschirmpunkt
/// kommen – damit Strichstärken und Schriftgrössen überall gleich wirken.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Document {
    pub schema_version: u32,
    #[serde(default = "one")]
    pub pixel_ratio: f32,
    pub artboard: Artboard,
    /// Reihenfolge = Zeichenreihenfolge; das erste Element liegt ganz unten.
    pub layers: Vec<Layer>,
}

fn one() -> f32 {
    1.0
}

/// Der Bereich, der exportiert wird.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Artboard {
    pub rect: Rect,
    /// `None` = transparent.
    pub background: Option<Fill>,
}

/// Flächenfüllung.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Fill {
    Solid {
        color: Color,
    },
    /// Linearer Verlauf; `angle` in Grad, 0 = von links nach rechts.
    LinearGradient {
        from: Color,
        to: Color,
        angle: f32,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(transparent)]
pub struct LayerId(pub u32);

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
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
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "source", content = "hash", rename_all = "snake_case")]
pub enum AssetRef {
    /// Originalaufnahme.
    Original,
    /// Hineingezogenes Bild (Hash des Inhalts).
    Asset(String),
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
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

/// Schatten unter einer Ebene (z. B. Screenshot auf Hintergrund).
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct Shadow {
    pub color: Color,
    pub blur: f32,
    pub offset_y: f32,
}

/// Schrift einer Text-Ebene.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
pub struct TextStyle {
    pub size: f32,
    pub color: Color,
    #[serde(default)]
    pub bold: bool,
    /// Hinterlegung hinter dem Text; `None` = ohne.
    #[serde(default)]
    pub background: Option<Color>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[cfg_attr(feature = "specta", derive(specta::Type))]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum LayerKind {
    Image {
        asset: AssetRef,
        /// Ziel-Rechteck auf der Leinwand (Position und Grösse nach Skalierung).
        bounds: Rect,
        /// Zuschnitt in Pixeln des Quellbilds; `None` = ganzes Bild.
        #[serde(default)]
        crop: Option<Rect>,
        /// Pixelgrösse des Quellbilds.
        source_size: Size,
        #[serde(default)]
        corner_radius: f32,
        #[serde(default)]
        shadow: Option<Shadow>,
    },
    Rectangle {
        bounds: Rect,
        stroke: Stroke,
        #[serde(default)]
        fill: Option<Color>,
        #[serde(default)]
        corner_radius: f32,
    },
    Ellipse {
        bounds: Rect,
        stroke: Stroke,
        #[serde(default)]
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
    /// Freihand-Linie.
    Pen {
        points: Vec<Point>,
        stroke: Stroke,
    },
    /// Textmarker: halbdurchsichtige Fläche, die den Inhalt einfärbt.
    Highlight {
        bounds: Rect,
        color: Color,
    },
    /// Text in einem Kasten; `bounds.width` bestimmt den Zeilenumbruch.
    Text {
        bounds: Rect,
        text: String,
        style: TextStyle,
    },
    /// Sprechblase mit Spitze, die auf `tail` zeigt.
    Callout {
        bounds: Rect,
        tail: Point,
        text: String,
        style: TextStyle,
        fill: Color,
    },
    /// Nummerierter Kreis ①②③.
    Step {
        center: Point,
        radius: f32,
        number: u32,
        fill: Color,
        text_color: Color,
    },
    Blur {
        bounds: Rect,
        radius: f32,
    },
    Pixelate {
        bounds: Rect,
        block: f32,
    },
    /// Alles ausser `bounds` wird abgedunkelt.
    Spotlight {
        bounds: Rect,
        /// 0.0 bis 1.0
        dim: f32,
    },
    /// Kreisrunde Lupe: zeigt `source` vergrössert in `bounds`.
    Magnifier {
        bounds: Rect,
        source: Rect,
        stroke: Stroke,
    },
}

impl LayerKind {
    /// Umschliessendes Rechteck ohne Drehung und ohne Strichbreite.
    pub fn bounds(&self) -> Rect {
        match self {
            LayerKind::Image { bounds, .. }
            | LayerKind::Rectangle { bounds, .. }
            | LayerKind::Ellipse { bounds, .. }
            | LayerKind::Highlight { bounds, .. }
            | LayerKind::Text { bounds, .. }
            | LayerKind::Blur { bounds, .. }
            | LayerKind::Pixelate { bounds, .. }
            | LayerKind::Spotlight { bounds, .. }
            | LayerKind::Magnifier { bounds, .. } => *bounds,
            LayerKind::Callout { bounds, tail, .. } => {
                bounds.union(&Rect::new(tail.x, tail.y, 0.0, 0.0))
            }
            LayerKind::Line { from, to, .. } | LayerKind::Arrow { from, to, .. } => Rect::new(
                from.x.min(to.x),
                from.y.min(to.y),
                (from.x - to.x).abs(),
                (from.y - to.y).abs(),
            ),
            LayerKind::Pen { points, .. } => {
                let mut it = points.iter();
                let Some(first) = it.next() else {
                    return Rect::default();
                };
                it.fold(Rect::new(first.x, first.y, 0.0, 0.0), |acc, p| {
                    acc.union(&Rect::new(p.x, p.y, 0.0, 0.0))
                })
            }
            LayerKind::Step { center, radius, .. } => Rect::new(
                center.x - radius,
                center.y - radius,
                radius * 2.0,
                radius * 2.0,
            ),
        }
    }

    /// Wirkt die Ebene auf das, was unter ihr liegt (Weichzeichnen usw.)?
    pub fn is_effect(&self) -> bool {
        matches!(
            self,
            LayerKind::Blur { .. }
                | LayerKind::Pixelate { .. }
                | LayerKind::Spotlight { .. }
                | LayerKind::Magnifier { .. }
        )
    }
}

impl Document {
    /// Neues Dokument aus einer Aufnahme: Der Screenshot wird zur untersten,
    /// gesperrten Ebene, die Zeichenfläche hat genau seine Grösse.
    pub fn from_capture(width: u32, height: u32) -> Self {
        Self::from_capture_scaled(width, height, 1.0)
    }

    /// Wie [`Document::from_capture`], mit Pixel pro Punkt (2.0 bei Retina).
    pub fn from_capture_scaled(width: u32, height: u32, pixel_ratio: f32) -> Self {
        let size = Size::new(width as f32, height as f32);
        let bounds = Rect::new(0.0, 0.0, size.width, size.height);
        Self {
            schema_version: SCHEMA_VERSION,
            pixel_ratio: pixel_ratio.max(1.0),
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
                    corner_radius: 0.0,
                    shadow: None,
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

    /// Alle Bilder, die das Dokument braucht.
    pub fn assets(&self) -> Vec<&AssetRef> {
        self.layers
            .iter()
            .filter_map(|l| match &l.kind {
                LayerKind::Image { asset, .. } => Some(asset),
                _ => None,
            })
            .collect()
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
        let doc = Document::from_capture_scaled(800, 600, 2.0);
        assert_eq!(doc.artboard.rect, Rect::new(0.0, 0.0, 800.0, 600.0));
        assert_eq!(doc.pixel_ratio, 2.0);
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
    fn freihand_grenzen() {
        let kind = LayerKind::Pen {
            points: vec![
                Point::new(5.0, 9.0),
                Point::new(1.0, 2.0),
                Point::new(7.0, 4.0),
            ],
            stroke: Stroke::default(),
        };
        assert_eq!(kind.bounds(), Rect::new(1.0, 2.0, 6.0, 7.0));
    }

    #[test]
    fn altes_dokument_ohne_neue_felder_laedt() {
        // Format aus Meilenstein 2: ohne pixel_ratio, corner_radius, shadow.
        let json = r#"{
            "schema_version": 1,
            "artboard": {"rect": {"x":0,"y":0,"width":10,"height":10}, "background": null},
            "layers": [{"id":1,"visible":true,"locked":true,"opacity":1,"rotation":0,
              "kind":{"type":"image","asset":{"source":"original"},
                      "bounds":{"x":0,"y":0,"width":10,"height":10},
                      "source_size":{"width":10,"height":10}}}]
        }"#;
        let doc: Document = serde_json::from_str(json).unwrap();
        assert_eq!(doc.pixel_ratio, 1.0);
        assert_eq!(doc, Document::from_capture(10, 10));
    }

    #[test]
    fn json_hin_und_zurueck_bleibt_gleich() {
        let mut doc = Document::from_capture(320, 200);
        doc.artboard.background = Some(Fill::LinearGradient {
            from: Color::WHITE,
            to: Color::BLACK,
            angle: 45.0,
        });
        doc.push_layer(LayerKind::Arrow {
            from: Point::new(10.0, 10.0),
            to: Point::new(100.0, 50.0),
            stroke: Stroke::default(),
        });
        doc.push_layer(LayerKind::Text {
            bounds: Rect::new(0.0, 0.0, 100.0, 20.0),
            text: "Hallo Ü".into(),
            style: TextStyle {
                size: 16.0,
                color: Color::BLACK,
                bold: true,
                background: None,
            },
        });
        doc.push_layer(LayerKind::Image {
            asset: AssetRef::Asset("abc123".into()),
            bounds: Rect::new(0.0, 0.0, 50.0, 50.0),
            crop: Some(Rect::new(5.0, 5.0, 20.0, 20.0)),
            source_size: Size::new(64.0, 64.0),
            corner_radius: 8.0,
            shadow: Some(Shadow {
                color: Color::BLACK,
                blur: 10.0,
                offset_y: 4.0,
            }),
        });
        let json = serde_json::to_string_pretty(&doc).unwrap();
        let back: Document = serde_json::from_str(&json).unwrap();
        assert_eq!(doc, back);
        assert!(json.contains("\"type\": \"arrow\""));
        assert!(json.contains("\"source\": \"asset\""));
    }
}
