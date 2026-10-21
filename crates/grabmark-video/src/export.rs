//! Export-Ablauf: Spuren lesen (je ein Thread), für jedes Ausgabebild die
//! Szene rechnen, auf der Grafikkarte zusammensetzen, an den Encoder geben.

use std::sync::mpsc::{Receiver, sync_channel};
use std::time::{Duration, Instant};

use grabmark_core::{Size, VideoDoc};

use grabmark_core::Rect;

use crate::cursor::{CursorTrack, arrow_image, ring_image};
use crate::gpu::Compositor;
use crate::head::HeadPath;
use crate::layout::{Placement, SceneExtras, Source, scene_full};
use crate::timeline::{output_duration, source_time};
use crate::{
    FrameRef, FrameSink, FrameSource, Mask, OverlaySource, Segmenter, VideoError, VideoFrame,
    output_size,
};

/// Grösse der Texturen für Zeiger und Klick-Kreis (werden skaliert).
const CURSOR_TEXTURE: u32 = 128;

/// Was der Export über die Spuren hinaus braucht (alles freiwillig).
#[derive(Default)]
pub struct ExportExtras {
    /// Weg des Kopfausschnitts (aus `<id>_faces.json`, Etappe 3).
    pub head: Option<HeadPath>,
    /// Trennt die Person vom Hintergrund (Etappe 3).
    pub segmenter: Option<Box<dyn Segmenter>>,
    /// Mausspur für eigenen Zeiger und Klicks (Etappe 5).
    pub cursor: Option<CursorTrack>,
    /// Zeigerhöhe in Pixeln der Bildschirm-Spur bei Grösse 1.
    pub cursor_px: f32,
    /// Ebenen über dem Video (Etappe 6).
    pub overlay: Option<Box<dyn OverlaySource>>,
    /// Grösse des fertigen Videos; `None` = wie der Bildschirm
    /// ([`output_size`]). Der Sink muss mit dieser Grösse angelegt sein.
    pub size: Option<(u32, u32)>,
}

/// Neue Maske der Person höchstens so oft (Vision braucht ~15 ms je Maske;
/// die Person bewegt sich zwischen zwei Bildern kaum, dazwischen gilt die
/// letzte).
const MASK_INTERVAL: Duration = Duration::from_millis(66);

/// So viele dekodierte Bilder liegen je Spur bereit.
const QUEUE: usize = 3;

/// Ergebnis eines Exports (für Messungen).
#[derive(Debug, Clone)]
pub struct ExportStats {
    pub frames: u64,
    pub elapsed: Duration,
    pub adapter: String,
}

/// Ein gelesenes Bild, bei der Webcam mit Maske der Person.
type Decoded = (VideoFrame, Option<Mask>);

/// Liest eine Spur im Hintergrund und hält das Bild, das gerade gilt.
/// Mit `segmenter` rechnet derselbe Thread die Maske gleich mit – so läuft
/// sie neben dem Zusammensetzen statt davor.
struct Track {
    rx: Receiver<Result<Decoded, String>>,
    current: Option<VideoFrame>,
    /// Maske zu `current`, noch nicht hochgeladen.
    mask: Option<Mask>,
    pending: Option<Decoded>,
    done: bool,
}

impl Track {
    fn spawn(mut source: Box<dyn FrameSource>, mut segmenter: Option<Box<dyn Segmenter>>) -> Self {
        let (tx, rx) = sync_channel(QUEUE);
        std::thread::spawn(move || {
            let mut last_mask: Option<Duration> = None;
            loop {
                let item = match source.next_frame() {
                    Ok(Some(f)) => {
                        let due = last_mask.is_none_or(|t| f.pts < t || f.pts - t >= MASK_INTERVAL);
                        let mask = segmenter
                            .as_mut()
                            .filter(|_| due)
                            .and_then(|s| s.mask(f.as_ref()));
                        if mask.is_some() {
                            last_mask = Some(f.pts);
                        }
                        Ok((f, mask))
                    }
                    Ok(None) => break,
                    Err(e) => Err(e),
                };
                let failed = item.is_err();
                // Empfänger weg = Export beendet.
                if tx.send(item).is_err() || failed {
                    break;
                }
            }
        });
        Self {
            rx,
            current: None,
            mask: None,
            pending: None,
            done: false,
        }
    }

    /// Bild für Zeitpunkt `t` bereitstellen. `early`: Vor dem ersten Bild
    /// gilt schon das erste. Gibt zurück, ob sich das Bild geändert hat.
    fn advance(&mut self, t: Duration, early: bool) -> Result<bool, VideoError> {
        let mut changed = false;
        loop {
            if self.pending.is_none() && !self.done {
                match self.rx.recv() {
                    Ok(Ok(f)) => self.pending = Some(f),
                    Ok(Err(e)) => return Err(VideoError::Source(e)),
                    Err(_) => self.done = true,
                }
            }
            let take = self
                .pending
                .as_ref()
                .is_some_and(|(f, _)| f.pts <= t || (early && self.current.is_none()));
            if !take {
                break;
            }
            if let Some((frame, mask)) = self.pending.take() {
                self.current = Some(frame);
                self.mask = mask;
            }
            changed = true;
        }
        Ok(changed)
    }
}

