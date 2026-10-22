//! Formen: Rechtecke, Pfeile, Freihand, Bilder …

use nabshot_core::{ArrowHead, Color, Fill, LineStyle, Point, Rect, RgbaImage, Stroke};
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

/// Strichmuster (Strich, Lücke) für gestrichelt/gepunktet. Mit runden Enden
/// wirkt jeder Strich um eine Strichbreite länger, die Lücke entsprechend kürzer.
fn dash_pattern(style: LineStyle, width: f32) -> Option<tiny_skia::StrokeDash> {
    let (on, off) = match style {
        LineStyle::Dashed => (width * 2.5, width * 3.0),
        // Strich der Länge ~0: Die runden Enden ergeben einen Punkt.
        LineStyle::Dotted => (0.001, width * 2.0),
        LineStyle::Solid | LineStyle::Double => return None,
    };
    tiny_skia::StrokeDash::new(vec![on, off], 0.0)
}

/// Strich in der gewählten Linienart.
pub(crate) fn stroke_path(
    pixmap: &mut Pixmap,
    path: &Path,
    stroke: &Stroke,
    opacity: f32,
    t: Transform,
) {
    if stroke.width <= 0.0 {
        return;
    }
    if stroke.style == LineStyle::Double {
        double_stroke(pixmap, path, stroke, opacity, t);
        return;
    }
    let style = tiny_skia::Stroke {
        dash: dash_pattern(stroke.style, stroke.width),
        ..stroke_style(stroke.width)
    };
    pixmap.stroke_path(path, &paint(stroke.color, opacity), &style, t, None);
}

