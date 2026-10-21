//! Eigener Mauszeiger, Klicks und Zoom (ADR 0011, Etappe 5).
//!
//! Aufgenommen wird der Bildschirm ohne Zeiger; die Mausspur
//! (`<id>_input.json`) sagt, wo er war und wann geklickt wurde. Daraus
//! entstehen beim Zeichnen der Zeiger, Kreise um Klicks und – auf Wunsch –
//! Zoom-Abschnitte, die dorthin vergrössern, wo geklickt wird.

use grabmark_core::{InputSample, InputTrack, Rect, ZoomSection};

/// So lange ist der Kreis um einen Klick zu sehen.
pub const CLICK_MS: f64 = 600.0;
/// Weicher Übergang in den Zoom hinein und wieder hinaus.
pub const ZOOM_EASE_MS: f64 = 500.0;
/// Klicks mit weniger Abstand gehören zu einem Zoom.
const AUTO_GAP_MS: u32 = 3000;
/// Vorlauf und Nachlauf eines Auto-Zooms um die Klicks.
const AUTO_BEFORE_MS: u32 = 700;
const AUTO_AFTER_MS: u32 = 1800;
/// Vergrösserung beim Auto-Zoom.
const AUTO_SCALE: f32 = 1.8;
/// Normale Zeigergrösse in Bildschirmpunkten (Höhe des Pfeils).
pub const CURSOR_POINTS: f32 = 24.0;
/// So weit darf ein gemeldeter Klick in Grabmark selbst von der Probe der
/// Mausspur abweichen (Abtastung alle 16 ms, Meldung im Haupt-Thread).
const OWN_CLICK_SLACK_MS: u32 = 250;

/// Ein Klick (Taste gedrückt) an einer Stelle.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Click {
    pub t_ms: u32,
    pub x: f32,
    pub y: f32,
}

/// Mausspur, aufbereitet zum schnellen Nachschlagen.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct CursorTrack {
    samples: Vec<InputSample>,
    clicks: Vec<Click>,
}

impl CursorTrack {
    pub fn new(track: &InputTrack) -> Self {
        let mut samples = track.samples.clone();
        samples.sort_by_key(|s| s.t_ms);
        let mut clicks = Vec::new();
        let mut was_down = false;
        for s in &samples {
            if s.down && !was_down {
                clicks.push(Click {
                    t_ms: s.t_ms,
                    x: s.x,
                    y: s.y,
                });
            }
            was_down = s.down;
        }
        Self { samples, clicks }
    }

    pub fn is_empty(&self) -> bool {
        self.samples.is_empty()
    }

    pub fn clicks(&self) -> &[Click] {
        &self.clicks
    }

    /// Klicks ohne die im Editor gelöschten (`hidden` = Zeiten der Klicks).
    pub fn visible_clicks<'a>(&'a self, hidden: &'a [u32]) -> impl Iterator<Item = &'a Click> {
        self.clicks.iter().filter(|c| !hidden.contains(&c.t_ms))
    }

    /// Lage des Zeigers zur Quellzeit (Anteil des Bildschirms), zwischen
    /// zwei Proben weich.
    pub fn position(&self, t_ms: f64) -> Option<(f32, f32)> {
        let i = self.samples.partition_point(|s| (s.t_ms as f64) <= t_ms);
        let after = self.samples.get(i);
        let before = i.checked_sub(1).and_then(|j| self.samples.get(j));
        match (before, after) {
            (Some(a), Some(b)) if b.t_ms > a.t_ms => {
                let k = ((t_ms - a.t_ms as f64) / (b.t_ms - a.t_ms) as f64) as f32;
                Some((a.x + (b.x - a.x) * k, a.y + (b.y - a.y) * k))
            }
            (Some(a), _) => Some((a.x, a.y)),
            (None, Some(b)) => Some((b.x, b.y)),
            (None, None) => None,
        }
    }

