//! Wo liegt in welchem Bild was? Editor-Vorschau und Export rechnen beide
//! hiermit (ADR 0011), damit sie nicht auseinanderlaufen.

use nabshot_core::{
    CameraFormat, Color, Rect, Size, VideoDoc, WebcamBackground, WebcamFraming, WebcamLayout,
    WebcamMode, WebcamPosition, WebcamSection, WebcamShape,
};

use crate::cursor::{CursorTrack, zoom_at, zoom_crop};
use crate::head::HeadPath;

/// So lange dauert der Übergang zwischen zwei Abschnitten.
pub const TRANSITION_MS: f64 = 300.0;
/// Abstand der Webcam zum Bildrand als Anteil der kürzeren Bildseite.
pub const MARGIN: f32 = 0.03;
/// Kleinste und grösste Breite der Webcam als Anteil der Bildbreite.
pub const MIN_SIZE: f32 = 0.05;
pub const MAX_SIZE: f32 = 0.6;
/// Grösse des Bildschirms, wenn die Webcam das Bild füllt.
const PIP_SIZE: f32 = 0.28;
/// Deckkraft des Schattens.
const SHADOW: f32 = 0.45;

/// Was eine Ebene des fertigen Bilds zeigt.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Source {
    Screen,
    Webcam,
    /// Eigener Mauszeiger (Etappe 5).
    Cursor,
    /// Kreis um einen Klick (Etappe 5).
    Click,
    /// Pfeile, Text usw. aus dem Dokument (Etappe 6).
    Overlay,
}

impl Source {
    pub const ALL: [Source; 5] = [
        Source::Screen,
        Source::Webcam,
        Source::Cursor,
        Source::Click,
        Source::Overlay,
    ];

    pub fn index(self) -> usize {
        self as usize
    }

    pub fn name(self) -> &'static str {
        match self {
            Source::Screen => "screen",
            Source::Webcam => "webcam",
            Source::Cursor => "cursor",
            Source::Click => "click",
            Source::Overlay => "overlay",
        }
    }
}

/// Eine Spur im fertigen Bild.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Placement {
    /// Ziel in Pixeln des Ausgabebilds.
    pub rect: Rect,
    /// Eckenradius in Pixeln.
    pub radius: f32,
    /// Ausschnitt der Quelle, 0 bis 1.
    pub crop: Rect,
    pub mirror: bool,
    /// Rahmenbreite in Pixeln und Farbe; liegt innen am Rand.
    pub border: Option<(f32, Color)>,
    /// Deckkraft des Schattens (0 = keiner).
    pub shadow: f32,
    pub opacity: f32,
    /// Hintergrund hinter der Person (nur Webcam; braucht eine Maske).
    pub background: WebcamBackground,
}

impl Placement {
    /// Bild ohne Form, Rahmen und Schatten, voll deckend.
    pub fn plain(rect: Rect) -> Self {
        Self {
            rect,
            radius: 0.0,
            crop: FULL,
            mirror: false,
            border: None,
            shadow: 0.0,
            opacity: 1.0,
            background: WebcamBackground::Original,
        }
    }
}

/// Was in einem Bild zu sehen ist, von unten nach oben.
#[derive(Debug, Clone, PartialEq)]
pub struct Scene {
    pub layers: Vec<(Source, Placement)>,
}

const FULL: Rect = Rect::new(0.0, 0.0, 1.0, 1.0);

/// Ausschnitt der Quelle, der das Ziel ohne Verzerren füllt (mittig).
pub fn aspect_fill_crop(source_aspect: f32, target_aspect: f32) -> Rect {
    if source_aspect <= 0.0 || target_aspect <= 0.0 {
        return FULL;
    }
    if source_aspect > target_aspect {
        let w = target_aspect / source_aspect;
        Rect::new((1.0 - w) / 2.0, 0.0, w, 1.0)
    } else {
        let h = source_aspect / target_aspect;
        Rect::new(0.0, (1.0 - h) / 2.0, 1.0, h)
    }
}

