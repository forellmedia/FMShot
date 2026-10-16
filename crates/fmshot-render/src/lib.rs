//! Zeichnet FMShot-Dokumente.
//!
//! Derselbe Code läuft nativ (Export, Auto-Export, Vorschaubilder, CLI) und als
//! WASM im Editor – damit ein Dokument überall gleich aussieht (ADR 0002).

use fmshot_core::{AssetRef, Color, Document, Layer, LayerKind, Point, Rect, RgbaImage, Stroke};
use tiny_skia::{
    FillRule, FilterQuality, IntRect, Paint, PathBuilder, Pixmap, PixmapPaint, Transform,
};

/// Liefert die Pixel der Bild-Ebenen. Die Bibliothek lädt sie von der Platte,
/// der Editor hält sie im Speicher.
pub trait AssetSource {
    fn image(&self, asset: &AssetRef) -> Option<&RgbaImage>;
}

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum RenderError {
    #[error("Zeichenfläche ist leer oder zu gross ({width}×{height})")]
    InvalidArtboard { width: u32, height: u32 },
    #[error("Bild {0:?} fehlt")]
    MissingAsset(AssetRef),
}

/// Rechnet das Dokument flach – Ergebnis hat die Grösse der Zeichenfläche.
pub fn render(doc: &Document, assets: &dyn AssetSource) -> Result<RgbaImage, RenderError> {
    let board = doc.artboard.rect;
    let width = board.width.ceil().max(0.0) as u32;
    let height = board.height.ceil().max(0.0) as u32;
    let mut pixmap =
        Pixmap::new(width, height).ok_or(RenderError::InvalidArtboard { width, height })?;

    if let Some(bg) = doc.artboard.background {
        pixmap.fill(skia_color(bg, 1.0));
    }

    let to_board = Transform::from_translate(-board.x, -board.y);
    for layer in doc.layers.iter().filter(|l| l.visible && l.opacity > 0.0) {
        draw_layer(&mut pixmap, layer, to_board, assets)?;
    }

    Ok(to_rgba_image(&pixmap))
}

fn draw_layer(
    pixmap: &mut Pixmap,
    layer: &Layer,
    to_board: Transform,
    assets: &dyn AssetSource,
) -> Result<(), RenderError> {
    let b = layer.kind.bounds();
    let transform = if layer.rotation != 0.0 {
        to_board.pre_concat(Transform::from_rotate_at(
            layer.rotation,
            b.x + b.width / 2.0,
            b.y + b.height / 2.0,
        ))
    } else {
        to_board
    };
    let opacity = layer.opacity.clamp(0.0, 1.0);

    match &layer.kind {
        LayerKind::Image {
            asset,
            bounds,
            crop,
            ..
        } => {
            let image = assets
                .image(asset)
                .ok_or_else(|| RenderError::MissingAsset(asset.clone()))?;
            draw_image(pixmap, image, *bounds, *crop, opacity, transform);
        }
        LayerKind::Rectangle {
            bounds,
            stroke,
            fill,
            corner_radius,
        } => {
            if let Some(path) = rect_path(*bounds, *corner_radius) {
                fill_and_stroke(pixmap, &path, *fill, stroke, opacity, transform);
            }
        }
        LayerKind::Ellipse {
            bounds,
            stroke,
            fill,
        } => {
            if let Some(path) = skia_rect(*bounds).and_then(PathBuilder::from_oval) {
                fill_and_stroke(pixmap, &path, *fill, stroke, opacity, transform);
            }
        }
        LayerKind::Line { from, to, stroke } => {
            if let Some(path) = line_path(*from, *to) {
                stroke_path(pixmap, &path, stroke, opacity, transform);
            }
        }
        LayerKind::Arrow { from, to, stroke } => {
            draw_arrow(pixmap, *from, *to, stroke, opacity, transform);
        }
    }
    Ok(())
}

fn draw_image(
    pixmap: &mut Pixmap,
    image: &RgbaImage,
    bounds: Rect,
    crop: Option<Rect>,
    opacity: f32,
    transform: Transform,
) {
    let Some(mut source) = to_pixmap(image) else {
        return;
    };
    let crop = crop.unwrap_or(Rect::new(
        0.0,
        0.0,
        image.width() as f32,
        image.height() as f32,
    ));
    if let Some(cropped) = IntRect::from_xywh(
        crop.x.round() as i32,
        crop.y.round() as i32,
        crop.width.round().max(1.0) as u32,
        crop.height.round().max(1.0) as u32,
    )
    .and_then(|r| source.clone_rect(r))
    {
        source = cropped;
    }
    let sx = bounds.width / source.width() as f32;
    let sy = bounds.height / source.height() as f32;
    let paint = PixmapPaint {
        opacity,
        quality: if sx == 1.0 && sy == 1.0 {
            FilterQuality::Nearest
        } else {
            FilterQuality::Bicubic
        },
        ..PixmapPaint::default()
    };
    let t = transform
        .pre_translate(bounds.x, bounds.y)
        .pre_scale(sx, sy);
    pixmap.draw_pixmap(0, 0, source.as_ref(), &paint, t, None);
}

