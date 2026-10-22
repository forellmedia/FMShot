//! Effekte, die auf das bereits Gezeichnete wirken: Weichzeichnen, Verpixeln,
//! Spotlight, Lupe, Schatten.
//!
//! Effekte arbeiten achsenparallel im Ausgabebild (Drehung wird ignoriert).

use nabshot_core::{Corner, Rect, Shadow, Stroke};
use tiny_skia::{
    BlendMode, FillRule, FilterQuality, Paint, PathBuilder, Pattern, Pixmap, PixmapPaint,
    SpreadMode, Transform,
};

use crate::shapes::{paint, rect_path, stroke_path};

/// Dokument-Rechteck → ganzzahliges Pixel-Rechteck im Ausgabebild (begrenzt).
fn device_rect(pixmap: &Pixmap, r: Rect, t: Transform) -> Option<(usize, usize, usize, usize)> {
    let mut pts = [
        tiny_skia::Point::from_xy(r.x, r.y),
        tiny_skia::Point::from_xy(r.right(), r.bottom()),
    ];
    t.map_points(&mut pts);
    let x0 = pts[0].x.min(pts[1].x).floor().max(0.0) as usize;
    let y0 = pts[0].y.min(pts[1].y).floor().max(0.0) as usize;
    let x1 = (pts[0].x.max(pts[1].x).ceil() as usize).min(pixmap.width() as usize);
    let y1 = (pts[0].y.max(pts[1].y).ceil() as usize).min(pixmap.height() as usize);
    (x1 > x0 && y1 > y0).then_some((x0, y0, x1 - x0, y1 - y0))
}

pub(crate) fn blur(pixmap: &mut Pixmap, bounds: Rect, radius: f32, t: Transform) {
    let Some((x, y, w, h)) = device_rect(pixmap, bounds, t) else {
        return;
    };
    let r = radius.round().max(1.0) as usize;
    let stride = pixmap.width() as usize * 4;
    box_blur_region(pixmap.data_mut(), stride, x, y, w, h, r);
}

/// Drei Durchgänge Kastenfilter ≈ Gauss. Arbeitet auf vormultiplizierten Daten.
fn box_blur_region(
    data: &mut [u8],
    stride: usize,
    x: usize,
    y: usize,
    w: usize,
    h: usize,
    r: usize,
) {
    let mut buf: Vec<[u32; 4]> = Vec::with_capacity(w * h);
    for row in y..y + h {
        for col in x..x + w {
            let i = row * stride + col * 4;
            buf.push([
                data[i] as u32,
                data[i + 1] as u32,
                data[i + 2] as u32,
                data[i + 3] as u32,
            ]);
        }
    }
    let mut tmp = buf.clone();
    for _ in 0..3 {
        blur_pass(&buf, &mut tmp, w, h, r, true);
        blur_pass(&tmp, &mut buf, w, h, r, false);
    }
    for (k, px) in buf.iter().enumerate() {
        let (row, col) = (y + k / w, x + k % w);
        let i = row * stride + col * 4;
        for c in 0..4 {
            data[i + c] = px[c].min(255) as u8;
        }
    }
}

/// Gleitender Mittelwert über ±r, Ränder werden fortgesetzt.
fn blur_pass(
    src: &[[u32; 4]],
    dst: &mut [[u32; 4]],
    w: usize,
    h: usize,
    r: usize,
    horizontal: bool,
) {
    let (lines, len) = if horizontal { (h, w) } else { (w, h) };
    let at = |line: usize, pos: usize| {
        if horizontal {
            line * w + pos
        } else {
            pos * w + line
        }
    };
    let n = (2 * r + 1) as u32;
    for line in 0..lines {
        let mut sum = [0u32; 4];
        for k in 0..=2 * r {
            let pos = k.saturating_sub(r).min(len - 1);
            let px = src[at(line, pos)];
            for c in 0..4 {
                sum[c] += px[c];
            }
        }
        for pos in 0..len {
            let out = &mut dst[at(line, pos)];
            for c in 0..4 {
                out[c] = sum[c] / n;
            }
            let add = (pos + r + 1).min(len - 1);
            let sub = pos.saturating_sub(r);
            let (pa, ps) = (src[at(line, add)], src[at(line, sub)]);
            for c in 0..4 {
                sum[c] = sum[c] + pa[c] - ps[c];
            }
        }
    }
}