/// Doppellinie: dreifach breiter Strich, die Mitte wieder ausgeschnitten.
/// Auf eigener Fläche, damit das Ausschneiden das Bild darunter nicht trifft.
fn double_stroke(pixmap: &mut Pixmap, path: &Path, stroke: &Stroke, opacity: f32, t: Transform) {
    let Some(mut layer) = Pixmap::new(pixmap.width(), pixmap.height()) else {
        return;
    };
    let butt = |width| tiny_skia::Stroke {
        line_cap: tiny_skia::LineCap::Butt,
        ..stroke_style(width)
    };
    layer.stroke_path(
        path,
        &paint(stroke.color, 1.0),
        &butt(stroke.width * 3.0),
        t,
        None,
    );
    let clear = Paint {
        blend_mode: BlendMode::Clear,
        anti_alias: true,
        ..Paint::default()
    };
    layer.stroke_path(path, &clear, &butt(stroke.width), t, None);
    let p = PixmapPaint {
        opacity,
        ..PixmapPaint::default()
    };
    pixmap.draw_pixmap(0, 0, layer.as_ref(), &p, Transform::identity(), None);
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

/// Wo und wie eine Bild-Ebene liegt.
pub(crate) struct ImagePlacement {
    pub bounds: Rect,
    pub crop: Option<Rect>,
    pub corner_radius: f32,
}

/// `quality`: Filter, wenn skaliert oder gedreht wird (1:1 bleibt pixelgenau).
pub(crate) fn image(
    pixmap: &mut Pixmap,
    image: &RgbaImage,
    place: ImagePlacement,
    opacity: f32,
    quality: FilterQuality,
    t: Transform,
) {
    let ImagePlacement {
        bounds,
        crop,
        corner_radius,
    } = place;
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
            quality
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

/// Wie der Pfeil aussieht.
pub(crate) struct ArrowShape<'a> {
    pub stroke: &'a Stroke,
    pub head: ArrowHead,
    /// Grösse der Spitze relativ zur Standardgrösse.
    pub head_size: f32,
    /// Kontrollpunkt eines gebogenen Pfeils.
    pub control: Option<Point>,
}

/// Schaft von `from` bis `cut` Pixel vor `to`, gerade oder über `control`
/// gebogen. Die Kurve wird dabei abgeschnitten, nicht verformt.
fn shaft_path(from: Point, to: Point, control: Option<Point>, cut: f32) -> Option<Path> {
    let Some(c) = control else {
        let len = (to.x - from.x).hypot(to.y - from.y).max(f32::EPSILON);
        let k = (1.0 - cut / len).max(0.0);
        return line_path(
            from,
            Point::new(from.x + (to.x - from.x) * k, from.y + (to.y - from.y) * k),
        );
    };
    let at = |t: f32| {
        let u = 1.0 - t;
        Point::new(
            u * u * from.x + 2.0 * u * t * c.x + t * t * to.x,
            u * u * from.y + 2.0 * u * t * c.y + t * t * to.y,
        )
    };
    // Parameter, bei dem die Kurve `cut` vom Ende entfernt ist (Bisektion).
    let (mut lo, mut hi) = (0.0_f32, 1.0_f32);
    for _ in 0..24 {
        let mid = (lo + hi) / 2.0;
        let p = at(mid);
        if (to.x - p.x).hypot(to.y - p.y) > cut {
            lo = mid;
        } else {
            hi = mid;
        }
    }
    // De Casteljau: Teilkurve von 0 bis t hat den Kontrollpunkt from + t·(c − from).
    let t = lo;
    let end = at(t);
    let ctrl = Point::new(from.x + (c.x - from.x) * t, from.y + (c.y - from.y) * t);
    let mut pb = PathBuilder::new();
    pb.move_to(from.x, from.y);
    pb.quad_to(ctrl.x, ctrl.y, end.x, end.y);
    pb.finish()
}

pub(crate) fn arrow(
    pixmap: &mut Pixmap,
    from: Point,
    to: Point,
    shape: ArrowShape,
    opacity: f32,
    t: Transform,
) {
    let ArrowShape {
        stroke,
        head,
        head_size,
        control,
    } = shape;
    let len = (to.x - from.x).hypot(to.y - from.y);
    // Richtung der Spitze: bei einer Kurve die Tangente am Ende (vom Kontrollpunkt her).
    let towards = control.unwrap_or(from);
    let (dx, dy) = (to.x - towards.x, to.y - towards.y);
    let dir = dx.hypot(dy);
    if len < f32::EPSILON || dir < f32::EPSILON {
        return;
    }
    let (ux, uy) = (dx / dir, dy / dir);
    // Spitze wächst mit der Strichbreite, ist aber nie länger als der halbe Pfeil.
    let head_len = ((stroke.width * 4.0).max(10.0) * head_size.clamp(0.3, 4.0)).min(len / 2.0);
    let head_half = head_len * 0.6;
    let base = Point::new(to.x - ux * head_len, to.y - uy * head_len);
    let left = Point::new(base.x - uy * head_half, base.y + ux * head_half);
    let right = Point::new(base.x + uy * head_half, base.y - ux * head_half);

    match head {
        ArrowHead::Filled => {
            if let Some(shaft) = shaft_path(from, to, control, head_len) {
                stroke_path(pixmap, &shaft, stroke, opacity, t);
            }
            let mut pb = PathBuilder::new();
            pb.move_to(to.x, to.y);
            pb.line_to(left.x, left.y);
            pb.line_to(right.x, right.y);
            pb.close();
            if let Some(path) = pb.finish() {
                fill_path(pixmap, &path, stroke.color, opacity, t);
            }
        }
        ArrowHead::Open => {
            // Schaft bis kurz vor die Spitze, damit sich die runden Enden
            // nicht über die Spitze hinaus wölben.
            if let Some(shaft) = shaft_path(from, to, control, stroke.width * 0.5) {
                stroke_path(pixmap, &shaft, stroke, opacity, t);
            }
            // Die Spitze bleibt immer durchgezogen.
            let solid = Stroke::new(stroke.color, stroke.width);
            let mut pb = PathBuilder::new();
            pb.move_to(left.x, left.y);
            pb.line_to(to.x, to.y);
            pb.line_to(right.x, right.y);
            if let Some(path) = pb.finish() {
                stroke_path(pixmap, &path, &solid, opacity, t);
            }
        }
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