/// Setzt das Video nach `doc` zusammen und schreibt es in `sink`, der mit
/// [`output_size`] des Bildschirms angelegt sein muss. `progress` bekommt
/// den Fortschritt (0 bis 1); gibt es `false` zurück, wird abgebrochen.
pub fn export(
    doc: &VideoDoc,
    screen: Box<dyn FrameSource>,
    webcam: Option<Box<dyn FrameSource>>,
    sink: &mut dyn FrameSink,
    progress: &mut dyn FnMut(f32) -> bool,
) -> Result<ExportStats, VideoError> {
    export_with(doc, screen, webcam, ExportExtras::default(), sink, progress)
}

/// Wie [`export`], mit Kopfausschnitt, Hintergrund usw. Die Länge des
/// Videos ist die Aufnahme ohne Schnitte; der Encoder bekommt Zeiten der
/// Ausgabe.
pub fn export_with(
    doc: &VideoDoc,
    screen: Box<dyn FrameSource>,
    webcam: Option<Box<dyn FrameSource>>,
    extras: ExportExtras,
    sink: &mut dyn FrameSink,
    progress: &mut dyn FnMut(f32) -> bool,
) -> Result<ExportStats, VideoError> {
    let started = Instant::now();
    let (w, h) = extras.size.unwrap_or_else(|| output_size(screen.size()));
    let mut compositor = Compositor::new(w, h)?;
    let result = run(doc, screen, webcam, extras, sink, progress, &mut compositor);
    match result {
        Ok(frames) => {
            sink.finish().map_err(VideoError::Sink)?;
            Ok(ExportStats {
                frames,
                elapsed: started.elapsed(),
                adapter: compositor.adapter().to_string(),
            })
        }
        Err(e) => {
            sink.cancel();
            Err(e)
        }
    }
}

fn run(
    doc: &VideoDoc,
    screen: Box<dyn FrameSource>,
    webcam: Option<Box<dyn FrameSource>>,
    mut extras: ExportExtras,
    sink: &mut dyn FrameSink,
    progress: &mut dyn FnMut(f32) -> bool,
    compositor: &mut Compositor,
) -> Result<u64, VideoError> {
    let (w, h) = extras.size.unwrap_or_else(|| output_size(screen.size()));
    let frame = Size::new(w as f32, h as f32);
    let shows_webcam = doc.webcam.as_ref().is_some_and(|t| {
        t.sections
            .iter()
            .any(|s| s.mode != grabmark_core::WebcamMode::Hidden)
    });
    let screen_size = Size::new(screen.size().0 as f32, screen.size().1 as f32);
    let needs_mask = doc
        .webcam
        .as_ref()
        .is_some_and(|t| t.layout.background != grabmark_core::WebcamBackground::Original);
    let segmenter = extras.segmenter.take().filter(|_| needs_mask);
    let mut screen = Track::spawn(screen, None);
    let mut webcam = webcam
        .filter(|_| shows_webcam)
        .map(|w| Track::spawn(w, segmenter));

    // Zeiger und Klick-Kreis sind feste Bilder; die Szene skaliert sie.
    if extras.cursor.is_some() {
        let s = CURSOR_TEXTURE;
        let c = doc.cursor.click_color;
        for (source, data) in [
            (Source::Cursor, arrow_image(s)),
            (Source::Click, ring_image(s, [c.r, c.g, c.b])),
        ] {
            compositor.upload(
                source,
                FrameRef {
                    width: s,
                    height: s,
                    stride: s as usize * 4,
                    data: &data,
                },
            );
        }
    }
    let mut overlay_on = false;
    let fps = doc.fps.clamp(1, 120) as u64;
    let count = (output_duration(doc) as u64 * fps).div_ceil(1000).max(1);
    for i in 0..count {
        // Zeit im fertigen Video und die Stelle der Aufnahme dazu.
        let t = Duration::from_nanos(i * 1_000_000_000 / fps);
        let source_ms = source_time(doc, t.as_secs_f64() * 1000.0);
        let src = Duration::from_secs_f64(source_ms / 1000.0);
        if screen.advance(src, true)?
            && let Some(f) = &screen.current
        {
            compositor.upload(Source::Screen, f.as_ref());
        }
        let mut camera = None;
        if let Some(cam) = webcam.as_mut() {
            if cam.advance(src, false)?
                && let Some(f) = &cam.current
            {
                compositor.upload(Source::Webcam, f.as_ref());
                if let Some(mask) = cam.mask.take() {
                    compositor.upload_mask(&mask);
                }
            }
            camera = cam
                .current
                .as_ref()
                .map(|f| Size::new(f.width as f32, f.height as f32));
        }
        let mut scene = scene_full(
            doc,
            source_ms,
            frame,
            screen_size,
            camera,
            &SceneExtras {
                head: extras.head.as_ref(),
                cursor: extras.cursor.as_ref(),
                cursor_px: extras.cursor_px,
            },
        );
        if let Some(overlay) = extras.overlay.as_mut()
            && let Some(update) = overlay.frame(source_ms)
        {
            overlay_on = update.is_some();
            if let Some(f) = update {
                compositor.upload(Source::Overlay, f.as_ref());
            }
        }
        if overlay_on {
            scene.layers.push((
                Source::Overlay,
                Placement::plain(Rect::new(0.0, 0.0, frame.width, frame.height)),
            ));
        }
        compositor
            .render(&scene, |data, stride| sink.push(data, stride, t))?
            .map_err(VideoError::Sink)?;
        if !progress((i + 1) as f32 / count as f32) {
            return Err(VideoError::Cancelled);
        }
    }
    Ok(count)
}
