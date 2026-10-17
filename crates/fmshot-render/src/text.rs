//! Text mit cosmic-text und der mitgelieferten Schrift Inter (OFL).
//!
//! Nur mitgelieferte Schriften werden geladen – nativ und im WASM dieselben,
//! damit Text überall gleich umbricht. Inter deckt Latein, Griechisch und
//! Kyrillisch ab; Emoji kommen farbig aus Twemoji (COLR, CC-BY 4.0), auf die
//! cosmic-text automatisch ausweicht.

use cosmic_text::fontdb::{Database, Source};
use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, Weight};
use fmshot_core::{Point, Rect, TextStyle};
use tiny_skia::{Pixmap, Transform};

use crate::shapes::{paint, rect_path};

static INTER_REGULAR: &[u8] = include_bytes!("../fonts/Inter-Regular.ttf");
static INTER_BOLD: &[u8] = include_bytes!("../fonts/Inter-Bold.ttf");
static TWEMOJI: &[u8] = include_bytes!("../fonts/Twemoji.Mozilla.ttf");

/// Familienname der eingebetteten Emoji-Schrift.
const EMOJI_FAMILY: &str = "Twemoji Mozilla";

/// Zeilenhöhe relativ zur Schriftgrösse.
const LINE_HEIGHT: f32 = 1.25;

/// Grösse eines gesetzten Textes in Dokument-Pixeln.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TextMetrics {
    pub width: f32,
    pub height: f32,
    pub line_height: f32,
}

pub(crate) struct TextEngine {
    fonts: FontSystem,
    cache: SwashCache,
}

impl TextEngine {
    pub fn new() -> Self {
        let mut db = Database::new();
        db.load_font_source(Source::Binary(std::sync::Arc::new(INTER_REGULAR)));
        db.load_font_source(Source::Binary(std::sync::Arc::new(INTER_BOLD)));
        db.load_font_source(Source::Binary(std::sync::Arc::new(TWEMOJI)));
        db.set_sans_serif_family("Inter");
        Self {
            fonts: FontSystem::new_with_locale_and_db("de-CH".into(), db),
            cache: SwashCache::new(),
        }
    }

    fn layout(&mut self, text: &str, style: &TextStyle, size: f32, wrap: Option<f32>) -> Buffer {
        let mut buffer = Buffer::new(
            &mut self.fonts,
            Metrics::new(size.max(1.0), size.max(1.0) * LINE_HEIGHT),
        );
        buffer.set_size(wrap.map(|w| w.max(size)), None);
        let attrs = Attrs::new()
            .family(Family::Name("Inter"))
            .weight(if style.bold {
                Weight::BOLD
            } else {
                Weight::NORMAL
            });
        let emoji = attrs.clone().family(Family::Name(EMOJI_FAMILY));
        let spans = split_emoji(text).into_iter().map(|(part, is_emoji)| {
            (
                part,
                if is_emoji {
                    emoji.clone()
                } else {
                    attrs.clone()
                },
            )
        });
        buffer.set_rich_text(spans, &attrs, Shaping::Advanced, None);
        buffer.shape_until_scroll(&mut self.fonts, false);
        buffer
    }

    pub fn measure(
        &mut self,
        text: &str,
        style: &TextStyle,
        max_width: Option<f32>,
    ) -> TextMetrics {
        let buffer = self.layout(text, style, style.size, max_width);
        let line_height = style.size * LINE_HEIGHT;
        let mut width: f32 = 0.0;
        let mut lines = 0;
        for run in buffer.layout_runs() {
            width = width.max(run.line_w);
            lines += 1;
        }
        TextMetrics {
            width: width.ceil(),
            height: line_height * lines.max(1) as f32,
            line_height,
        }
    }

    /// Innenabstand eines Textkastens mit Hinterlegung.
    pub fn padding(style: &TextStyle) -> f32 {
        if style.background.is_some() {
            style.size * 0.35
        } else {
            0.0
        }
    }

