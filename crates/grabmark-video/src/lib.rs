//! Video (ADR 0011), plattformunabhängig: Webcam-Layout und Abschnitte
//! rechnen ([`layout`]), Abschnitte während der Aufnahme mitschreiben
//! ([`sections`]), abgebrochene Aufnahmen wiederherstellen ([`recovery`]) und – mit der Funktion `gpu` – den Export zusammensetzen
//! ([`export()`]).
//!
//! Dekodieren und Kodieren macht das System (`grabmark-media`); diese Crate
//! kennt nur die Schnittstellen [`FrameSource`] und [`FrameSink`]. So lässt
//! sich der Export ohne Bildschirm und ohne Videodateien testen.

pub mod cursor;
pub mod edit;
pub mod head;
pub mod layout;
pub mod recovery;
pub mod sections;
pub mod timeline;

#[cfg(feature = "gpu")]
mod export;
#[cfg(feature = "gif")]
mod gif;
#[cfg(feature = "gpu")]
mod gpu;
#[cfg(feature = "layers")]
pub mod overlay;
#[cfg(feature = "layers")]
pub mod sketch;

pub use cursor::CursorTrack;
#[cfg(feature = "gpu")]
pub use export::{ExportExtras, ExportStats, ExportTimes, export, export_with};
#[cfg(feature = "gif")]
pub use gif::{GIF_FPS, GifSink, gif_size};
#[cfg(feature = "gpu")]
pub use gpu::{Compositor, Readback, Ticket};
pub use head::{HeadFollower, HeadPath, HeadPose, LiveHead, head_crop};
pub use layout::{
    Placement, Scene, SceneExtras, Source, camera_frame_size, layout_from_bubble, scene_at,
    scene_full, scene_with_head,
};
pub use sections::SectionLog;

use std::time::Duration;

#[derive(Debug, thiserror::Error)]
pub enum VideoError {
    #[error("Keine Grafikkarte für den Export gefunden: {0}")]
    NoGpu(String),
    #[error("Grafikkarte: {0}")]
    Gpu(String),
    #[error("Video lesen: {0}")]
    Source(String),
    #[error("Video schreiben: {0}")]
    Sink(String),
    #[error("Export abgebrochen")]
    Cancelled,
}

/// Ein dekodiertes Bild, BGRA mit 8 Bit je Kanal.
#[derive(Debug, Clone, PartialEq)]
pub struct VideoFrame {
    /// Zeitpunkt ab Beginn des Videos.
    pub pts: Duration,
    pub width: u32,
    pub height: u32,
    /// Bytes pro Zeile (mindestens `width * 4`).
    pub stride: usize,
    pub data: Vec<u8>,
}

impl VideoFrame {
    pub fn as_ref(&self) -> FrameRef<'_> {
        FrameRef {
            width: self.width,
            height: self.height,
            stride: self.stride,
            data: &self.data,
        }
    }
}

/// Maske der Person in einem Webcam-Bild: ein Byte je Pixel, 255 = Person,
/// Zeilen ohne Lücke. Meist kleiner als das Bild (z. B. 256×144).
#[derive(Debug, Clone, PartialEq)]
pub struct Mask {
    pub width: u32,
    pub height: u32,
    pub data: Vec<u8>,
}

/// Trennt die Person vom Hintergrund (System: Apple Vision; später ONNX).
pub trait Segmenter: Send {
    /// Maske zum BGRA-Bild; `None`, wenn es nicht klappt (dann bleibt der
    /// Hintergrund unverändert).
    fn mask(&mut self, frame: FrameRef<'_>) -> Option<Mask>;
}

/// Liefert die Ebenen (Pfeile, Text …) als durchsichtiges Bild in Grösse
/// des Videos (Etappe 6).
pub trait OverlaySource: Send {
    /// Bild zur Quellzeit: `None` = unverändert seit dem letzten Aufruf,
    /// `Some(None)` = nichts zu sehen, `Some(Some(bild))` = neues Bild (BGRA,
    /// gerade Deckkraft).
    fn frame(&mut self, source_ms: f64) -> Option<Option<VideoFrame>>;
}

/// Geliehenes Bild, BGRA.
#[derive(Debug, Clone, Copy)]
pub struct FrameRef<'a> {
    pub width: u32,
    pub height: u32,
    pub stride: usize,
    pub data: &'a [u8],
}

/// Liefert die Bilder einer Videospur der Reihe nach (Dekoder des Systems).
/// Läuft im eigenen Thread.
pub trait FrameSource: Send {
    /// Grösse der Bilder in Pixeln.
    fn size(&self) -> (u32, u32);
    /// Nächstes Bild; `None` am Ende.
    fn next_frame(&mut self) -> Result<Option<VideoFrame>, String>;
}

/// Nimmt die fertigen Bilder an (Encoder des Systems).
/// `Send`: Der Export liest und kodiert in einem eigenen Thread.
pub trait FrameSink: Send {
    /// Ein Bild, BGRA mit `stride` Bytes pro Zeile, Grösse wie beim Anlegen.
    fn push(&mut self, bgra: &[u8], stride: usize, pts: Duration) -> Result<(), String>;
    /// Schliesst die Datei ab (mit Ton, falls vorhanden).
    fn finish(&mut self) -> Result<(), String>;
    /// Bricht ab und löscht die Datei.
    fn cancel(&mut self);
}