    /// Klicks, deren Kreis zur Quellzeit zu sehen ist: (Klick, Fortschritt
    /// 0–1). Gelöschte (`hidden`) fehlen.
    pub fn active_clicks<'a>(
        &'a self,
        t_ms: f64,
        hidden: &'a [u32],
    ) -> impl Iterator<Item = (Click, f32)> + 'a {
        let from = self
            .clicks
            .partition_point(|c| (c.t_ms as f64) < t_ms - CLICK_MS);
        self.clicks[from..]
            .iter()
            .take_while(move |c| (c.t_ms as f64) <= t_ms)
            .filter(move |c| !hidden.contains(&c.t_ms))
            .map(move |c| (*c, ((t_ms - c.t_ms as f64) / CLICK_MS) as f32))
    }
}

/// Nimmt Klicks aus der Mausspur, die in Grabmark selbst passiert sind
/// (Zeichenleiste, Zeichnen, Aufnahmeleiste, Webcam-Blase): Die Taste gilt
/// dort als nicht gedrückt – kein Kreis, kein Auto-Zoom. `own_ms` sind die
/// Zeitpunkte dieser Klicks (Zeit im Video); `samples` zeitlich geordnet.
pub fn remove_own_clicks(samples: &mut [InputSample], own_ms: &[u32]) {
    if own_ms.is_empty() {
        return;
    }
    let mut was_down = false;
    let mut own = false;
    for s in samples.iter_mut() {
        if s.down && !was_down {
            own = own_ms
                .iter()
                .any(|&t| t.abs_diff(s.t_ms) <= OWN_CLICK_SLACK_MS);
        }
        was_down = s.down;
        if own && s.down {
            s.down = false;
        }
    }
}

fn smooth(t: f64) -> f64 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Zoom zur Quellzeit: Mitte und Vergrösserung (1 = kein Zoom). Am Anfang
/// und Ende eines Abschnitts weich hinein bzw. hinaus.
pub fn zoom_at(zooms: &[ZoomSection], t_ms: f64) -> (f32, f32, f32) {
    for z in zooms {
        let (s, e) = (z.start_ms as f64, z.end_ms as f64);
        if t_ms < s || t_ms >= e {
            continue;
        }
        let ease = ZOOM_EASE_MS.min((e - s) / 2.0).max(1.0);
        let k = smooth(((t_ms - s) / ease).min((e - t_ms) / ease)) as f32;
        let scale = 1.0 + (z.scale.clamp(1.0, 4.0) - 1.0) * k;
        return (z.x, z.y, scale);
    }
    (0.5, 0.5, 1.0)
}

/// Ausschnitt des Bildschirms (0 bis 1) bei Zoom um `(x, y)` mit `scale`.
pub fn zoom_crop(x: f32, y: f32, scale: f32) -> Rect {
    let size = 1.0 / scale.max(1.0);
    Rect::new(
        (x - size / 2.0).clamp(0.0, 1.0 - size),
        (y - size / 2.0).clamp(0.0, 1.0 - size),
        size,
        size,
    )
}

/// Zoom-Abschnitte aus den Klicks: nahe beieinander liegende Klicks ergeben
/// einen Abschnitt, mittig auf ihre Lage. Gelöschte Klicks (`hidden`) zählen nicht.
pub fn auto_zooms(track: &CursorTrack, duration_ms: u32, hidden: &[u32]) -> Vec<ZoomSection> {
    let mut groups: Vec<Vec<Click>> = Vec::new();
    for c in track.visible_clicks(hidden) {
        match groups.last_mut() {
            Some(g) if c.t_ms - g.last().map_or(0, |l| l.t_ms) <= AUTO_GAP_MS => g.push(*c),
            _ => groups.push(vec![*c]),
        }
    }
    let mut zooms: Vec<ZoomSection> = Vec::new();
    for g in groups {
        let n = g.len() as f32;
        let x = g.iter().map(|c| c.x).sum::<f32>() / n;
        let y = g.iter().map(|c| c.y).sum::<f32>() / n;
        let start = g[0].t_ms.saturating_sub(AUTO_BEFORE_MS);
        let end = (g[g.len() - 1].t_ms + AUTO_AFTER_MS).min(duration_ms);
        if end <= start {
            continue;
        }
        // Überlappt der vorige Abschnitt, schliesst dieser direkt an.
        let start = zooms.last().map_or(start, |p| start.max(p.end_ms));
        if end > start {
            zooms.push(ZoomSection {
                start_ms: start,
                end_ms: end,
                x: x.clamp(0.0, 1.0),
                y: y.clamp(0.0, 1.0),
                scale: AUTO_SCALE,
            });
        }
    }
    zooms
}

