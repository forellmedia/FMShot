//! Zeichnet FMShot-Dokumente.
//!
//! Derselbe Code läuft nativ (Export, Auto-Export, Vorschaubilder, CLI) und als
//! WASM im Editor – damit ein Dokument überall gleich aussieht (ADR 0002).
//!
//! Der Editor zeichnet nur den sichtbaren Ausschnitt in Bildschirmauflösung
//! ([`RenderOptions::region`], [`RenderOptions::scale`]) und kann sich auf
//! einen Teil der Ebenen beschränken ([`RenderOptions::layers`]), um beim
//! Ziehen nur die aktive Ebene neu zu zeichnen.

mod codec;
mod effects;
mod shapes;
mod text;

use std::ops::Range;

use fmshot_core::{AssetRef, Document, Fill, Layer, LayerId, LayerKind, Rect, RgbaImage};
use tiny_skia::{Pixmap, Transform};

pub use codec::{CodecError, decode_image, decode_png};
pub use text::TextMetrics;

/// Liefert die Pixel der Bild-Ebenen. Die Bibliothek lädt sie von der Platte,
/// der Editor hält sie im Speicher.
pub trait AssetSource {
    fn image(&self, asset: &AssetRef) -> Option<&RgbaImage>;
}

impl AssetSource for std::collections::HashMap<AssetRef, RgbaImage> {
    fn image(&self, asset: &AssetRef) -> Option<&RgbaImage> {
        self.get(asset)
    }
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RenderError {
    #[error("Ausgabe ist leer oder zu gross ({width}×{height})")]
    InvalidSize { width: u32, height: u32 },
    #[error("Bild {0:?} fehlt")]
    MissingAsset(AssetRef),
}

/// Was und wie gezeichnet wird.
#[derive(Debug, Clone, PartialEq)]
pub struct RenderOptions {
    /// Ausschnitt in Dokument-Koordinaten.
    pub region: Rect,
    /// Ausgabepixel pro Dokument-Pixel (z. B. 0.5 für halbe Grösse).
    pub scale: f32,
    /// Welche Ebenen (Indizes in `doc.layers`).
    pub layers: Range<usize>,
    /// Hintergrund der Zeichenfläche zeichnen?
    pub background: bool,
    /// Diese Ebene auslassen (z. B. Text, der gerade bearbeitet wird).
    pub skip: Option<LayerId>,
}

impl RenderOptions {
    /// Ganze Zeichenfläche in Originalgrösse – für Export und Zwischenablage.
    pub fn full(doc: &Document) -> Self {
        Self {
            region: doc.artboard.rect,
            scale: 1.0,
            layers: 0..doc.layers.len(),
            background: true,
            skip: None,
        }
    }

    /// Ganze Zeichenfläche, verkleinert auf höchstens `max` Pixel Kantenlänge.
    pub fn fit(doc: &Document, max: u32) -> Self {
        let r = doc.artboard.rect;
        let longest = r.width.max(r.height).max(1.0);
        Self {
            scale: (max as f32 / longest).min(1.0),
            ..Self::full(doc)
        }
    }

    /// Ausgabegrösse in Pixeln.
    pub fn output_size(&self) -> (u32, u32) {
        (
            (self.region.width * self.scale).ceil().max(0.0) as u32,
            (self.region.height * self.scale).ceil().max(0.0) as u32,
        )
    }
}

/// Zeichnet Dokumente. Hält die Schriften (teuer zu laden) – einmal anlegen
/// und wiederverwenden.
pub struct Renderer {
    text: text::TextEngine,
}

impl Default for Renderer {
    fn default() -> Self {
        Self::new()
    }
}

impl Renderer {
    pub fn new() -> Self {
        Self {
            text: text::TextEngine::new(),
        }
    }

    /// Rechnet das ganze Dokument flach (Grösse der Zeichenfläche).
    pub fn render(
        &mut self,
        doc: &Document,
        assets: &dyn AssetSource,
    ) -> Result<RgbaImage, RenderError> {
        self.render_with(doc, assets, &RenderOptions::full(doc))
    }

    pub fn render_with(
        &mut self,
        doc: &Document,
        assets: &dyn AssetSource,
        opts: &RenderOptions,
    ) -> Result<RgbaImage, RenderError> {
        let (width, height) = opts.output_size();
        let mut pixmap =
            Pixmap::new(width, height).ok_or(RenderError::InvalidSize { width, height })?;

        // Dokument → Ausgabepixel
        let base = Transform::from_scale(opts.scale, opts.scale)
            .pre_translate(-opts.region.x, -opts.region.y);

        if opts.background
            && let Some(fill) = &doc.artboard.background
        {
            shapes::fill_rect(&mut pixmap, doc.artboard.rect, fill, base);
        }

        let end = opts.layers.end.min(doc.layers.len());
        let start = opts.layers.start.min(end);
        for layer in doc.layers[start..end]
            .iter()
            .filter(|l| l.visible && l.opacity > 0.0 && Some(l.id) != opts.skip)
        {
            self.draw_layer(&mut pixmap, layer, base, opts.scale, assets)?;
        }

        Ok(codec::to_rgba_image(&pixmap))
    }

