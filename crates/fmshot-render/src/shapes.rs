//! Formen: Rechtecke, Pfeile, Freihand, Bilder …

use fmshot_core::{Color, Fill, Point, Rect, RgbaImage, Stroke};
use tiny_skia::{
    BlendMode, FillRule, FilterQuality, GradientStop, IntRect, LinearGradient, Mask, Paint, Path,
    PathBuilder, Pixmap, PixmapPaint, SpreadMode, Transform,
};

use crate::codec::to_pixmap;

pub(crate) fn skia_color(c: Color, opacity: f32) -> tiny_skia::Color {
    let a = (c.a as f32 / 255.0) * opacity;
    tiny_skia::Color::from_rgba8(c.r, c.g, c.b, (a * 255.0).round() as u8)
}

pub(crate) fn paint(c: Color, opacity: f32) -> Paint<'static> {
    let mut p = Paint::default();
    p.set_color(skia_color(c, opacity));
    p.anti_alias = true;
    p
}

pub(crate) fn skia_rect(r: Rect) -> Option<tiny_skia::Rect> {
    tiny_skia::Rect::from_xywh(r.x, r.y, r.width.max(0.01), r.height.max(0.01))
}

/// Rechteck mit abgerundeten Ecken (Kreis-Näherung über Bézierkurven).
pub(crate) fn rect_path(r: Rect, radius: f32) -> Option<Path> {
    let rect = skia_rect(r)?;
    let radius = radius.min(r.width / 2.0).min(r.height / 2.0);
    if radius <= 0.0 {
        return Some(PathBuilder::from_rect(rect));
    }
    let mut pb = PathBuilder::new();
    push_round_rect(&mut pb, r, radius);
    pb.finish()
}

fn push_round_rect(pb: &mut PathBuilder, r: Rect, radius: f32) {
    const K: f32 = 0.552_284_8;
    let (x0, y0, x1, y1) = (r.x, r.y, r.right(), r.bottom());
    let c = radius * K;
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
}

pub(crate) fn line_path(from: Point, to: Point) -> Option<Path> {
    let mut pb = PathBuilder::new();
    pb.move_to(from.x, from.y);
    pb.line_to(to.x, to.y);
    pb.finish()
}

fn stroke_style(width: f32) -> tiny_skia::Stroke {
    tiny_skia::Stroke {
        width,
        line_cap: tiny_skia::LineCap::Round,
        line_join: tiny_skia::LineJoin::Round,
        ..tiny_skia::Stroke::default()
    }
}

pub(crate) fn stroke_path(
    pixmap: &mut Pixmap,
    path: &Path,
    stroke: &Stroke,
    opacity: f32,
    t: Transform,
) {
    if stroke.width > 0.0 {
        pixmap.stroke_path(
            path,
            &paint(stroke.color, opacity),
            &stroke_style(stroke.width),
            t,
            None,
        );
    }
}

fn fill_path(pixmap: &mut Pixmap, path: &Path, color: Color, opacity: f32, t: Transform) {
    pixmap.fill_path(path, &paint(color, opacity), FillRule::Winding, t, None);
}

/// Fläche mit Farbe oder Verlauf füllen.
pub(crate) fn fill_rect(pixmap: &mut Pixmap, r: Rect, fill: &Fill, t: Transform) {
    let Some(rect) = skia_rect(r) else { return };
    let path = PathBuilder::from_rect(rect);
    let mut p = Paint {
        anti_alias: true,
        ..Paint::default()
    };
    match *fill {
        Fill::Solid { color } => p.set_color(skia_color(color, 1.0)),
        Fill::LinearGradient { from, to, angle } => {
            let (sin, cos) = angle.to_radians().sin_cos();
            let (cx, cy) = (r.x + r.width / 2.0, r.y + r.height / 2.0);
            let half = (r.width * cos).abs() / 2.0 + (r.height * sin).abs() / 2.0;
            let shader = LinearGradient::new(
                tiny_skia::Point::from_xy(cx - cos * half, cy - sin * half),
                tiny_skia::Point::from_xy(cx + cos * half, cy + sin * half),
                vec![
                    GradientStop::new(0.0, skia_color(from, 1.0)),
                    GradientStop::new(1.0, skia_color(to, 1.0)),
                ],
                SpreadMode::Pad,
                Transform::identity(),
            );
            match shader {
                Some(s) => p.shader = s,
                None => p.set_color(skia_color(from, 1.0)),
            }
        }
    }
    pixmap.fill_path(&path, &p, FillRule::Winding, t, None);
}