/// Umriss des Pfeils (Einheit: Höhe 1, Spitze bei 0,0), aussen weiss,
/// innen schwarz – wie der Zeiger von macOS.
pub const ARROW_OUTER: [(f32, f32); 7] = [
    (0.0, 0.0),
    (0.0, 1.0),
    (0.25, 0.77),
    (0.4, 1.08),
    (0.56, 1.01),
    (0.42, 0.71),
    (0.72, 0.71),
];
pub const ARROW_INNER: [(f32, f32); 7] = [
    (0.07, 0.15),
    (0.07, 0.85),
    (0.26, 0.67),
    (0.42, 0.98),
    (0.47, 0.96),
    (0.32, 0.64),
    (0.56, 0.64),
];

fn inside(poly: &[(f32, f32)], x: f32, y: f32) -> bool {
    let mut c = false;
    let mut j = poly.len() - 1;
    for i in 0..poly.len() {
        let (xi, yi) = poly[i];
        let (xj, yj) = poly[j];
        if (yi > y) != (yj > y) && x < (xj - xi) * (y - yi) / (yj - yi) + xi {
            c = !c;
        }
        j = i;
    }
    c
}

/// Zeigerbild als BGRA (gerade Deckkraft), `size` Pixel hoch, mit
/// Kantenglättung (4×4 Abtastpunkte je Pixel). Breite = Höhe.
pub fn arrow_image(size: u32) -> Vec<u8> {
    let s = size.max(4);
    let mut out = vec![0u8; (s * s * 4) as usize];
    // Etwas Rand für den Schatten links oben nicht nötig: Spitze sitzt bei 4 %.
    let scale = 0.9 / 1.08;
    let offset = 0.04;
    for py in 0..s {
        for px in 0..s {
            let (mut white, mut black) = (0u32, 0u32);
            for sy in 0..4 {
                for sx in 0..4 {
                    let x = ((px as f32 + (sx as f32 + 0.5) / 4.0) / s as f32 - offset) / scale;
                    let y = ((py as f32 + (sy as f32 + 0.5) / 4.0) / s as f32 - offset) / scale;
                    if inside(&ARROW_INNER, x, y) {
                        black += 1;
                    } else if inside(&ARROW_OUTER, x, y) {
                        white += 1;
                    }
                }
            }
            let cover = white + black;
            if cover == 0 {
                continue;
            }
            // Farbe: Mischung aus Weiss und Schwarz nach Anteil.
            let v = (255 * white / cover) as u8;
            let i = ((py * s + px) * 4) as usize;
            out[i..i + 4].copy_from_slice(&[v, v, v, (255 * cover / 16) as u8]);
        }
    }
    out
}