/// Rechteck der Grösse `w`×`h` in der Ecke bzw. an der freien Stelle.
fn place(position: WebcamPosition, w: f32, h: f32, frame: Size) -> Rect {
    let m = MARGIN * frame.width.min(frame.height);
    let (x, y) = match position {
        WebcamPosition::TopLeft => (m, m),
        WebcamPosition::TopRight => (frame.width - m - w, m),
        WebcamPosition::BottomLeft => (m, frame.height - m - h),
        WebcamPosition::BottomRight => (frame.width - m - w, frame.height - m - h),
        WebcamPosition::Free { x, y } => (
            (x * frame.width - w / 2.0).clamp(0.0, (frame.width - w).max(0.0)),
            (y * frame.height - h / 2.0).clamp(0.0, (frame.height - h).max(0.0)),
        ),
    };
    Rect::new(x, y, w, h)
}

/// Grösse der Webcam im Bild.
fn webcam_size(layout: &WebcamLayout, frame: Size) -> (f32, f32) {
    let aspect = layout.shape.aspect();
    let mut w = layout.size.clamp(MIN_SIZE, MAX_SIZE) * frame.width;
    let mut h = w / aspect;
    if h > 0.8 * frame.height {
        h = 0.8 * frame.height;
        w = h * aspect;
    }
    (w, h)
}

/// Webcam klein über dem Bildschirm.
pub fn webcam_overlay(layout: &WebcamLayout, frame: Size, camera: Size) -> Placement {
    let (w, h) = webcam_size(layout, frame);
    let rect = place(layout.position, w, h, frame);
    let short = w.min(h);
    let radius = match layout.shape {
        WebcamShape::Circle => short / 2.0,
        _ => layout.corner_radius.clamp(0.0, 0.5) * short,
    };
    // Ohne Hintergrund schwebt nur die Person – Rahmen und Schatten der
    // Form wären dann um nichts herum.
    let removed = layout.background == WebcamBackground::Remove;
    Placement {
        rect,
        radius,
        crop: aspect_fill_crop(camera.width / camera.height.max(1.0), w / h),
        mirror: layout.mirror,
        border: layout
            .border
            .filter(|b| b.width > 0.0 && !removed)
            .map(|b| (b.width.clamp(0.0, 0.2) * short, b.color)),
        shadow: if layout.shadow && !removed {
            SHADOW
        } else {
            0.0
        },
        opacity: 1.0,
        background: layout.background,
    }
}

/// Spur füllt das ganze Bild.
fn fullscreen(frame: Size, source: Size, mirror: bool) -> Placement {
    Placement {
        rect: Rect::new(0.0, 0.0, frame.width, frame.height),
        radius: 0.0,
        crop: aspect_fill_crop(
            source.width / source.height.max(1.0),
            frame.width / frame.height.max(1.0),
        ),
        mirror,
        border: None,
        shadow: 0.0,
        opacity: 1.0,
        background: WebcamBackground::Original,
    }
}

/// Nächste Ecke zu einer freien Lage.
fn nearest_corner(position: WebcamPosition) -> WebcamPosition {
    match position {
        WebcamPosition::Free { x, y } => match (x < 0.5, y < 0.5) {
            (true, true) => WebcamPosition::TopLeft,
            (false, true) => WebcamPosition::TopRight,
            (true, false) => WebcamPosition::BottomLeft,
            (false, false) => WebcamPosition::BottomRight,
        },
        corner => corner,
    }
}

/// Bildschirm klein in der Ecke der Webcam, wenn die Webcam das Bild füllt.
fn screen_pip(layout: &WebcamLayout, frame: Size, screen: Size) -> Placement {
    let w = layout.size.max(PIP_SIZE) * frame.width;
    let h = w * screen.height / screen.width.max(1.0);
    let rect = place(nearest_corner(layout.position), w, h, frame);
    Placement {
        rect,
        radius: 0.03 * w.min(h),
        crop: FULL,
        mirror: false,
        border: None,
        shadow: SHADOW,
        opacity: 1.0,
        background: WebcamBackground::Original,
    }
}

/// Lage beider Spuren in einem Abschnitt; die Webcam fehlt, wenn sie
/// ausgeblendet ist. `bool` = Webcam liegt oben.
fn arrangement(
    mode: WebcamMode,
    layout: &WebcamLayout,
    frame: Size,
    screen: Size,
    camera: Size,
) -> (Placement, Option<Placement>, bool) {
    match mode {
        WebcamMode::Overlay => (
            fullscreen(frame, screen, false),
            Some(webcam_overlay(layout, frame, camera)),
            true,
        ),
        WebcamMode::Large => (
            screen_pip(layout, frame, screen),
            Some(Placement {
                background: layout.background,
                ..fullscreen(frame, camera, layout.mirror)
            }),
            false,
        ),
        WebcamMode::Hidden => (fullscreen(frame, screen, false), None, true),
    }
}

