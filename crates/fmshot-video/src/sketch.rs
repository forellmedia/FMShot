//! Live zeichnen während der Aufnahme (ADR 0011): Pfeile, Linien, Rahmen,
//! Kreise, Freihand, Nummern und Textmarker direkt auf dem Bildschirm. Nichts
//! wird ins Bildschirmvideo eingebrannt – jede Zeichnung wird eine Ebene mit
//! Zeitbereich und bleibt im Editor änderbar.
//!
//! Was gerade zu sehen ist, bildet eine Gruppe: Sie verschwindet gemeinsam
//! [`HOLD_MS`] nach der letzten Zeichnung oder sofort mit Esc. Danach
//! beginnen die Nummern wieder bei 1.
//!
//! Koordinaten in Pixeln der Aufnahme (wie das Dokument), Grössen der
//! Werkzeuge in Punkten × `pixel_ratio` wie im Editor.

use fmshot_core::{
    ArrowHead, Color, Document, LayerKind, LineStyle, Point, Rect, RgbaImage, Stroke,
};
use fmshot_render::{RenderOptions, Renderer};

/// So lange bleibt eine Gruppe nach der letzten Zeichnung stehen.
pub const HOLD_MS: u64 = 3000;
/// Deckkraft des Textmarkers (auf durchsichtigem Grund wirkt „Multiplizieren“
/// nicht, darum halbdurchsichtig).
pub const HIGHLIGHT_OPACITY: f32 = 0.45;
/// Kürzere Striche (Punkte) gelten als Klick und werden verworfen.
const MIN_POINTS: f32 = 3.0;

/// Werkzeuge der Zeichenleiste.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SketchTool {
    Arrow,
    Line,
    Rectangle,
    Ellipse,
    Pen,
    Step,
    Highlight,
}

impl SketchTool {
    pub const ALL: [SketchTool; 7] = [
        SketchTool::Arrow,
        SketchTool::Line,
        SketchTool::Rectangle,
        SketchTool::Ellipse,
        SketchTool::Pen,
        SketchTool::Step,
        SketchTool::Highlight,
    ];

    /// Taste wie im Editor.
    pub fn key(self) -> char {
        match self {
            SketchTool::Arrow => 'a',
            SketchTool::Line => 'l',
            SketchTool::Rectangle => 'r',
            SketchTool::Ellipse => 'o',
            SketchTool::Pen => 'p',
            SketchTool::Step => 'n',
            SketchTool::Highlight => 'h',
        }
    }
}

/// Farben der Leiste (wie die ersten Farben im Editor).
pub const SKETCH_COLORS: [Color; 4] = [
    Color::rgb(0xe5, 0x39, 0x35),
    Color::rgb(0xfd, 0xd8, 0x35),
    Color::rgb(0x43, 0xa0, 0x47),
    Color::rgb(0x1e, 0x88, 0xe5),
];
const HIGHLIGHT_YELLOW: Color = Color::rgb(0xff, 0xeb, 0x3b);

/// Zwei Uhren: `rec_ms` = Zeit im Video (ohne Pausen) für die Ebenen,
/// `wall_ms` = Uhrzeit fürs Verschwinden (läuft auch in der Pause).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Clock {
    pub rec_ms: u32,
    pub wall_ms: u64,
}

/// Fertige Zeichnung: Ebene und wann sie zu sehen ist (Zeit im Video).
#[derive(Debug, Clone, PartialEq)]
pub struct SketchItem {
    pub kind: LayerKind,
    pub opacity: f32,
    pub start_ms: u32,
    pub end_ms: u32,
}

#[derive(Debug, Clone)]
struct Drawn {
    kind: LayerKind,
    start_ms: u32,
}

#[derive(Debug, Clone)]
struct Active {
    start: Point,
    kind: LayerKind,
    start_ms: u32,
}

/// Zustand der Zeichenfläche während einer Aufnahme.
pub struct LiveSketch {
    size: (u32, u32),
    pixel_ratio: f32,
    pub tool: SketchTool,
    pub color: Color,
    /// Sichtbare Gruppe.
    visible: Vec<Drawn>,
    active: Option<Active>,
    /// Wann die Gruppe verschwindet (Uhrzeit).
    expires: Option<u64>,
    done: Vec<SketchItem>,
    renderer: Renderer,
}