/// Ring um einen Klick als BGRA (gerade Deckkraft) in `color`.
pub fn ring_image(size: u32, color: [u8; 3]) -> Vec<u8> {
    let s = size.max(8);
    let mut out = vec![0u8; (s * s * 4) as usize];
    let c = s as f32 / 2.0;
    let (outer, width) = (c - 1.0, s as f32 * 0.09);
    for y in 0..s {
        for x in 0..s {
            let d = ((x as f32 + 0.5 - c).powi(2) + (y as f32 + 0.5 - c).powi(2)).sqrt();
            // Ring mit weichen Kanten, innen leicht gefüllt.
            let ring =
                (1.0 - ((d - (outer - width / 2.0)).abs() - width / 2.0).max(0.0)).clamp(0.0, 1.0);
            let fill = if d < outer - width { 0.18 } else { 0.0 };
            let a = ring.max(fill);
            if a <= 0.0 {
                continue;
            }
            let i = ((y * s + x) * 4) as usize;
            out[i..i + 4].copy_from_slice(&[color[2], color[1], color[0], (a * 255.0) as u8]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn track(points: &[(u32, f32, f32, bool)]) -> CursorTrack {
        CursorTrack::new(&InputTrack {
            samples: points
                .iter()
                .map(|&(t_ms, x, y, down)| InputSample { t_ms, x, y, down })
                .collect(),
        })
    }

    #[test]
    fn klicks_aus_tastendruck() {
        let t = track(&[
            (0, 0.1, 0.1, false),
            (100, 0.2, 0.2, true),
            (150, 0.2, 0.2, true),
            (200, 0.2, 0.2, false),
            (5000, 0.8, 0.8, true),
        ]);
        assert_eq!(t.clicks().len(), 2);
        assert_eq!(t.clicks()[0].t_ms, 100);
    }

    #[test]
    fn eigene_klicks_fallen_weg() {
        let sample = |t_ms, down| InputSample {
            t_ms,
            x: 0.5,
            y: 0.5,
            down,
        };
        let mut samples = vec![
            sample(0, false),
            sample(100, true),
            sample(150, true),
            sample(200, false),
            sample(1000, true),
            sample(1100, false),
            sample(3000, true),
        ];
        // Klick in der Leiste bei 90 ms, beim Zeichnen bei 3010 ms.
        remove_own_clicks(&mut samples, &[90, 3010]);
        let t = CursorTrack::new(&InputTrack { samples });
        assert_eq!(t.clicks().len(), 1);
        assert_eq!(t.clicks()[0].t_ms, 1000);
    }

    #[test]
    fn zeiger_wird_zwischen_proben_gemittelt() {
        let t = track(&[(0, 0.0, 0.0, false), (100, 1.0, 0.5, false)]);
        assert_eq!(t.position(50.0), Some((0.5, 0.25)));
        assert_eq!(t.position(500.0), Some((1.0, 0.5)));
        assert_eq!(CursorTrack::default().position(0.0), None);
    }

    #[test]
    fn klickkreis_nur_kurz_sichtbar() {
        let t = track(&[(1000, 0.5, 0.5, true)]);
        assert_eq!(t.active_clicks(900.0, &[]).count(), 0);
        let (c, p) = t.active_clicks(1300.0, &[]).next().unwrap();
        assert_eq!(c.t_ms, 1000);
        assert!((p - 0.5).abs() < 0.01);
        assert_eq!(t.active_clicks(1700.0, &[]).count(), 0);
        // Im Editor gelöscht: kein Kreis.
        assert_eq!(t.active_clicks(1300.0, &[1000]).count(), 0);
    }

    #[test]
    fn zoom_weich_hinein_und_hinaus() {
        let zooms = [ZoomSection {
            start_ms: 1000,
            end_ms: 4000,
            x: 0.3,
            y: 0.4,
            scale: 2.0,
        }];
        assert_eq!(zoom_at(&zooms, 500.0).2, 1.0);
        assert_eq!(zoom_at(&zooms, 2500.0), (0.3, 0.4, 2.0));
        let mid = zoom_at(&zooms, 1250.0).2;
        assert!(mid > 1.0 && mid < 2.0);
        assert_eq!(zoom_at(&zooms, 4000.0).2, 1.0);
    }

    #[test]
    fn zoom_ausschnitt_bleibt_im_bild() {
        let c = zoom_crop(0.95, 0.5, 2.0);
        assert_eq!(c, Rect::new(0.5, 0.25, 0.5, 0.5));
    }

    #[test]
    fn auto_zoom_fasst_nahe_klicks_zusammen() {
        let t = track(&[
            (1000, 0.2, 0.2, true),
            (1100, 0.2, 0.2, false),
            (2000, 0.4, 0.2, true),
            (2100, 0.4, 0.2, false),
            (9000, 0.8, 0.9, true),
        ]);
        let z = auto_zooms(&t, 10_000, &[]);
        assert_eq!(z.len(), 2);
        assert_eq!((z[0].start_ms, z[0].end_ms), (300, 3800));
        assert!((z[0].x - 0.3).abs() < 1e-6);
        assert_eq!(z[1].end_ms, 10_000);
        // Gelöschte Klicks ergeben keinen Zoom.
        let z = auto_zooms(&t, 10_000, &[9000]);
        assert_eq!(z.len(), 1);
    }

    #[test]
    fn pfeil_hat_spitze_oben_links() {
        let img = arrow_image(32);
        // oben links (nahe Spitze) sichtbar, unten rechts leer
        let alpha = |x: u32, y: u32| img[((y * 32 + x) * 4 + 3) as usize];
        assert!(alpha(2, 4) > 0);
        assert_eq!(alpha(30, 30), 0);
    }
}
