//! Text mit cosmic-text und der mitgelieferten Schrift Inter (OFL).
//!
//! Nur Inter wird geladen – nativ und im WASM dieselbe Schrift, damit Text
//! überall gleich umbricht. Inter deckt Latein, Griechisch und Kyrillisch ab;
//! Emoji folgen später.

use cosmic_text::fontdb::{Database, Source};
use cosmic_text::{Attrs, Buffer, Family, FontSystem, Metrics, Shaping, SwashCache, Weight};
use fmshot_core::{Point, Rect, TextStyle};
use tiny_skia::{Pixmap, Transform};

use crate::shapes::{paint, rect_path};

static INTER_REGULAR: &[u8] = include_bytes!("../fonts/Inter-Regular.ttf");
static INTER_BOLD: &[u8] = include_bytes!("../fonts/Inter-Bold.ttf");

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
        buffer.set_text(text, &attrs, Shaping::Advanced, None);
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
    /// die Glyphen direkt in die Pixmap.
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
        let mut pts = [tiny_skia::Point::from_xy(origin.x, origin.y)];
        base.map_points(&mut pts);
        let (ox, oy) = (pts[0].x.round() as i32, pts[0].y.round() as i32);
        let size = style.size * scale;
        if size < 1.0 {
            return;
        }
        let mut buffer = self.layout(text, style, size, wrap.map(|w| w * scale + 1.0));
        let c = style.color;
        let color = cosmic_text::Color::rgba(c.r, c.g, c.b, ((c.a as f32) * opacity) as u8);
        let (pw, ph) = (pixmap.width() as i32, pixmap.height() as i32);
        let stride = pw as usize * 4;
        let data = pixmap.data_mut();
        buffer.draw(
            &mut self.fonts,
            &mut self.cache,
            color,
            |x, y, w, h, col| {
                let a = col.a() as u32;
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