pub(crate) fn pixelate(pixmap: &mut Pixmap, bounds: Rect, block: f32, t: Transform) {
    let Some((x, y, w, h)) = device_rect(pixmap, bounds, t) else {
        return;
    };
    let block = block.round().max(2.0) as usize;
    let stride = pixmap.width() as usize * 4;
    let data = pixmap.data_mut();
    for by in (y..y + h).step_by(block) {
        for bx in (x..x + w).step_by(block) {
            let (ex, ey) = ((bx + block).min(x + w), (by + block).min(y + h));
            let mut sum = [0u32; 4];
            for row in by..ey {
                for col in bx..ex {
                    let i = row * stride + col * 4;
                    for c in 0..4 {
                        sum[c] += data[i + c] as u32;
                    }
                }
            }
            let n = ((ex - bx) * (ey - by)) as u32;
            let avg = sum.map(|v| ((v + n / 2) / n) as u8);
            for row in by..ey {
                for col in bx..ex {
                    let i = row * stride + col * 4;
                    data[i..i + 4].copy_from_slice(&avg);
                }
            }
        }
    }
}

/// Wie das Spotlight aussieht (Grössen in Dokument-Pixeln).
pub(crate) struct SpotlightShape {
    pub bounds: Rect,
    /// 0.0 bis 1.0
    pub dim: f32,
    pub corner_radius: f32,
    /// Breite des weichen Übergangs.
    pub feather: f32,
}