    /// Misst einen Text (in Dokument-Pixeln) – für die Grösse von Textkästen.
    pub fn measure_text(
        &mut self,
        text: &str,
        style: &fmshot_core::TextStyle,
        max_width: Option<f32>,
    ) -> TextMetrics {
        self.text.measure(text, style, max_width)
    }

    fn draw_layer(
        &mut self,
        pixmap: &mut Pixmap,
        layer: &Layer,
        base: Transform,
        scale: f32,
        assets: &dyn AssetSource,
    ) -> Result<(), RenderError> {
        let b = layer.kind.bounds();
        // Effekte wirken auf ein achsparalleles Rechteck und drehen nicht mit.
        let t = if layer.rotation != 0.0 && !layer.kind.is_effect() {
            base.pre_concat(Transform::from_rotate_at(
                layer.rotation,
                b.x + b.width / 2.0,
                b.y + b.height / 2.0,
            ))
        } else {
            base
        };
        let opacity = layer.opacity.clamp(0.0, 1.0);

        match &layer.kind {
            LayerKind::Image {
                asset,
                bounds,
                crop,
                corner_radius,
                shadow,
                ..
            } => {
                let image = assets
                    .image(asset)
                    .ok_or_else(|| RenderError::MissingAsset(asset.clone()))?;
                if let Some(shadow) = shadow {
                    effects::shadow(pixmap, *bounds, *corner_radius, shadow, t, scale);
                }
                shapes::image(pixmap, image, *bounds, *crop, *corner_radius, opacity, t);
            }
            LayerKind::Rectangle {
                bounds,
                stroke,
                fill,
                corner_radius,
            } => shapes::rectangle(pixmap, *bounds, stroke, *fill, *corner_radius, opacity, t),
            LayerKind::Ellipse {
                bounds,
                stroke,
                fill,
            } => shapes::ellipse(pixmap, *bounds, stroke, *fill, opacity, t),
            LayerKind::Line { from, to, stroke } => {
                shapes::line(pixmap, *from, *to, stroke, opacity, t)
            }
            LayerKind::Arrow { from, to, stroke } => {
                shapes::arrow(pixmap, *from, *to, stroke, opacity, t)
            }
            LayerKind::Pen { points, stroke } => shapes::pen(pixmap, points, stroke, opacity, t),
            LayerKind::Highlight { bounds, color } => {
                shapes::highlight(pixmap, *bounds, *color, opacity, t)
            }
            LayerKind::Text {
                bounds,
                text,
                style,
            } => self
                .text
                .draw_box(pixmap, *bounds, text, style, None, opacity, t, scale),
            LayerKind::Callout {
                bounds,
                tail,
                text,
                style,
                fill,
            } => {
                shapes::callout_bubble(pixmap, *bounds, *tail, *fill, opacity, t);
                let pad = style.size * 0.5;
                let inner = Rect::new(
                    bounds.x + pad,
                    bounds.y + pad,
                    (bounds.width - 2.0 * pad).max(1.0),
                    (bounds.height - 2.0 * pad).max(1.0),
                );
                self.text
                    .draw_box(pixmap, inner, text, style, None, opacity, t, scale);
            }
            LayerKind::Step {
                center,
                radius,
                number,
                fill,
                text_color,
            } => {
                shapes::circle(pixmap, *center, *radius, *fill, opacity, t);
                let style = fmshot_core::TextStyle {
                    size: radius * 1.1,
                    color: *text_color,
                    bold: true,
                    background: None,
                };
                self.text.draw_centered(
                    pixmap,
                    *center,
                    &number.to_string(),
                    &style,
                    opacity,
                    t,
                    scale,
                );
            }
            LayerKind::Blur { bounds, radius } => {
                effects::blur(pixmap, *bounds, radius * scale, base)
            }
            LayerKind::Pixelate { bounds, block } => {
                effects::pixelate(pixmap, *bounds, block * scale, base)
            }
            LayerKind::Spotlight { bounds, dim } => {
                effects::spotlight(pixmap, *bounds, *dim * opacity, base)
            }
            LayerKind::Magnifier {
                bounds,
                source,
                stroke,
            } => effects::magnifier(pixmap, *bounds, *source, stroke, base),
        }
        Ok(())
    }
}

/// Liegt über der Ebene `index` ein Effekt (Weichzeichnen usw.)? Dann kann
/// der Editor die Ebenen darüber nicht getrennt zwischenspeichern.
pub fn has_effect_above(doc: &Document, index: usize) -> bool {
    doc.layers
        .iter()
        .skip(index + 1)
        .any(|l| l.visible && l.kind.is_effect())
}

/// Hintergrund, wie ihn das Werkzeug „Hintergrund“ vorschlägt.
pub fn default_background() -> Fill {
    Fill::LinearGradient {
        from: fmshot_core::Color::rgb(0x6a, 0x5a, 0xf9),
        to: fmshot_core::Color::rgb(0xf9, 0x5a, 0x9a),
        angle: 135.0,
    }
}

#[cfg(test)]
mod tests;
