//! Windows: `Windows.Media.Ocr` – offline, Sprachen laut Windows-Einstellungen.
//!
//! Die Engine verarbeitet nur Bilder bis [`OcrEngine::MaxImageDimension`]
//! (meist 10 000 px). Lange Scroll-Aufnahmen werden deshalb in überlappenden
//! Streifen erkannt. Sehr kleine Ausschnitte (ein einzelnes Wort) erkennt die
//! Engine schlecht – sie werden vorher ganzzahlig vergrössert.

use fmshot_core::{Rect, RgbaImage};
use windows::Graphics::Imaging::{BitmapPixelFormat, SoftwareBitmap};
use windows::Media::Ocr::OcrEngine as WinOcrEngine;
use windows::Security::Cryptography::CryptographicBuffer;

use crate::{OcrEngine, OcrError, TextBlock};

/// Überlappung zweier Streifen – grösser als jede realistische Textzeile.
const TILE_OVERLAP: u32 = 200;
/// Kleinere Bilder werden vergrössert, bis die kürzere Kante so lang ist.
const MIN_EDGE: u32 = 120;
const MAX_UPSCALE: u32 = 4;

pub struct WindowsOcr;

impl WindowsOcr {
    /// `None`, wenn Windows keine einzige OCR-Sprache installiert hat.
    pub fn new() -> Option<Self> {
        let count = WinOcrEngine::AvailableRecognizerLanguages()
            .and_then(|l| l.Size())
            .unwrap_or(0);
        (count > 0).then_some(Self)
    }
}

impl OcrEngine for WindowsOcr {
    fn languages(&self) -> Vec<String> {
        let Ok(list) = WinOcrEngine::AvailableRecognizerLanguages() else {
            return Vec::new();
        };
        (0..list.Size().unwrap_or(0))
            .filter_map(|i| list.GetAt(i).ok())
            .filter_map(|l| l.LanguageTag().ok())
            .map(|t| t.to_string())
            .collect()
    }

    fn recognize(&self, image: &RgbaImage) -> Result<Vec<TextBlock>, OcrError> {
        let engine = engine()?;
        let max = WinOcrEngine::MaxImageDimension()
            .map_err(backend)?
            .max(TILE_OVERLAP * 4);
        let (w, h) = (image.width(), image.height());
        if w == 0 || h == 0 {
            return Ok(Vec::new());
        }
        // Zu breite Bilder (selten) verkleinern, damit die Breite passt.
        let shrink = w.div_ceil(max).max(1);
        let grow = upscale_factor(w / shrink, h / shrink, max);
        let mut blocks = Vec::new();
        for tile in tiles(h, max / grow * shrink, TILE_OVERLAP) {
            let part = scaled_bgra(image, tile.y0, tile.y1, shrink, grow);
            let lines = recognize_bgra(&engine, &part)?;
            let factor = shrink as f32 / grow as f32;
            for (text, r) in lines {
                let bounds = Rect::new(
                    r.x * factor,
                    tile.y0 as f32 + r.y * factor,
                    r.width * factor,
                    r.height * factor,
                );
                let center = bounds.y + bounds.height / 2.0;
                if center >= tile.keep_from as f32 && center < tile.keep_to as f32 {
                    blocks.push(TextBlock {
                        text,
                        bounds,
                        // Windows liefert keine Sicherheit pro Zeile.
                        confidence: 1.0,
                    });
                }
            }
        }
        Ok(blocks)
    }
}

/// Engine für die Sprachen des Benutzerprofils, sonst die erste verfügbare.
fn engine() -> Result<WinOcrEngine, OcrError> {
    if let Ok(e) = WinOcrEngine::TryCreateFromUserProfileLanguages() {
        return Ok(e);
    }
    let list = WinOcrEngine::AvailableRecognizerLanguages().map_err(backend)?;
    let first = list.GetAt(0).map_err(|_| {
        OcrError::Unavailable(
            "keine OCR-Sprache installiert (Einstellungen → Zeit und Sprache → Sprache)".into(),
        )
    })?;
    WinOcrEngine::TryCreateFromLanguage(&first).map_err(backend)
}

/// Bild im Format der Engine (BGRA, eine Kopie).
struct Bgra {
    width: u32,
    height: u32,
    data: Vec<u8>,
}

/// Zeilen mit Text und Lage (Pixel im übergebenen Bild).
fn recognize_bgra(engine: &WinOcrEngine, img: &Bgra) -> Result<Vec<(String, Rect)>, OcrError> {
    let buffer = CryptographicBuffer::CreateFromByteArray(&img.data).map_err(backend)?;
    let bitmap = SoftwareBitmap::CreateCopyFromBuffer(
        &buffer,
        BitmapPixelFormat::Bgra8,
        img.width as i32,
        img.height as i32,
    )
    .map_err(backend)?;
    // Blockiert bis zum Ergebnis – wir laufen immer in einem Hintergrund-Thread.
    let result = engine
        .RecognizeAsync(&bitmap)
        .and_then(|op| op.join())
        .map_err(backend)?;
    let lines = result.Lines().map_err(backend)?;
    let mut out = Vec::new();
    for i in 0..lines.Size().map_err(backend)? {
        let line = lines.GetAt(i).map_err(backend)?;
        let text = line.Text().map_err(backend)?.to_string();
        let words = line.Words().map_err(backend)?;
        let mut bounds: Option<(f32, f32, f32, f32)> = None;
        for j in 0..words.Size().map_err(backend)? {
            let r = words
                .GetAt(j)
                .and_then(|w| w.BoundingRect())
                .map_err(backend)?;
            let (x0, y0, x1, y1) = (r.X, r.Y, r.X + r.Width, r.Y + r.Height);
            bounds = Some(match bounds {
                Some((a, b, c, d)) => (a.min(x0), b.min(y0), c.max(x1), d.max(y1)),
                None => (x0, y0, x1, y1),
            });
        }
        if let Some((x0, y0, x1, y1)) = bounds
            && !text.trim().is_empty()
        {
            out.push((text, Rect::new(x0, y0, x1 - x0, y1 - y0)));
        }
    }
    Ok(out)
}