pub(crate) fn image(
    pixmap: &mut Pixmap,
    image: &RgbaImage,
    bounds: Rect,
    crop: Option<Rect>,
    corner_radius: f32,
    opacity: f32,
    t: Transform,
) {
    let Some(mut source) = to_pixmap(image) else {
        return;
    };
    if let Some(crop) = crop
        && let Some(cropped) = IntRect::from_xywh(
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
    let t_img = t.pre_translate(bounds.x, bounds.y).pre_scale(sx, sy);
    // 1:1 ohne Drehung bleibt pixelgenau; sonst weich skalieren.
    let exact = (t_img.sx - 1.0).abs() < 1e-4
        && (t_img.sy - 1.0).abs() < 1e-4
        && t_img.kx == 0.0
        && t_img.ky == 0.0;
    let paint = PixmapPaint {
        opacity,
        quality: if exact {
            FilterQuality::Nearest
        } else {
            FilterQuality::Bicubic
        },
        ..PixmapPaint::default()
    };
    let mask = if corner_radius > 0.0 {
        rect_path(bounds, corner_radius).and_then(|path| {
            let mut mask = Mask::new(pixmap.width(), pixmap.height())?;
            mask.fill_path(&path, FillRule::Winding, true, t);
            Some(mask)
        })
    } else {
        None
    };
    pixmap.draw_pixmap(0, 0, source.as_ref(), &paint, t_img, mask.as_ref());
}

pub(crate) fn rectangle(
    pixmap: &mut Pixmap,
    bounds: Rect,
    stroke: &Stroke,
    fill: Option<Color>,
    corner_radius: f32,
    opacity: f32,
    t: Transform,
) {
    if let Some(path) = rect_path(bounds, corner_radius) {
        if let Some(fill) = fill {
            fill_path(pixmap, &path, fill, opacity, t);
        }
        stroke_path(pixmap, &path, stroke, opacity, t);
    }
}

pub(crate) fn ellipse(
    pixmap: &mut Pixmap,
    bounds: Rect,
    stroke: &Stroke,
    fill: Option<Color>,
    opacity: f32,
    t: Transform,
) {
    if let Some(path) = skia_rect(bounds).and_then(PathBuilder::from_oval) {
        if let Some(fill) = fill {
            fill_path(pixmap, &path, fill, opacity, t);
        }
        stroke_path(pixmap, &path, stroke, opacity, t);
    }
}

pub(crate) fn circle(
    pixmap: &mut Pixmap,
    center: Point,
    radius: f32,
    fill: Color,
    opacity: f32,
    t: Transform,
) {
    if let Some(path) = PathBuilder::from_circle(center.x, center.y, radius.max(0.5)) {
        fill_path(pixmap, &path, fill, opacity, t);
    }
}

pub(crate) fn line(
    pixmap: &mut Pixmap,
    from: Point,
    to: Point,
    stroke: &Stroke,
    opacity: f32,
    t: Transform,
) {
    if let Some(path) = line_path(from, to) {
        stroke_path(pixmap, &path, stroke, opacity, t);
    }
}

pub(crate) fn arrow(
    pixmap: &mut Pixmap,
    from: Point,
    to: Point,
    stroke: &Stroke,
    opacity: f32,
    t: Transform,
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
        stroke_path(pixmap, &shaft, stroke, opacity, t);
    }
    let mut pb = PathBuilder::new();
    pb.move_to(to.x, to.y);
    pb.line_to(base.x - uy * head_half, base.y + ux * head_half);
    pb.line_to(base.x + uy * head_half, base.y - ux * head_half);
    pb.close();
    if let Some(head) = pb.finish() {
        fill_path(pixmap, &head, stroke.color, opacity, t);
    }
}