    /// Text in einem Kasten (oben links ausgerichtet, Umbruch an der Breite).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_box(
        &mut self,
        pixmap: &mut Pixmap,
        bounds: Rect,
        text: &str,
        style: &TextStyle,
        _align: Option<()>,
        opacity: f32,
        base: Transform,
        scale: f32,
    ) {
        if let Some(bg) = style.background
            && let Some(path) = rect_path(bounds, style.size * 0.25)
        {
            pixmap.fill_path(
                &path,
                &paint(bg, opacity),
                tiny_skia::FillRule::Winding,
                base,
                None,
            );
        }
        let pad = Self::padding(style);
        let origin = Point::new(bounds.x + pad, bounds.y + pad);
        let wrap = (bounds.width - 2.0 * pad).max(1.0);
        self.draw_at(
            pixmap,
            origin,
            text,
            style,
            Some(wrap),
            opacity,
            base,
            scale,
        );
    }

    /// Text mittig um `center` (z. B. Zahl im Nummern-Kreis).
    #[allow(clippy::too_many_arguments)]
    pub fn draw_centered(
        &mut self,
        pixmap: &mut Pixmap,
        center: Point,
        text: &str,
        style: &TextStyle,
        opacity: f32,
        base: Transform,
        scale: f32,
    ) {
        let m = self.measure(text, style, None);
        // Inter: optische Mitte der Ziffern liegt leicht über der Zeilenmitte.
        let origin = Point::new(
            center.x - m.width / 2.0,
            center.y - m.height / 2.0 + style.size * 0.02,
        );
        self.draw_at(pixmap, origin, text, style, None, opacity, base, scale);
    }

    /// Setzt den Text in Ausgabeauflösung (scharf bei jedem Zoom) und mischt
    /// die Glyphen direkt in die Pixmap. Bei gedrehtem `base` wird der Text
    /// erst ungedreht in ein eigenes Bild gesetzt und dieses dann gedreht
    /// eingefügt.
    #[allow(clippy::too_many_arguments)]
    fn draw_at(
        &mut self,
        pixmap: &mut Pixmap,
        origin: Point,
        text: &str,
        style: &TextStyle,
        wrap: Option<f32>,
        opacity: f32,
        base: Transform,
        scale: f32,
    ) {
        if base.kx != 0.0 || base.ky != 0.0 {
            self.draw_rotated(pixmap, origin, text, style, wrap, opacity, base, scale);
            return;
        }
        self.draw_straight(pixmap, origin, text, style, wrap, opacity, base, scale);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_rotated(
        &mut self,
        pixmap: &mut Pixmap,
        origin: Point,
        text: &str,
        style: &TextStyle,
        wrap: Option<f32>,
        opacity: f32,
        base: Transform,
        scale: f32,
    ) {
        let m = self.measure(text, style, wrap);
        // Etwas Rand für Unterlängen und Kantenglättung.
        let margin = style.size * 0.25;
        let w = ((wrap.unwrap_or(m.width).max(m.width) + 2.0 * margin) * scale).ceil() as u32;
        let h = ((m.height + 2.0 * margin) * scale).ceil() as u32;
        let Some(mut layer) = Pixmap::new(w.max(1), h.max(1)) else {
            return;
        };
        let local = Transform::from_scale(scale, scale)
            .pre_translate(-(origin.x - margin), -(origin.y - margin));
        self.draw_straight(&mut layer, origin, text, style, wrap, 1.0, local, scale);
        let place = base
            .pre_translate(origin.x - margin, origin.y - margin)
            .pre_scale(1.0 / scale, 1.0 / scale);
        let paint = tiny_skia::PixmapPaint {
            opacity: opacity.clamp(0.0, 1.0),
            quality: tiny_skia::FilterQuality::Bilinear,
            ..Default::default()
        };
        pixmap.draw_pixmap(0, 0, layer.as_ref(), &paint, place, None);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_straight(
        &mut self,
        pixmap: &mut Pixmap,
        origin: Point,
        text: &str,
        style: &TextStyle,
        wrap: Option<f32>,
        opacity: f32,
        base: Transform,
        scale: f32,
    ) {
        let mut pts = [tiny_skia::Point::from_xy(origin.x, origin.y)];
        base.map_points(&mut pts);
        let (ox, oy) = (pts[0].x.round() as i32, pts[0].y.round() as i32);
        let size = style.size * scale;
        if size < 1.0 {
            return;
        }
        let mut buffer = self.layout(text, style, size, wrap.map(|w| w * scale + 1.0));
        let c = style.color;
        // Deckkraft erst beim Mischen: farbige Emoji bringen eigene Farben mit
        // und ignorieren die Textfarbe samt Alpha.
        let color = cosmic_text::Color::rgba(c.r, c.g, c.b, c.a);
        let opacity = (opacity.clamp(0.0, 1.0) * 255.0).round() as u32;
        let (pw, ph) = (pixmap.width() as i32, pixmap.height() as i32);
        let stride = pw as usize * 4;
        let data = pixmap.data_mut();
        buffer.draw(
            &mut self.fonts,
            &mut self.cache,
            color,
            |x, y, w, h, col| {
                let a = col.a() as u32 * opacity / 255;
                if a == 0 {
                    return;
                }
                for yy in 0..h as i32 {
                    let py = oy + y + yy;
                    if py < 0 || py >= ph {
                        continue;
                    }
                    for xx in 0..w as i32 {
                        let px = ox + x + xx;
                        if px < 0 || px >= pw {
                            continue;
                        }
                        let i = py as usize * stride + px as usize * 4;
                        // Quelle vormultiplizieren, dann „source over“.
                        let src = [
                            col.r() as u32 * a / 255,
                            col.g() as u32 * a / 255,
                            col.b() as u32 * a / 255,
                            a,
                        ];
                        let inv = 255 - a;
                        for k in 0..4 {
                            data[i + k] =
                                (src[k] + (data[i + k] as u32 * inv + 127) / 255).min(255) as u8;
                        }
                    }
                }
            },
        );
    }
}

/// Zerlegt Text in Abschnitte aus Emoji und übrigem Text. Emoji-Folgen
/// werden ausdrücklich in Twemoji gesetzt: Inter enthält einige Zeichen wie
/// ❤ selbst und würde sie sonst schwarz statt farbig zeichnen, obwohl U+FE0F
/// die Emoji-Darstellung verlangt.
fn split_emoji(text: &str) -> Vec<(&str, bool)> {
    use unicode_segmentation::UnicodeSegmentation;
    let mut out: Vec<(&str, bool)> = Vec::new();
    let mut start = 0;
    let mut current: Option<bool> = None;
    for (i, cluster) in text.grapheme_indices(true) {
        let e = is_emoji_cluster(cluster);
        if current.is_some_and(|c| c != e) {
            out.push((&text[start..i], current.unwrap_or(false)));
            start = i;
        }
        current = Some(e);
    }
    if start < text.len() {
        out.push((&text[start..], current.unwrap_or(false)));
    }
    out
}

/// Ein Graphem gilt als Emoji, wenn es die Emoji-Darstellung verlangt
/// (U+FE0F, Tastenkappe U+20E3, ZWJ-Folge) oder aus den Emoji-Blöcken ab
/// U+1F000 stammt (inkl. Flaggen aus Regional-Indikatoren).
fn is_emoji_cluster(cluster: &str) -> bool {
    cluster
        .chars()
        .any(|c| matches!(c, '\u{FE0F}' | '\u{20E3}' | '\u{200D}') || c as u32 >= 0x1F000)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn emoji_abschnitte() {
        assert_eq!(split_emoji("Hallo"), vec![("Hallo", false)]);
        assert_eq!(
            split_emoji("Ja ❤️ und 👍🏽!"),
            vec![
                ("Ja ", false),
                ("❤️", true),
                (" und ", false),
                ("👍🏽", true),
                ("!", false)
            ]
        );
        assert_eq!(split_emoji("❤"), vec![("❤", false)]);
        assert_eq!(split_emoji("🇨🇭1️⃣"), vec![("🇨🇭1️⃣", true)]);
        assert_eq!(split_emoji(""), vec![]);
    }
}