fn lerp(a: f32, b: f32, t: f32) -> f32 {
    a + (b - a) * t
}

fn lerp_rect(a: Rect, b: Rect, t: f32) -> Rect {
    Rect::new(
        lerp(a.x, b.x, t),
        lerp(a.y, b.y, t),
        lerp(a.width, b.width, t),
        lerp(a.height, b.height, t),
    )
}

/// Weicher Übergang zwischen zwei Lagen; `t` von 0 bis 1.
pub fn blend(a: &Placement, b: &Placement, t: f32) -> Placement {
    let border = match (a.border, b.border) {
        (Some((wa, c)), Some((wb, _))) => Some((lerp(wa, wb, t), c)),
        (Some((w, c)), None) => Some((lerp(w, 0.0, t), c)),
        (None, Some((w, c))) => Some((lerp(0.0, w, t), c)),
        (None, None) => None,
    };
    Placement {
        rect: lerp_rect(a.rect, b.rect, t),
        radius: lerp(a.radius, b.radius, t),
        crop: lerp_rect(a.crop, b.crop, t),
        mirror: if t < 0.5 { a.mirror } else { b.mirror },
        border,
        shadow: lerp(a.shadow, b.shadow, t),
        opacity: lerp(a.opacity, b.opacity, t),
        background: if t < 0.5 { a.background } else { b.background },
    }
}

fn smoothstep(t: f32) -> f32 {
    let t = t.clamp(0.0, 1.0);
    t * t * (3.0 - 2.0 * t)
}

/// Abschnitt zur Zeit `t_ms`: aktueller Modus, vorheriger Modus und wie weit
/// der Übergang ist (1 = fertig).
pub fn mode_at(sections: &[WebcamSection], t_ms: f64) -> (WebcamMode, WebcamMode, f32) {
    let mut current = WebcamMode::Hidden;
    let mut previous = WebcamMode::Hidden;
    let mut since = f64::INFINITY;
    for s in sections {
        if s.start_ms as f64 > t_ms {
            break;
        }
        previous = current;
        current = s.mode;
        since = t_ms - s.start_ms as f64;
    }
    // Der erste Abschnitt ab 0 beginnt ohne Übergang.
    let first = sections.first().is_some_and(|s| s.start_ms == 0) && since == t_ms;
    let progress = if first || previous == current {
        1.0
    } else {
        smoothstep((since / TRANSITION_MS) as f32)
    };
    (current, previous, progress)
}

/// Was im Bild zur Zeit `t_ms` wo liegt. Ohne Webcam (`camera = None` oder
/// keine Webcam-Spur im Dokument) nur der Bildschirm.
pub fn scene_at(
    doc: &VideoDoc,
    t_ms: f64,
    frame: Size,
    screen: Size,
    camera: Option<Size>,
) -> Scene {
    scene_with_head(doc, t_ms, frame, screen, camera, None)
}

/// Wie [`scene_at`], beim Ausschnitt „Kopf“ entlang `head` (Etappe 3).
/// Ohne erkannten Kopf bleibt es beim ganzen Kamerabild.
pub fn scene_with_head(
    doc: &VideoDoc,
    t_ms: f64,
    frame: Size,
    screen: Size,
    camera: Option<Size>,
    head: Option<&HeadPath>,
) -> Scene {
    scene_full(
        doc,
        t_ms,
        frame,
        screen,
        camera,
        &SceneExtras {
            head,
            ..SceneExtras::default()
        },
    )
}

/// Was die Szene über das Dokument hinaus braucht.
#[derive(Debug, Clone, Copy, Default)]
pub struct SceneExtras<'a> {
    /// Kopfausschnitt der Webcam (Etappe 3).
    pub head: Option<&'a HeadPath>,
    /// Mausspur für den eigenen Zeiger und Klicks (Etappe 5).
    pub cursor: Option<&'a CursorTrack>,
    /// Höhe des Zeigers in Pixeln der Bildschirm-Spur bei Grösse 1
    /// (Bildschirmpunkte × Skalierung, z. B. 24 × 2 bei Retina).
    pub cursor_px: f32,
}