impl LiveSketch {
    /// `size` = aufgenommene Fläche in Pixeln, `pixel_ratio` = Pixel je Punkt.
    pub fn new(size: (u32, u32), pixel_ratio: f32) -> Self {
        Self {
            size,
            pixel_ratio: pixel_ratio.max(1.0),
            tool: SketchTool::Arrow,
            color: SKETCH_COLORS[0],
            visible: Vec::new(),
            active: None,
            expires: None,
            done: Vec::new(),
            renderer: Renderer::new(),
        }
    }

    fn px(&self, points: f32) -> f32 {
        points * self.pixel_ratio
    }

    /// Ist etwas zu sehen (oder wird gerade gezeichnet)?
    pub fn is_empty(&self) -> bool {
        self.visible.is_empty() && self.active.is_none()
    }

    fn next_step(&self) -> u32 {
        let steps = self
            .visible
            .iter()
            .filter(|d| matches!(d.kind, LayerKind::Step { .. }))
            .count();
        steps as u32 + 1
    }

    /// Maustaste gedrückt.
    pub fn press(&mut self, p: Point, clock: Clock) {
        let stroke = Stroke::new(self.color, self.px(3.0));
        let empty = Rect::new(p.x, p.y, 0.0, 0.0);
        let kind = match self.tool {
            SketchTool::Arrow => LayerKind::Arrow {
                from: p,
                to: p,
                stroke,
                head: ArrowHead::Filled,
                head_size: 1.6,
                control: None,
            },
            SketchTool::Line => LayerKind::Line {
                from: p,
                to: p,
                stroke,
            },
            SketchTool::Rectangle => LayerKind::Rectangle {
                bounds: empty,
                stroke,
                fill: None,
                corner_radius: self.px(4.0),
            },
            SketchTool::Ellipse => LayerKind::Ellipse {
                bounds: empty,
                stroke,
                fill: None,
            },
            SketchTool::Pen => LayerKind::Pen {
                points: vec![p],
                stroke: Stroke {
                    style: LineStyle::Solid,
                    ..stroke
                },
            },
            SketchTool::Step => LayerKind::Step {
                center: p,
                radius: self.px(14.0),
                number: self.next_step(),
                fill: self.color,
                text_color: Color::rgb(255, 255, 255),
            },
            SketchTool::Highlight => LayerKind::Highlight {
                bounds: empty,
                color: HIGHLIGHT_YELLOW,
            },
        };
        self.active = Some(Active {
            start: p,
            kind,
            start_ms: clock.rec_ms,
        });
        // Solange gezeichnet wird, verschwindet nichts.
        self.expires = None;
    }

    /// Maus gezogen; `shift` = Linien in 45°-Schritten, Rahmen quadratisch.
    pub fn drag(&mut self, p: Point, shift: bool) {
        let Some(active) = self.active.as_mut() else {
            return;
        };
        let start = active.start;
        let boxed = |p: Point| {
            if !shift {
                return rect_from(start, p);
            }
            let d = (p.x - start.x).abs().max((p.y - start.y).abs());
            let sign = |v: f32| if v < 0.0 { -1.0 } else { 1.0 };
            rect_from(
                start,
                Point::new(
                    start.x + sign(p.x - start.x) * d,
                    start.y + sign(p.y - start.y) * d,
                ),
            )
        };
        let snapped = |p: Point| {
            if !shift {
                return p;
            }
            let step = std::f32::consts::FRAC_PI_4;
            let a = ((p.y - start.y).atan2(p.x - start.x) / step).round() * step;
            let len = (p.x - start.x).hypot(p.y - start.y);
            Point::new(start.x + a.cos() * len, start.y + a.sin() * len)
        };
        match &mut active.kind {
            LayerKind::Arrow { to, .. } | LayerKind::Line { to, .. } => *to = snapped(p),
            LayerKind::Rectangle { bounds, .. }
            | LayerKind::Ellipse { bounds, .. }
            | LayerKind::Highlight { bounds, .. } => *bounds = boxed(p),
            LayerKind::Pen { points, .. } => points.push(p),
            LayerKind::Step { center, .. } => *center = p,
            _ => {}
        }
    }