/// Dunkelt alles ausser `bounds` ab. `scale` = Ausgabe-Pixel pro Dokument-Pixel.
pub(crate) fn spotlight(pixmap: &mut Pixmap, shape: SpotlightShape, t: Transform, scale: f32) {
    let SpotlightShape {
        bounds,
        dim,
        corner_radius,
        feather,
    } = shape;
    let Some(mut shade) = Pixmap::new(pixmap.width(), pixmap.height()) else {
        return;
    };
    shade.fill(
        tiny_skia::Color::from_rgba(0.0, 0.0, 0.0, dim.clamp(0.0, 1.0))
            .unwrap_or(tiny_skia::Color::BLACK),
    );
    if let Some(hole) = rect_path(bounds, corner_radius) {
        let clear = Paint {
            blend_mode: BlendMode::Clear,
            anti_alias: true,
            ..Paint::default()
        };
        shade.fill_path(&hole, &clear, FillRule::Winding, t, None);
        if feather > 0.0 {
            // Nur um den Rand weichzeichnen – weiter aussen ist alles gleich dunkel.
            let reach = feather * 2.0;
            let around = Rect::new(
                bounds.x - reach,
                bounds.y - reach,
                bounds.width + 2.0 * reach,
                bounds.height + 2.0 * reach,
            );
            blur(&mut shade, around, feather * scale, t);
        }
    }
    pixmap.draw_pixmap(
        0,
        0,
        shade.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
}

/// Runde Lupe: zeigt `source` vergrössert in `bounds`, auf Wunsch mit Griff.
pub(crate) fn magnifier(
    pixmap: &mut Pixmap,
    bounds: Rect,
    source: Rect,
    stroke: &Stroke,
    handle: Option<Corner>,
    t: Transform,
) {
    let snapshot = pixmap.clone();
    let mut pts = [
        tiny_skia::Point::from_xy(source.x, source.y),
        tiny_skia::Point::from_xy(source.right(), source.bottom()),
        tiny_skia::Point::from_xy(bounds.x, bounds.y),
        tiny_skia::Point::from_xy(bounds.right(), bounds.bottom()),
    ];
    t.map_points(&mut pts);
    let (sw, sh) = (
        (pts[1].x - pts[0].x).max(1.0),
        (pts[1].y - pts[0].y).max(1.0),
    );
    let (tw, th) = (pts[3].x - pts[2].x, pts[3].y - pts[2].y);
    // Muster-Koordinaten (Ausgabebild) → vergrösserte Lupe
    let pattern_t = Transform::from_translate(pts[2].x, pts[2].y)
        .pre_scale(tw / sw, th / sh)
        .pre_translate(-pts[0].x, -pts[0].y);
    let Some(oval) = tiny_skia::Rect::from_xywh(pts[2].x, pts[2].y, tw.max(1.0), th.max(1.0))
        .and_then(PathBuilder::from_oval)
    else {
        return;
    };
    let p = Paint {
        shader: Pattern::new(
            snapshot.as_ref(),
            SpreadMode::Pad,
            FilterQuality::Bicubic,
            1.0,
            pattern_t,
        ),
        anti_alias: true,
        ..Paint::default()
    };
    pixmap.fill_path(&oval, &p, FillRule::Winding, Transform::identity(), None);
    let scale = t.sx.abs().max(0.01);
    let device_stroke = Stroke {
        width: stroke.width * scale,
        ..*stroke
    };
    if let Some(corner) = handle {
        let (rx, ry) = (tw.abs() / 2.0, th.abs() / 2.0);
        let (cx, cy) = (pts[2].x + tw / 2.0, pts[2].y + th / 2.0);
        let (sx, sy) = match corner {
            Corner::TopLeft => (-1.0, -1.0),
            Corner::TopRight => (1.0, -1.0),
            Corner::BottomLeft => (-1.0, 1.0),
            Corner::BottomRight => (1.0, 1.0),
        };
        let d = std::f32::consts::FRAC_1_SQRT_2;
        // Vom Rand nach aussen, etwa drei Viertel des Radius lang.
        let mut pb = PathBuilder::new();
        pb.move_to(cx + sx * d * rx, cy + sy * d * ry);
        pb.line_to(cx + sx * d * rx * 1.75, cy + sy * d * ry * 1.75);
        if let Some(path) = pb.finish() {
            let grip = Stroke {
                width: (device_stroke.width * 2.5).max(rx.min(ry) * 0.16),
                style: nabshot_core::LineStyle::Solid,
                ..device_stroke
            };
            stroke_path(pixmap, &path, &grip, 1.0, Transform::identity());
        }
    }
    stroke_path(pixmap, &oval, &device_stroke, 1.0, Transform::identity());
}

/// Weicher Schatten unter einem (abgerundeten) Rechteck.
pub(crate) fn shadow(
    pixmap: &mut Pixmap,
    bounds: Rect,
    corner_radius: f32,
    shadow: &Shadow,
    t: Transform,
    scale: f32,
) {
    let blur = (shadow.blur * scale).max(0.0);
    let margin = (blur * 3.0).ceil() + 2.0;
    let shifted = Rect::new(
        bounds.x,
        bounds.y + shadow.offset_y,
        bounds.width,
        bounds.height,
    );
    let mut pts = [
        tiny_skia::Point::from_xy(shifted.x, shifted.y),
        tiny_skia::Point::from_xy(shifted.right(), shifted.bottom()),
    ];
    t.map_points(&mut pts);
    let ox = pts[0].x.min(pts[1].x) - margin;
    let oy = pts[0].y.min(pts[1].y) - margin;
    let w = ((pts[1].x - pts[0].x).abs() + 2.0 * margin).ceil() as u32;
    let h = ((pts[1].y - pts[0].y).abs() + 2.0 * margin).ceil() as u32;
    // Riesige Schatten (stark gezoomt) nicht als eigenes Bild anlegen.
    if w == 0 || h == 0 || w as u64 * h as u64 > 64_000_000 {
        return;
    }
    let Some(mut layer) = Pixmap::new(w, h) else {
        return;
    };
    if let Some(path) = rect_path(shifted, corner_radius) {
        let local = Transform::from_translate(-ox, -oy).pre_concat(t);
        layer.fill_path(
            &path,
            &paint(shadow.color, 1.0),
            FillRule::Winding,
            local,
            None,
        );
    }
    if blur >= 1.0 {
        let stride = w as usize * 4;
        box_blur_region(
            layer.data_mut(),
            stride,
            0,
            0,
            w as usize,
            h as usize,
            (blur / 2.0).round().max(1.0) as usize,
        );
    }
    pixmap.draw_pixmap(
        ox.round() as i32,
        oy.round() as i32,
        layer.as_ref(),
        &PixmapPaint::default(),
        Transform::identity(),
        None,
    );
}