/// Freihand: geglättet über Mittelpunkte (quadratische Kurven).
pub(crate) fn pen(
    pixmap: &mut Pixmap,
    points: &[Point],
    stroke: &Stroke,
    opacity: f32,
    t: Transform,
) {
    let Some(first) = points.first() else { return };
    let mut pb = PathBuilder::new();
    pb.move_to(first.x, first.y);
    if points.len() == 1 {
        // Ein einzelner Klick: Punkt zeichnen.
        pb.line_to(first.x + 0.01, first.y);
    } else {
        for w in points.windows(2) {
            let (a, b) = (w[0], w[1]);
            pb.quad_to(a.x, a.y, (a.x + b.x) / 2.0, (a.y + b.y) / 2.0);
        }
        let last = points[points.len() - 1];
        pb.line_to(last.x, last.y);
    }
    if let Some(path) = pb.finish() {
        stroke_path(pixmap, &path, stroke, opacity, t);
    }
}

/// Textmarker: färbt ein wie ein echter Leuchtstift (Multiplizieren).
pub(crate) fn highlight(
    pixmap: &mut Pixmap,
    bounds: Rect,
    color: Color,
    opacity: f32,
    t: Transform,
) {
    let Some(path) = rect_path(bounds, 0.0) else {
        return;
    };
    let mut p = paint(color, opacity);
    p.blend_mode = BlendMode::Multiply;
    pixmap.fill_path(&path, &p, FillRule::Winding, t, None);
}

/// Sprechblase: abgerundeter Kasten plus Spitze Richtung `tail`.
pub(crate) fn callout_bubble(
    pixmap: &mut Pixmap,
    bounds: Rect,
    tail: Point,
    fill: Color,
    opacity: f32,
    t: Transform,
) {
    let radius = (bounds.width.min(bounds.height) * 0.2).min(16.0);
    let mut pb = PathBuilder::new();
    push_round_rect(&mut pb, bounds, radius);
    // Spitze nur, wenn das Ziel ausserhalb der Blase liegt.
    let inside = tail.x >= bounds.x
        && tail.x <= bounds.right()
        && tail.y >= bounds.y
        && tail.y <= bounds.bottom();
    if !inside {
        let (cx, cy) = (
            bounds.x + bounds.width / 2.0,
            bounds.y + bounds.height / 2.0,
        );
        let half = (bounds.width.min(bounds.height) * 0.18).max(4.0);
        // Basis der Spitze in der Mitte der Seite, die zum Ziel zeigt.
        let (dx, dy) = (tail.x - cx, tail.y - cy);
        let horizontal = dx.abs() / bounds.width.max(1.0) > dy.abs() / bounds.height.max(1.0);
        let (a, b) = if horizontal {
            let x = if dx > 0.0 {
                bounds.right() - 1.0
            } else {
                bounds.x + 1.0
            };
            let y = cy.clamp(bounds.y + radius + half, bounds.bottom() - radius - half);
            (Point::new(x, y - half), Point::new(x, y + half))
        } else {
            let y = if dy > 0.0 {
                bounds.bottom() - 1.0
            } else {
                bounds.y + 1.0
            };
            let x = cx.clamp(bounds.x + radius + half, bounds.right() - radius - half);
            (Point::new(x - half, y), Point::new(x + half, y))
        };
        pb.move_to(a.x, a.y);
        pb.line_to(tail.x, tail.y);
        pb.line_to(b.x, b.y);
        pb.close();
    }
    if let Some(path) = pb.finish() {
        fill_path(pixmap, &path, fill, opacity, t);
    }
}