/// Zeilen `y0..y1` des Bildes als BGRA; `shrink` verkleinert (jedes n-te
/// Pixel), `grow` vergrössert (Pixel wiederholen). Nie beides zugleich > 1.
fn scaled_bgra(image: &RgbaImage, y0: u32, y1: u32, shrink: u32, grow: u32) -> Bgra {
    let src_w = image.width() as usize;
    let width = image.width() / shrink * grow;
    let height = (y1 - y0) / shrink * grow;
    let src = image.data();
    let mut data = Vec::with_capacity(width as usize * height as usize * 4);
    for y in 0..height {
        let sy = y0 as usize + (y / grow * shrink) as usize;
        let row = &src[sy * src_w * 4..(sy + 1) * src_w * 4];
        for x in 0..width {
            let sx = (x / grow * shrink) as usize * 4;
            let px = &row[sx..sx + 4];
            // Durchsichtiges (runde Fensterecken) auf Weiss.
            let a = px[3] as u32;
            let mix = |c: u8| ((c as u32 * a + 255 * (255 - a) + 127) / 255) as u8;
            data.extend_from_slice(&[mix(px[2]), mix(px[1]), mix(px[0]), 255]);
        }
    }
    Bgra {
        width,
        height,
        data,
    }
}

/// Ganzzahlige Vergrösserung für kleine Bilder (höchstens [`MAX_UPSCALE`],
/// nie über die Grenze der Engine).
fn upscale_factor(w: u32, h: u32, max: u32) -> u32 {
    let short = w.min(h).max(1);
    let long = w.max(h).max(1);
    let wanted = MIN_EDGE.div_ceil(short).clamp(1, MAX_UPSCALE);
    wanted.min((max / long).max(1))
}

/// Ein waagrechter Streifen des Bildes. Zeilen, deren Mitte in
/// `keep_from..keep_to` liegt, gehören zu diesem Streifen – so wird keine
/// Zeile aus der Überlappung doppelt gezählt.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Tile {
    y0: u32,
    y1: u32,
    keep_from: u32,
    keep_to: u32,
}

fn tiles(height: u32, max: u32, overlap: u32) -> Vec<Tile> {
    let max = max.max(overlap * 2 + 1);
    if height <= max {
        return vec![Tile {
            y0: 0,
            y1: height,
            keep_from: 0,
            keep_to: height,
        }];
    }
    let step = max - overlap;
    let mut out: Vec<Tile> = Vec::new();
    let mut y0 = 0;
    loop {
        let y1 = (y0 + max).min(height);
        let keep_from = if y0 == 0 { 0 } else { y0 + overlap / 2 };
        let last = y1 == height;
        let keep_to = if last { height } else { y1 - overlap / 2 };
        out.push(Tile {
            y0,
            y1,
            keep_from,
            keep_to,
        });
        if last {
            return out;
        }
        y0 += step;
    }
}

fn backend(e: windows::core::Error) -> OcrError {
    OcrError::Backend(e.message().to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn kleines_bild_ist_ein_streifen() {
        assert_eq!(
            tiles(500, 10_000, 200),
            [Tile {
                y0: 0,
                y1: 500,
                keep_from: 0,
                keep_to: 500
            }]
        );
    }

    #[test]
    fn streifen_decken_alles_ohne_luecke_ab() {
        let t = tiles(25_000, 10_000, 200);
        assert_eq!(t.first().map(|t| t.keep_from), Some(0));
        assert_eq!(t.last().map(|t| t.keep_to), Some(25_000));
        for pair in t.windows(2) {
            assert_eq!(pair[0].keep_to, pair[1].keep_from);
            assert!(pair[1].y0 < pair[0].y1, "Streifen überlappen");
        }
        assert!(t.iter().all(|t| t.y1 - t.y0 <= 10_000));
    }

    #[test]
    fn einzelnes_wort_wird_vergroessert() {
        assert_eq!(upscale_factor(200, 30, 10_000), 4);
        assert_eq!(upscale_factor(800, 600, 10_000), 1);
        // Nie über die Grenze der Engine.
        assert_eq!(upscale_factor(9_000, 20, 10_000), 1);
    }

    #[test]
    fn bgra_vergroessert_und_tauscht_kanaele() {
        let img = RgbaImage::new(1, 1, vec![10, 20, 30, 255]).unwrap();
        let b = scaled_bgra(&img, 0, 1, 1, 2);
        assert_eq!((b.width, b.height), (2, 2));
        assert_eq!(&b.data[..4], &[30, 20, 10, 255]);
    }
}