    /// Maustaste los: Die Zeichnung kommt in die Gruppe (zu kleine
    /// Zeichnungen, z. B. ein Klick mit dem Pfeil, fallen weg).
    pub fn release(&mut self, p: Point, shift: bool, clock: Clock) {
        self.drag(p, shift);
        if let Some(active) = self.active.take()
            && !self.too_small(&active.kind)
        {
            self.visible.push(Drawn {
                kind: active.kind,
                start_ms: active.start_ms,
            });
        }
        self.expires = (!self.visible.is_empty()).then_some(clock.wall_ms + HOLD_MS);
    }

    fn too_small(&self, kind: &LayerKind) -> bool {
        let min = self.px(MIN_POINTS);
        match kind {
            LayerKind::Arrow { from, to, .. } | LayerKind::Line { from, to, .. } => {
                (to.x - from.x).hypot(to.y - from.y) < min
            }
            LayerKind::Pen { points, .. } => points.len() < 2,
            LayerKind::Step { .. } => false,
            k => {
                let b = k.bounds();
                b.width < min || b.height < min
            }
        }
    }

    /// Gruppe abgelaufen? Gibt zurück, ob sich etwas geändert hat.
    pub fn tick(&mut self, clock: Clock) -> bool {
        match self.expires {
            Some(t) if clock.wall_ms >= t && self.active.is_none() => {
                self.close_group(clock.rec_ms);
                true
            }
            _ => false,
        }
    }

    /// Esc: Gruppe sofort entfernen. `false`, wenn nichts zu sehen war.
    pub fn clear(&mut self, clock: Clock) -> bool {
        if self.is_empty() {
            return false;
        }
        self.active = None;
        self.close_group(clock.rec_ms);
        true
    }

    fn close_group(&mut self, end_ms: u32) {
        for d in self.visible.drain(..) {
            let opacity = layer_opacity(&d.kind);
            self.done.push(SketchItem {
                // Mindestens ein Bild lang zu sehen.
                end_ms: end_ms.max(d.start_ms + 34),
                start_ms: d.start_ms,
                opacity,
                kind: d.kind,
            });
        }
        self.expires = None;
    }

    /// Ende der Aufnahme: Was noch zu sehen ist, endet hier.
    pub fn finish(&mut self, end_ms: u32) -> Vec<SketchItem> {
        self.active = None;
        self.close_group(end_ms);
        std::mem::take(&mut self.done)
    }

    /// Alles, was jetzt zu sehen ist, auf durchsichtigem Grund. `scale` =
    /// Ausgabepixel je Pixel der Aufnahme (z. B. `1 / pixel_ratio` für
    /// Punkte). `None`, wenn nichts zu sehen ist.
    pub fn render(&mut self, scale: f32) -> Option<RgbaImage> {
        if self.is_empty() {
            return None;
        }
        let mut doc = Document::from_capture(self.size.0, self.size.1);
        doc.layers.clear();
        doc.pixel_ratio = self.pixel_ratio;
        let kinds = self
            .visible
            .iter()
            .map(|d| &d.kind)
            .chain(self.active.as_ref().map(|a| &a.kind));
        for kind in kinds {
            let opacity = layer_opacity(kind);
            let id = doc.push_layer(kind.clone());
            if let Some(l) = doc.layers.iter_mut().find(|l| l.id == id) {
                l.opacity = opacity;
            }
        }
        let opts = RenderOptions {
            background: false,
            scale: scale.clamp(0.1, 1.0),
            ..RenderOptions::full(&doc)
        };
        match self
            .renderer
            .render_with(&doc, &std::collections::HashMap::new(), &opts)
        {
            Ok(img) => Some(img),
            Err(e) => {
                log::warn!("Zeichnung: {e}");
                None
            }
        }
    }
}

fn rect_from(a: Point, b: Point) -> Rect {
    Rect::new(
        a.x.min(b.x),
        a.y.min(b.y),
        (a.x - b.x).abs(),
        (a.y - b.y).abs(),
    )
}