/// Ganze Szene zur Quellzeit `t_ms`: Spuren, Zoom, Zeiger und Klicks.
pub fn scene_full(
    doc: &VideoDoc,
    t_ms: f64,
    frame: Size,
    screen: Size,
    camera: Option<Size>,
    extras: &SceneExtras<'_>,
) -> Scene {
    let mut scene = base_scene(doc, t_ms, frame, screen, camera, extras.head);
    let Some(i) = scene.layers.iter().position(|(s, _)| *s == Source::Screen) else {
        return scene;
    };
    // Zoom: Ausschnitt des Bildschirms verkleinern.
    let (zx, zy, zs) = zoom_at(&doc.zooms, t_ms);
    if zs > 1.0 {
        let p = &mut scene.layers[i].1;
        let z = zoom_crop(zx, zy, zs);
        p.crop = Rect::new(
            p.crop.x + z.x * p.crop.width,
            p.crop.y + z.y * p.crop.height,
            z.width * p.crop.width,
            z.height * p.crop.height,
        );
    }
    let style = doc.cursor;
    let Some(track) = extras.cursor.filter(|t| style.visible && !t.is_empty()) else {
        return scene;
    };
    let sp = scene.layers[i].1;
    // Bildschirm-Anteil → Pixel im fertigen Bild (nur innerhalb des Ausschnitts).
    let to_output = |x: f32, y: f32| -> Option<(f32, f32)> {
        let u = (x - sp.crop.x) / sp.crop.width.max(1e-6);
        let v = (y - sp.crop.y) / sp.crop.height.max(1e-6);
        ((0.0..=1.0).contains(&u) && (0.0..=1.0).contains(&v)).then_some((
            sp.rect.x + u * sp.rect.width,
            sp.rect.y + v * sp.rect.height,
        ))
    };
    // So viele Ausgabepixel hat ein Pixel der Bildschirm-Spur.
    let zoom = sp.rect.width / (sp.crop.width * screen.width).max(1.0);
    let size = (extras.cursor_px.max(1.0) * style.size.clamp(0.5, 4.0) * zoom).max(4.0);
    let plain = |rect: Rect, opacity: f32| Placement {
        rect,
        radius: 0.0,
        crop: FULL,
        mirror: false,
        border: None,
        shadow: 0.0,
        opacity: opacity * sp.opacity,
        background: WebcamBackground::Original,
    };
    let mut extra = Vec::new();
    if style.highlight_clicks {
        for (c, p) in track.active_clicks(t_ms, &doc.hidden_clicks) {
            if let Some((x, y)) = to_output(c.x, c.y) {
                let d = size * 1.4 * (0.5 + 0.9 * p);
                extra.push((
                    Source::Click,
                    plain(Rect::new(x - d / 2.0, y - d / 2.0, d, d), 1.0 - p),
                ));
            }
        }
    }
    if let Some((x, y)) = track.position(t_ms).and_then(|(x, y)| to_output(x, y)) {
        // Spitze des Pfeils sitzt bei 4 % des Bilds (siehe `arrow_image`).
        let o = 0.04 * size;
        extra.push((
            Source::Cursor,
            plain(Rect::new(x - o, y - o, size, size), 1.0),
        ));
    }
    let at = i + 1;
    scene.layers.splice(at..at, extra);
    scene
}