/// Leere Spur einer festen Grösse: ein schwarzes Bild, dann Ende. Steht bei
/// „nur Webcam“ an der Stelle des Bildschirms und gibt die Grösse des
/// fertigen Videos vor.
pub struct BlankSource {
    size: (u32, u32),
    done: bool,
}

impl BlankSource {
    pub fn new(size: (u32, u32)) -> Self {
        Self { size, done: false }
    }
}

impl FrameSource for BlankSource {
    fn size(&self) -> (u32, u32) {
        self.size
    }

    fn next_frame(&mut self) -> Result<Option<VideoFrame>, String> {
        if std::mem::replace(&mut self.done, true) {
            return Ok(None);
        }
        // Ein Bildpunkt reicht: Die Szene zeigt die Spur nicht.
        Ok(Some(VideoFrame {
            pts: Duration::ZERO,
            width: 1,
            height: 1,
            stride: 4,
            data: vec![0, 0, 0, 255],
        }))
    }
}

/// Grösse des fertigen Videos: wie der Bildschirm, mit geraden Kanten
/// (H.264/HEVC mit 4:2:0).
pub fn output_size(screen: (u32, u32)) -> (u32, u32) {
    ((screen.0 & !1).max(2), (screen.1 & !1).max(2))
}

/// Grösse nach der Einstellung *Auflösung*: kürzere Seite höchstens so
/// lang, Seitenverhältnis bleibt, nie grösser als `frame`.
pub fn export_size(frame: (u32, u32), resolution: grabmark_core::VideoResolution) -> (u32, u32) {
    let frame = output_size(frame);
    let Some(limit) = resolution.short_side() else {
        return frame;
    };
    let short = frame.0.min(frame.1);
    if short <= limit {
        return frame;
    }
    let k = limit as f64 / short as f64;
    let side = |v: u32| ((v as f64 * k).round() as u32 & !1).max(2);
    (side(frame.0), side(frame.1))
}

/// Längste Seite, die H.264 auf Apple Silicon in Hardware kodiert. Darüber
/// rechnet der Prozessor, gemessen ≈ 15× langsamer (6016 × 3384: 170 ms
/// statt 11 ms je Bild); die meisten Abspieler können es ohnehin nicht.
pub const H264_MAX_SIDE: u32 = 4096;

/// Grösse, in der kodiert wird: [`export_size`] nach der Einstellung, bei
/// H.264 zusätzlich höchstens [`H264_MAX_SIDE`] (HEVC kann bis 8K).
pub fn encode_size(frame: (u32, u32), encoding: &grabmark_core::VideoEncoding) -> (u32, u32) {
    let size = export_size(frame, encoding.resolution);
    let long = size.0.max(size.1);
    if encoding.codec != grabmark_core::VideoCodec::H264 || long <= H264_MAX_SIDE {
        return size;
    }
    let k = H264_MAX_SIDE as f64 / long as f64;
    let side = |v: u32| ((v as f64 * k).floor() as u32 & !1).max(2);
    (side(size.0), side(size.1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use grabmark_core::VideoResolution;

    #[test]
    fn h264_hoechstens_4096() {
        use grabmark_core::{VideoCodec, VideoEncoding};
        let h264 = VideoEncoding::default();
        assert_eq!(h264.codec, VideoCodec::H264);
        assert_eq!(encode_size((6016, 3384), &h264), (4096, 2304));
        assert_eq!(encode_size((3384, 6016), &h264), (2304, 4096));
        assert_eq!(encode_size((3840, 2160), &h264), (3840, 2160));
        let hevc = VideoEncoding {
            codec: VideoCodec::Hevc,
            ..h264
        };
        assert_eq!(encode_size((6016, 3384), &hevc), (6016, 3384));
        // Die Einstellung *Auflösung* gilt weiter.
        let small = VideoEncoding {
            resolution: VideoResolution::P1080,
            ..h264
        };
        assert_eq!(encode_size((6016, 3384), &small), (1920, 1080));
    }

    #[test]
    fn aufloesung_begrenzt_kuerzere_seite() {
        let retina = (2880, 1800);
        assert_eq!(export_size(retina, VideoResolution::Original), retina);
        assert_eq!(export_size(retina, VideoResolution::P1080), (1728, 1080));
        assert_eq!(export_size(retina, VideoResolution::P720), (1152, 720));
        // Nie vergrössern
        assert_eq!(
            export_size((1280, 720), VideoResolution::P2160),
            (1280, 720)
        );
        // Hochformat: Breite ist die kürzere Seite
        assert_eq!(
            export_size((1080, 1920), VideoResolution::P720),
            (720, 1280)
        );
    }

    #[test]
    fn ausgabe_hat_gerade_kanten() {
        assert_eq!(output_size((1921, 1081)), (1920, 1080));
        assert_eq!(output_size((1, 1)), (2, 2));
    }
}