fn layer_opacity(kind: &LayerKind) -> f32 {
    match kind {
        LayerKind::Highlight { .. } => HIGHLIGHT_OPACITY,
        _ => 1.0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn at(rec_ms: u32, wall_ms: u64) -> Clock {
        Clock { rec_ms, wall_ms }
    }

    fn draw(s: &mut LiveSketch, a: (f32, f32), b: (f32, f32), clock: Clock) {
        s.press(Point::new(a.0, a.1), clock);
        s.drag(Point::new((a.0 + b.0) / 2.0, (a.1 + b.1) / 2.0), false);
        s.release(Point::new(b.0, b.1), false, clock);
    }

    #[test]
    fn gruppe_verschwindet_nach_der_letzten_zeichnung() {
        let mut s = LiveSketch::new((400, 300), 2.0);
        draw(&mut s, (10.0, 10.0), (100.0, 80.0), at(1000, 50_000));
        s.tool = SketchTool::Rectangle;
        draw(&mut s, (20.0, 20.0), (120.0, 90.0), at(2000, 51_000));
        // 3 s nach der ersten, aber nicht nach der letzten: bleibt
        assert!(!s.tick(at(4000, 53_500)));
        assert!(s.tick(at(5000, 54_000)));
        assert!(s.is_empty());
        let items = s.finish(9000);
        assert_eq!(items.len(), 2);
        assert_eq!((items[0].start_ms, items[0].end_ms), (1000, 5000));
        assert_eq!((items[1].start_ms, items[1].end_ms), (2000, 5000));
        assert!(matches!(items[1].kind, LayerKind::Rectangle { .. }));
    }

    #[test]
    fn esc_entfernt_sofort_und_klicks_zaehlen_nicht() {
        let mut s = LiveSketch::new((400, 300), 1.0);
        assert!(!s.clear(at(0, 0)));
        // Nur geklickt: kein Pfeil
        draw(&mut s, (10.0, 10.0), (11.0, 10.0), at(500, 1000));
        assert!(s.is_empty());
        draw(&mut s, (10.0, 10.0), (90.0, 10.0), at(700, 1200));
        assert!(s.clear(at(900, 1400)));
        assert!(s.is_empty());
        let items = s.finish(5000);
        assert_eq!(items.len(), 1);
        assert_eq!((items[0].start_ms, items[0].end_ms), (700, 900));
    }

    #[test]
    fn nummern_beginnen_nach_dem_verschwinden_wieder_bei_eins() {
        let mut s = LiveSketch::new((400, 300), 1.0);
        s.tool = SketchTool::Step;
        let number = |s: &LiveSketch| match s.visible.last().map(|d| &d.kind) {
            Some(LayerKind::Step { number, .. }) => *number,
            _ => 0,
        };
        for (i, x) in [10.0, 50.0, 90.0].into_iter().enumerate() {
            s.press(Point::new(x, 10.0), at(i as u32 * 100, i as u64 * 100));
            s.release(
                Point::new(x, 10.0),
                false,
                at(i as u32 * 100, i as u64 * 100),
            );
            assert_eq!(number(&s), i as u32 + 1);
        }
        assert!(s.tick(at(10_000, 10_000)));
        s.press(Point::new(10.0, 10.0), at(11_000, 11_000));
        s.release(Point::new(10.0, 10.0), false, at(11_000, 11_000));
        assert_eq!(number(&s), 1);
    }

    #[test]
    fn shift_rastet_ein_und_ende_der_aufnahme_schliesst_ab() {
        let mut s = LiveSketch::new((400, 300), 1.0);
        s.tool = SketchTool::Line;
        s.press(Point::new(0.0, 0.0), at(0, 0));
        s.release(Point::new(100.0, 8.0), true, at(0, 0));
        match &s.visible[0].kind {
            LayerKind::Line { to, .. } => assert!(to.y.abs() < 0.01 && (to.x - 100.3).abs() < 0.5),
            k => panic!("{k:?}"),
        }
        s.tool = SketchTool::Highlight;
        draw(&mut s, (0.0, 0.0), (50.0, 20.0), at(100, 100));
        let items = s.finish(2000);
        assert_eq!(items.len(), 2);
        assert!(items.iter().all(|i| i.end_ms == 2000));
        assert_eq!(items[1].opacity, HIGHLIGHT_OPACITY);
    }

    #[test]
    fn zeichnung_wird_sichtbar_gerendert() {
        let mut s = LiveSketch::new((200, 100), 2.0);
        assert!(s.render(0.5).is_none());
        draw(&mut s, (20.0, 50.0), (180.0, 50.0), at(0, 0));
        let img = s.render(0.5).unwrap();
        assert_eq!((img.width(), img.height()), (100, 50));
        let alpha = |x: u32, y: u32| img.data()[((y * 100 + x) * 4 + 3) as usize];
        assert!(alpha(50, 25) > 0);
        assert_eq!(alpha(50, 5), 0);
    }
}