fn base_scene(
    doc: &VideoDoc,
    t_ms: f64,
    frame: Size,
    screen: Size,
    camera: Option<Size>,
    head: Option<&HeadPath>,
) -> Scene {
    if doc.camera_only.is_some() {
        return camera_scene(doc, t_ms, frame, camera, head);
    }
    let full = fullscreen(frame, screen, false);
    let (Some(track), Some(camera)) = (&doc.webcam, camera) else {
        return Scene {
            layers: vec![(Source::Screen, full)],
        };
    };
    let (mode, previous, t) = mode_at(&track.sections, t_ms);
    let (screen_now, cam_now, cam_top) = arrangement(mode, &track.layout, frame, screen, camera);
    // Im Übergang steht die Spur dahinter schon an ihrem Ziel; nur die
    // vordere gleitet (bzw. blendet ein oder aus). So scheint nie der
    // schwarze Hintergrund durch.
    let (screen_place, cam_place) = if t >= 1.0 {
        (screen_now, cam_now)
    } else {
        let (screen_before, cam_before, _) =
            arrangement(previous, &track.layout, frame, screen, camera);
        if cam_top {
            let faded = |p: Placement| Placement { opacity: 0.0, ..p };
            let cam = match (cam_before, cam_now) {
                (Some(a), Some(b)) => Some(blend(&a, &b, t)),
                (Some(a), None) => Some(blend(&a, &faded(a), t)),
                (None, Some(b)) => Some(blend(&faded(b), &b, t)),
                (None, None) => None,
            };
            (screen_now, cam)
        } else {
            (blend(&screen_before, &screen_now, t), cam_now)
        }
    };
    let head = head.filter(|_| track.layout.framing == WebcamFraming::Head);
    let cam_place = cam_place.map(|mut c| {
        if let Some(head) = head {
            c.crop = head.crop(t_ms, camera, c.rect.width / c.rect.height.max(1.0));
        }
        c
    });
    let mut layers = vec![(Source::Screen, screen_place)];
    if let Some(cam) = cam_place.filter(|c| c.opacity > 0.0) {
        if cam_top {
            layers.push((Source::Webcam, cam));
        } else {
            layers.insert(0, (Source::Webcam, cam));
        }
    }
    Scene { layers }
}

/// Nur Webcam: Sie füllt das ganze Bild (ohne Form, Rahmen und Schatten).
fn camera_scene(
    doc: &VideoDoc,
    t_ms: f64,
    frame: Size,
    camera: Option<Size>,
    head: Option<&HeadPath>,
) -> Scene {
    let (Some(track), Some(camera)) = (&doc.webcam, camera) else {
        return Scene { layers: Vec::new() };
    };
    let layout = &track.layout;
    let mut place = Placement {
        background: layout.background,
        ..fullscreen(frame, camera, layout.mirror)
    };
    if let Some(head) = head.filter(|_| layout.framing == WebcamFraming::Head) {
        place.crop = head.crop(t_ms, camera, frame.width / frame.height.max(1.0));
    }
    Scene {
        layers: vec![(Source::Webcam, place)],
    }
}

/// Grösse des fertigen Videos bei „nur Webcam“: die kürzere Seite wie die
/// Kamera, höchstens 1080 Pixel (Hochformat 1080×1920 wie bei Reels).
pub fn camera_frame_size(format: CameraFormat, camera: (u32, u32)) -> (u32, u32) {
    let short = camera.0.min(camera.1).clamp(2, 1080) as f32;
    let aspect = format.aspect();
    let (w, h) = if aspect >= 1.0 {
        (short * aspect, short)
    } else {
        (short, short / aspect)
    };
    (
        (w.round() as u32 & !1).max(2),
        (h.round() as u32 & !1).max(2),
    )
}

/// Lage der Live-Blase → Startlayout: `area` ist der aufgenommene Bereich,
/// `bubble` die Blase, beide in denselben Koordinaten. Nah an einer Ecke
/// (oder ausserhalb des Bereichs) rastet sie in der Ecke ein, sonst bleibt
/// sie frei. Gibt Lage und Grösse (Anteil der Breite) zurück.
pub fn layout_from_bubble(area: Rect, bubble: Rect) -> (WebcamPosition, f32) {
    let size = (bubble.width / area.width.max(1.0)).clamp(MIN_SIZE, MAX_SIZE);
    let cx = bubble.x + bubble.width / 2.0;
    let cy = bubble.y + bubble.height / 2.0;
    let free = WebcamPosition::Free {
        x: ((cx - area.x) / area.width.max(1.0)).clamp(0.0, 1.0),
        y: ((cy - area.y) / area.height.max(1.0)).clamp(0.0, 1.0),
    };
    let overlaps = bubble.x < area.right()
        && bubble.right() > area.x
        && bubble.y < area.bottom()
        && bubble.bottom() > area.y;
    let corner = nearest_corner(free);
    if !overlaps {
        return (corner, size);
    }
    // Wo die Blase in der Ecke läge (gleiche Grösse).
    let frame = Size::new(area.width, area.height);
    let snapped = place(corner, bubble.width, bubble.height, frame);
    let dx = (bubble.x - area.x) - snapped.x;
    let dy = (bubble.y - area.y) - snapped.y;
    let snap = 0.08 * area.width.min(area.height);
    if dx.hypot(dy) <= snap {
        (corner, size)
    } else {
        (free, size)
    }
}