fn draw_arrow(
    pixmap: &mut Pixmap,
    from: Point,
    to: Point,
    stroke: &Stroke,
    opacity: f32,
    transform: Transform,
) {
    let (dx, dy) = (to.x - from.x, to.y - from.y);
    let len = (dx * dx + dy * dy).sqrt();
    if len < f32::EPSILON {
        return;
    }
    let (ux, uy) = (dx / len, dy / len);
    // Spitze wächst mit der Strichbreite, ist aber nie länger als der halbe Pfeil.
    let head_len = (stroke.width * 4.0).max(10.0).min(len / 2.0);
    let head_half = head_len * 0.6;
    let base = Point::new(to.x - ux * head_len, to.y - uy * head_len);

    if let Some(shaft) = line_path(from, base) {
        stroke_path(pixmap, &shaft, stroke, opacity, transform);
    }
    let mut pb = PathBuilder::new();
    pb.move_to(to.x, to.y);
    pb.line_to(base.x - uy * head_half, base.y + ux * head_half);
    pb.line_to(base.x + uy * head_half, base.y - ux * head_half);
    pb.close();
    if let Some(head) = pb.finish() {
        let mut paint = Paint::default();
        paint.set_color(skia_color(stroke.color, opacity));
        paint.anti_alias = true;
        pixmap.fill_path(&head, &paint, FillRule::Winding, transform, None);
    }
}

fn fill_and_stroke(
    pixmap: &mut Pixmap,
    path: &tiny_skia::Path,
    fill: Option<Color>,
    stroke: &Stroke,
    opacity: f32,
    transform: Transform,
) {
    if let Some(fill) = fill {
        let mut paint = Paint::default();
        paint.set_color(skia_color(fill, opacity));
        paint.anti_alias = true;
        pixmap.fill_path(path, &paint, FillRule::Winding, transform, None);
    }
    stroke_path(pixmap, path, stroke, opacity, transform);
}

fn stroke_path(
    pixmap: &mut Pixmap,
    path: &tiny_skia::Path,
    stroke: &Stroke,
    opacity: f32,
    transform: Transform,
) {
    if stroke.width <= 0.0 {
        return;
    }
    let mut paint = Paint::default();
    paint.set_color(skia_color(stroke.color, opacity));
    paint.anti_alias = true;
    let style = tiny_skia::Stroke {
        width: stroke.width,
        line_cap: tiny_skia::LineCap::Round,
        line_join: tiny_skia::LineJoin::Round,
        ..tiny_skia::Stroke::default()
    };
    pixmap.stroke_path(path, &paint, &style, transform, None);
}

fn line_path(from: Point, to: Point) -> Option<tiny_skia::Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(from.x, from.y);
    pb.line_to(to.x, to.y);
    pb.finish()
}

fn rect_path(r: Rect, radius: f32) -> Option<tiny_skia::Path> {
    let rect = skia_rect(r)?;
    let radius = radius.min(r.width / 2.0).min(r.height / 2.0);
    if radius <= 0.0 {
        return Some(PathBuilder::from_rect(rect));
    }
    // Abgerundete Ecken über kubische Bézierkurven (Kreis-Näherung).
    const K: f32 = 0.552_284_8;
    let (x0, y0, x1, y1) = (r.x, r.y, r.right(), r.bottom());
    let c = radius * K;
    let mut pb = PathBuilder::new();
    pb.move_to(x0 + radius, y0);
    pb.line_to(x1 - radius, y0);
    pb.cubic_to(x1 - radius + c, y0, x1, y0 + radius - c, x1, y0 + radius);
    pb.line_to(x1, y1 - radius);
    pb.cubic_to(x1, y1 - radius + c, x1 - radius + c, y1, x1 - radius, y1);
    pb.line_to(x0 + radius, y1);
    pb.cubic_to(x0 + radius - c, y1, x0, y1 - radius + c, x0, y1 - radius);
    pb.line_to(x0, y0 + radius);
    pb.cubic_to(x0, y0 + radius - c, x0 + radius - c, y0, x0 + radius, y0);
    pb.close();
    pb.finish()
}

fn skia_rect(r: Rect) -> Option<tiny_skia::Rect> {
    tiny_skia::Rect::from_xywh(r.x, r.y, r.width, r.height)
}

