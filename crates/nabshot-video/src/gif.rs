//! Export als animiertes GIF (Etappe 4): verkleinert, mit weniger Bildern
//! pro Sekunde, Farben je Bild mit NeuQuant (`gif`-Crate, MIT/Apache-2.0).

use std::fs::File;
use std::io::BufWriter;
use std::path::{Path, PathBuf};
use std::time::Duration;

use crate::FrameSink;

/// Längste Kante des GIFs in Pixeln.
pub const GIF_MAX_SIZE: u32 = 960;
/// Bilder pro Sekunde im GIF.
pub const GIF_FPS: u32 = 12;
/// 1 = beste Farben, 30 = schnell. 10 ist ein guter Mittelweg.
const QUANT_SPEED: i32 = 10;

/// Schreibt die Bilder des Exports als GIF.
pub struct GifSink {
    encoder: Option<::gif::Encoder<BufWriter<File>>>,
    path: PathBuf,
    source: (u32, u32),
    size: (u32, u32),
    /// Zeit des nächsten Bilds, das ins GIF kommt.
    next: Duration,
    /// Zeit und Pixel des zuletzt angenommenen Bilds (wird geschrieben,
    /// sobald seine Dauer feststeht).
    pending: Option<(Duration, Vec<u8>)>,
}

/// Grösse des GIFs zu einer Bildgrösse.
pub fn gif_size(source: (u32, u32)) -> (u32, u32) {
    let (w, h) = (source.0.max(1), source.1.max(1));
    let scale = (GIF_MAX_SIZE as f32 / w.max(h) as f32).min(1.0);
    (
        ((w as f32 * scale).round() as u32).max(1),
        ((h as f32 * scale).round() as u32).max(1),
    )
}

impl GifSink {
    /// `source` = Grösse der Bilder, die ankommen.
    pub fn create(path: &Path, source: (u32, u32)) -> Result<Self, String> {
        let size = gif_size(source);
        log::debug!(
            "GIF {}: {}×{} → {}×{}, {GIF_FPS} fps",
            path.display(),
            source.0,
            source.1,
            size.0,
            size.1
        );
        let file = File::create(path).map_err(|e| {
            log::error!("GIF {} anlegen fehlgeschlagen: {e}", path.display());
            e.to_string()
        })?;
        let mut encoder =
            ::gif::Encoder::new(BufWriter::new(file), size.0 as u16, size.1 as u16, &[])
                .map_err(|e| e.to_string())?;
        encoder
            .set_repeat(::gif::Repeat::Infinite)
            .map_err(|e| e.to_string())?;
        Ok(Self {
            encoder: Some(encoder),
            path: path.to_path_buf(),
            source,
            size,
            next: Duration::ZERO,
            pending: None,
        })
    }

    fn write(&mut self, rgba: &mut [u8], delay: Duration) -> Result<(), String> {
        let Some(encoder) = self.encoder.as_mut() else {
            return Ok(());
        };
        let mut frame = ::gif::Frame::from_rgba_speed(
            self.size.0 as u16,
            self.size.1 as u16,
            rgba,
            QUANT_SPEED,
        );
        frame.delay = (delay.as_millis() / 10).clamp(2, u16::MAX as u128) as u16;
        encoder.write_frame(&frame).map_err(|e| {
            log::error!("GIF-Bild schreiben fehlgeschlagen: {e}");
            e.to_string()
        })
    }
}

/// BGRA (mit Zeilenlänge) → RGBA in Zielgrösse, Mittelwert über die Fläche.
pub fn downscale(bgra: &[u8], stride: usize, source: (u32, u32), size: (u32, u32)) -> Vec<u8> {
    let (sw, sh) = source;
    let (dw, dh) = size;
    let mut out = Vec::with_capacity((dw * dh * 4) as usize);
    for y in 0..dh {
        let y0 = y * sh / dh;
        let y1 = ((y + 1) * sh / dh).max(y0 + 1).min(sh);
        for x in 0..dw {
            let x0 = x * sw / dw;
            let x1 = ((x + 1) * sw / dw).max(x0 + 1).min(sw);
            let mut sum = [0u32; 3];
            let mut n = 0;
            for yy in y0..y1 {
                let row = yy as usize * stride;
                for xx in x0..x1 {
                    let i = row + xx as usize * 4;
                    sum[0] += bgra[i + 2] as u32;
                    sum[1] += bgra[i + 1] as u32;
                    sum[2] += bgra[i] as u32;
                    n += 1;
                }
            }
            let n = n.max(1);
            out.extend_from_slice(&[
                (sum[0] / n) as u8,
                (sum[1] / n) as u8,
                (sum[2] / n) as u8,
                255,
            ]);
        }
    }
    out
}

impl FrameSink for GifSink {
    fn push(&mut self, bgra: &[u8], stride: usize, pts: Duration) -> Result<(), String> {
        if pts < self.next {
            return Ok(());
        }
        let step = Duration::from_nanos(1_000_000_000 / GIF_FPS as u64);
        while self.next <= pts {
            self.next += step;
        }
        let rgba = downscale(bgra, stride, self.source, self.size);
        if let Some((t, mut previous)) = self.pending.take() {
            self.write(&mut previous, pts.saturating_sub(t))?;
        }
        self.pending = Some((pts, rgba));
        Ok(())
    }

    fn finish(&mut self) -> Result<(), String> {
        if let Some((_, mut last)) = self.pending.take() {
            let step = Duration::from_nanos(1_000_000_000 / GIF_FPS as u64);
            self.write(&mut last, step)?;
        }
        // Encoder schliessen = Datei abschliessen.
        drop(self.encoder.take());
        log::debug!("GIF {} abgeschlossen", self.path.display());
        Ok(())
    }

    fn cancel(&mut self) {
        drop(self.encoder.take());
        if let Err(e) = std::fs::remove_file(&self.path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!("GIF {} löschen fehlgeschlagen: {e}", self.path.display());
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grosse_bilder_werden_verkleinert() {
        assert_eq!(gif_size((1920, 1080)), (960, 540));
        assert_eq!(gif_size((400, 300)), (400, 300));
    }

    #[test]
    fn verkleinern_mittelt_farben() {
        // 2×1 BGRA: blau und rot → ein Pixel lila
        let bgra = [255, 0, 0, 255, 0, 0, 255, 255];
        assert_eq!(downscale(&bgra, 8, (2, 1), (1, 1)), vec![127, 0, 127, 255]);
    }

    #[test]
    fn gif_wird_geschrieben() {
        let dir = std::env::temp_dir().join(format!("nabshot-gif-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("test.gif");
        let mut sink = GifSink::create(&path, (4, 2)).unwrap();
        let frame = vec![200u8; 4 * 2 * 4];
        for i in 0..30 {
            sink.push(&frame, 16, Duration::from_millis(i * 33))
                .unwrap();
        }
        sink.finish().unwrap();
        let bytes = std::fs::read(&path).unwrap();
        assert_eq!(&bytes[..6], b"GIF89a");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