#[cfg(test)]
mod tests {
    use nabshot_core::{WebcamBorder, WebcamTrack};

    use super::*;

    const FRAME: Size = Size::new(1920.0, 1080.0);
    const CAM: Size = Size::new(1280.0, 720.0);

    fn doc(sections: &[(u32, WebcamMode)], layout: WebcamLayout) -> VideoDoc {
        VideoDoc {
            duration_ms: 10_000,
            fps: 30,
            webcam: Some(WebcamTrack {
                layout,
                sections: sections
                    .iter()
                    .map(|&(start_ms, mode)| WebcamSection { start_ms, mode })
                    .collect(),
            }),
            ..VideoDoc::default()
        }
    }

    fn close(a: f32, b: f32) -> bool {
        (a - b).abs() < 0.01
    }

    #[test]
    fn nur_webcam_fuellt_das_bild() {
        let mut d = doc(&[(0, WebcamMode::Large)], WebcamLayout::default());
        d.camera_only = Some(CameraFormat::Portrait);
        let frame = Size::new(1080.0, 1920.0);
        let scene = scene_at(&d, 1000.0, frame, Size::new(2.0, 2.0), Some(CAM));
        assert_eq!(scene.layers.len(), 1);
        let (source, p) = scene.layers[0];
        assert_eq!(source, Source::Webcam);
        assert_eq!(p.rect, Rect::new(0.0, 0.0, 1080.0, 1920.0));
        // 16:9-Kamera ins Hochformat: nur die Mitte
        assert!(close(p.crop.width, 0.3164) && close(p.crop.x, 0.3418));
        assert!(p.mirror && p.border.is_none() && p.shadow == 0.0 && p.radius == 0.0);
        // Ohne Kamerabild bleibt das Bild leer
        assert!(scene_at(&d, 0.0, frame, frame, None).layers.is_empty());
    }

    #[test]
    fn groesse_nur_webcam() {
        let hd = (1920, 1080);
        assert_eq!(camera_frame_size(CameraFormat::Landscape, hd), (1920, 1080));
        assert_eq!(camera_frame_size(CameraFormat::Portrait, hd), (1080, 1920));
        assert_eq!(camera_frame_size(CameraFormat::Square, hd), (1080, 1080));
        assert_eq!(
            camera_frame_size(CameraFormat::Landscape, (1280, 720)),
            (1280, 720)
        );
        // 4K-Kamera: höchstens 1080 auf der kürzeren Seite
        assert_eq!(
            camera_frame_size(CameraFormat::Landscape, (3840, 2160)),
            (1920, 1080)
        );
    }

    #[test]
    fn kreis_unten_rechts_mit_rand() {
        let p = webcam_overlay(&WebcamLayout::default(), FRAME, CAM);
        // 20 % von 1920 = 384, Rand 3 % von 1080 = 32.4
        assert!(close(p.rect.width, 384.0) && close(p.rect.height, 384.0));
        assert!(close(p.rect.x, 1920.0 - 32.4 - 384.0));
        assert!(close(p.rect.y, 1080.0 - 32.4 - 384.0));
        assert!(close(p.radius, 192.0));
        // 16:9-Kamera in ein Quadrat: links und rechts abgeschnitten
        assert!(close(p.crop.width, 0.5625) && close(p.crop.x, 0.21875));
        assert!(p.mirror && p.shadow > 0.0);
        assert!(close(p.border.unwrap().0, 0.025 * 384.0));
    }

    #[test]
    fn hochformat_oben_links_ohne_rundung() {
        let layout = WebcamLayout {
            position: WebcamPosition::TopLeft,
            shape: WebcamShape::Portrait,
            corner_radius: 0.0,
            size: 0.15,
            border: None,
            shadow: false,
            ..WebcamLayout::default()
        };
        let p = webcam_overlay(&layout, FRAME, CAM);
        assert!(close(p.rect.x, 32.4) && close(p.rect.y, 32.4));
        assert!(close(p.rect.width, 288.0) && close(p.rect.height, 512.0));
        assert_eq!(p.radius, 0.0);
        assert_eq!(p.border, None);
        assert_eq!(p.shadow, 0.0);
    }