fn skia_color(c: Color, opacity: f32) -> tiny_skia::Color {
    let a = (c.a as f32 / 255.0) * opacity;
    tiny_skia::Color::from_rgba8(c.r, c.g, c.b, (a * 255.0).round() as u8)
}

/// RGBA mit gerader Deckkraft → vormultiplizierte Pixmap.
fn to_pixmap(image: &RgbaImage) -> Option<Pixmap> {
    let mut data = image.data().to_vec();
    for px in data.chunks_exact_mut(4) {
        let a = px[3] as u16;
        if a != 255 {
            for c in &mut px[..3] {
                *c = ((*c as u16 * a + 127) / 255) as u8;
            }
        }
    }
    let size = tiny_skia::IntSize::from_wh(image.width(), image.height())?;
    Pixmap::from_vec(data, size)
}

/// Vormultiplizierte Pixmap → RGBA mit gerader Deckkraft.
fn to_rgba_image(pixmap: &Pixmap) -> RgbaImage {
    let mut data = Vec::with_capacity(pixmap.data().len());
    for px in pixmap.pixels() {
        let c = px.demultiply();
        data.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    RgbaImage::new(pixmap.width(), pixmap.height(), data)
        .expect("Pixmap liefert immer passende Länge")
}

#[cfg(test)]
mod tests {
    use super::*;
    use fmshot_core::Size;

    struct OnlyOriginal(RgbaImage);

    impl AssetSource for OnlyOriginal {
        fn image(&self, asset: &AssetRef) -> Option<&RgbaImage> {
            matches!(asset, AssetRef::Original).then_some(&self.0)
        }
    }

    const BLUE: [u8; 4] = [0, 0, 255, 255];

    #[test]
    fn screenshot_ohne_ebenen_bleibt_pixelgleich() {
        let original = RgbaImage::filled(40, 30, BLUE);
        let doc = Document::from_capture(40, 30);
        let out = render(&doc, &OnlyOriginal(original.clone())).unwrap();
        assert_eq!(out, original);
    }

    #[test]
    fn gefuelltes_rechteck_landet_an_richtiger_stelle() {
        let mut doc = Document::from_capture(40, 30);
        doc.push_layer(LayerKind::Rectangle {
            bounds: Rect::new(10.0, 10.0, 10.0, 10.0),
            stroke: Stroke {
                color: Color::BLACK,
                width: 0.0,
            },
            fill: Some(Color::WHITE),
            corner_radius: 0.0,
        });
        let out = render(&doc, &OnlyOriginal(RgbaImage::filled(40, 30, BLUE))).unwrap();
        assert_eq!(out.pixel(15, 15), Some([255, 255, 255, 255]));
        assert_eq!(out.pixel(5, 5), Some(BLUE));
    }

    #[test]
    fn verschobene_zeichenflaeche_verschiebt_ausgabe() {
        let mut doc = Document::from_capture(40, 30);
        doc.artboard.rect = Rect::new(-10.0, 0.0, 50.0, 30.0);
        doc.artboard.background = Some(Color::WHITE);
        let out = render(&doc, &OnlyOriginal(RgbaImage::filled(40, 30, BLUE))).unwrap();
        assert_eq!(out.width(), 50);
        assert_eq!(out.pixel(5, 5), Some([255, 255, 255, 255]));
        assert_eq!(out.pixel(15, 5), Some(BLUE));
    }

    #[test]
    fn fehlendes_bild_ist_ein_fehler() {
        let mut doc = Document::from_capture(10, 10);
        doc.push_layer(LayerKind::Image {
            asset: AssetRef::Asset("fehlt".into()),
            bounds: Rect::new(0.0, 0.0, 5.0, 5.0),
            crop: None,
            source_size: Size::new(5.0, 5.0),
        });
        let err = render(&doc, &OnlyOriginal(RgbaImage::filled(10, 10, BLUE))).unwrap_err();
        assert_eq!(
            err,
            RenderError::MissingAsset(AssetRef::Asset("fehlt".into()))
        );
    }

    #[test]
    fn pfeil_zeichnet_spitze_am_ziel() {
        let mut doc = Document::from_capture(100, 40);
        doc.push_layer(LayerKind::Arrow {
            from: Point::new(10.0, 20.0),
            to: Point::new(90.0, 20.0),
            stroke: Stroke {
                color: Color::BLACK,
                width: 4.0,
            },
        });
        let out = render(&doc, &OnlyOriginal(RgbaImage::filled(100, 40, BLUE))).unwrap();
        assert_eq!(out.pixel(85, 20), Some([0, 0, 0, 255]));
        assert_eq!(out.pixel(85, 5), Some(BLUE));
    }
}