    #[test]
    fn zu_hohe_webcam_wird_begrenzt() {
        let layout = WebcamLayout {
            shape: WebcamShape::Portrait,
            size: 0.6,
            ..WebcamLayout::default()
        };
        let p = webcam_overlay(&layout, FRAME, CAM);
        assert!(close(p.rect.height, 864.0));
        assert!(close(p.rect.width, 864.0 * 9.0 / 16.0));
    }

    #[test]
    fn freie_lage_bleibt_im_bild() {
        let layout = WebcamLayout {
            position: WebcamPosition::Free { x: 1.0, y: 0.5 },
            ..WebcamLayout::default()
        };
        let p = webcam_overlay(&layout, FRAME, CAM);
        assert!(close(p.rect.right(), 1920.0));
        assert!(close(p.rect.y + p.rect.height / 2.0, 540.0));
    }

    #[test]
    fn abschnitte_mit_uebergang() {
        let sections = [
            WebcamSection {
                start_ms: 0,
                mode: WebcamMode::Overlay,
            },
            WebcamSection {
                start_ms: 1000,
                mode: WebcamMode::Large,
            },
        ];
        assert_eq!(
            mode_at(&sections, 0.0),
            (WebcamMode::Overlay, WebcamMode::Hidden, 1.0)
        );
        assert_eq!(mode_at(&sections, 500.0).0, WebcamMode::Overlay);
        let (now, before, t) = mode_at(&sections, 1150.0);
        assert_eq!((now, before), (WebcamMode::Large, WebcamMode::Overlay));
        assert!(close(t, 0.5));
        assert_eq!(mode_at(&sections, 2000.0).2, 1.0);
        // Vor dem ersten Abschnitt: ausgeblendet
        let late = [WebcamSection {
            start_ms: 500,
            mode: WebcamMode::Overlay,
        }];
        assert_eq!(mode_at(&late, 100.0).0, WebcamMode::Hidden);
    }

    #[test]
    fn webcam_gross_legt_bildschirm_in_die_ecke() {
        let d = doc(&[(0, WebcamMode::Large)], WebcamLayout::default());
        let scene = scene_at(&d, 100.0, FRAME, FRAME, Some(CAM));
        assert_eq!(scene.layers.len(), 2);
        let (bottom, cam) = scene.layers[0];
        let (top, pip) = scene.layers[1];
        assert_eq!((bottom, top), (Source::Webcam, Source::Screen));
        assert_eq!(cam.rect, Rect::new(0.0, 0.0, 1920.0, 1080.0));
        assert!(close(pip.rect.width, 0.28 * 1920.0));
        assert!(pip.rect.x > 960.0 && pip.rect.y > 540.0);
    }

    #[test]
    fn ausblenden_wird_weich() {
        let d = doc(
            &[(0, WebcamMode::Overlay), (1000, WebcamMode::Hidden)],
            WebcamLayout::default(),
        );
        let mid = scene_at(&d, 1150.0, FRAME, FRAME, Some(CAM));
        assert_eq!(mid.layers.len(), 2);
        assert!(close(mid.layers[1].1.opacity, 0.5));
        let after = scene_at(&d, 1400.0, FRAME, FRAME, Some(CAM));
        assert_eq!(after.layers.len(), 1);
    }

    #[test]
    fn ohne_webcam_nur_bildschirm() {
        let d = doc(&[(0, WebcamMode::Overlay)], WebcamLayout::default());
        assert_eq!(scene_at(&d, 0.0, FRAME, FRAME, None).layers.len(), 1);
    }

    #[test]
    fn blase_in_der_ecke_rastet_ein() {
        let area = Rect::new(100.0, 50.0, 1000.0, 600.0);
        // fast unten rechts: Rand wäre 18 px
        let bubble = Rect::new(
            100.0 + 1000.0 - 200.0 - 30.0,
            50.0 + 600.0 - 200.0 - 10.0,
            200.0,
            200.0,
        );
        let (pos, size) = layout_from_bubble(area, bubble);
        assert_eq!(pos, WebcamPosition::BottomRight);
        assert!(close(size, 0.2));
    }

    #[test]
    fn blase_in_der_mitte_bleibt_frei() {
        let area = Rect::new(0.0, 0.0, 1000.0, 500.0);
        let (pos, _) = layout_from_bubble(area, Rect::new(400.0, 150.0, 200.0, 200.0));
        assert_eq!(pos, WebcamPosition::Free { x: 0.5, y: 0.5 });
    }

    #[test]
    fn blase_ausserhalb_kommt_in_die_naechste_ecke() {
        let area = Rect::new(0.0, 0.0, 800.0, 600.0);
        let (pos, size) = layout_from_bubble(area, Rect::new(-300.0, -50.0, 200.0, 200.0));
        assert_eq!(pos, WebcamPosition::TopLeft);
        assert!(close(size, 0.25));
    }

    #[test]
    fn kopf_ausschnitt_ersetzt_mittigen_zuschnitt() {
        use nabshot_core::{FaceSample, FaceTrack};
        let layout = WebcamLayout {
            framing: WebcamFraming::Head,
            ..WebcamLayout::default()
        };
        let d = doc(&[(0, WebcamMode::Overlay)], layout);
        let faces = FaceTrack {
            samples: vec![FaceSample {
                t_ms: 0,
                face: Some(Rect::new(0.1, 0.2, 0.1, 0.2)),
            }],
        };
        let head = HeadPath::new(&faces, false).unwrap();
        let scene = scene_with_head(&d, 0.0, FRAME, FRAME, Some(CAM), Some(&head));
        let cam = scene.layers[1].1;
        // Kopf links: Ausschnitt beginnt am linken Rand, nicht in der Mitte
        assert!(close(cam.crop.x, 0.0));
        assert!(close(cam.crop.height, 0.52));
        // Ohne „Kopf“ in den Einstellungen bleibt der mittige Zuschnitt.
        let full = doc(&[(0, WebcamMode::Overlay)], WebcamLayout::default());
        let scene = scene_with_head(&full, 0.0, FRAME, FRAME, Some(CAM), Some(&head));
        assert!(close(scene.layers[1].1.crop.x, 0.21875));
    }

    #[test]
    fn ohne_hintergrund_kein_rahmen_und_schatten() {
        let layout = WebcamLayout {
            background: WebcamBackground::Remove,
            ..WebcamLayout::default()
        };
        let p = webcam_overlay(&layout, FRAME, CAM);
        assert_eq!(p.border, None);
        assert_eq!(p.shadow, 0.0);
        assert_eq!(p.background, WebcamBackground::Remove);
    }

    #[test]
    fn zeiger_und_klick_folgen_dem_zoom() {
        use nabshot_core::{InputSample, InputTrack, ZoomSection};
        let mut d = doc(&[], WebcamLayout::default());
        d.webcam = None;
        d.zooms = vec![ZoomSection {
            start_ms: 0,
            end_ms: 10_000,
            x: 0.25,
            y: 0.25,
            scale: 2.0,
        }];
        let track = CursorTrack::new(&InputTrack {
            samples: vec![InputSample {
                t_ms: 4000,
                x: 0.25,
                y: 0.25,
                down: true,
            }],
        });
        let extras = SceneExtras {
            cursor: Some(&track),
            cursor_px: 48.0,
            ..SceneExtras::default()
        };
        let scene = scene_full(&d, 4200.0, FRAME, FRAME, None, &extras);
        let kinds: Vec<Source> = scene.layers.iter().map(|(s, _)| *s).collect();
        assert_eq!(kinds, vec![Source::Screen, Source::Click, Source::Cursor]);
        // Zoom 2× auf die linke obere Ecke: Ausschnitt 0–0.5
        assert_eq!(scene.layers[0].1.crop, Rect::new(0.0, 0.0, 0.5, 0.5));
        // Zeiger bei 0.25 → Bildmitte, doppelt so gross (48 · 1.5 · 2)
        let cursor = scene.layers[2].1;
        assert!(close(cursor.rect.width, 144.0));
        assert!(close(cursor.rect.x + 0.04 * 144.0, 960.0));
    }

    #[test]
    fn rahmen_blendet_beim_uebergang_ein() {
        let a = Placement {
            rect: Rect::new(0.0, 0.0, 10.0, 10.0),
            radius: 0.0,
            crop: FULL,
            mirror: false,
            border: None,
            shadow: 0.0,
            opacity: 1.0,
            background: WebcamBackground::Original,
        };
        let b = Placement {
            border: Some((4.0, WebcamBorder::default().color)),
            ..a
        };
        assert!(close(blend(&a, &b, 0.5).border.unwrap().0, 2.0));
    }
}
