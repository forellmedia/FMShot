//! Umwandlungen zwischen Bildformaten.

use fmshot_core::RgbaImage;
use tiny_skia::{IntSize, Pixmap};

#[derive(Debug, thiserror::Error, PartialEq, Eq)]
pub enum CodecError {
    #[error("PNG konnte nicht gelesen werden: {0}")]
    Png(String),
    #[error("Bildformat wird nicht unterstützt: {0}")]
    Unsupported(String),
}

/// Liest ein PNG in RGBA mit gerader Deckkraft (8 Bit).
pub fn decode_png(bytes: &[u8]) -> Result<RgbaImage, CodecError> {
    let mut decoder = png::Decoder::new(std::io::Cursor::new(bytes));
    decoder.set_transformations(png::Transformations::EXPAND | png::Transformations::STRIP_16);
    let mut reader = decoder
        .read_info()
        .map_err(|e| CodecError::Png(e.to_string()))?;
    let size = reader
        .output_buffer_size()
        .ok_or_else(|| CodecError::Png("Bild zu gross".into()))?;
    let mut buf = vec![0; size];
    let info = reader
        .next_frame(&mut buf)
        .map_err(|e| CodecError::Png(e.to_string()))?;
    buf.truncate(info.buffer_size());
    let (w, h) = (info.width, info.height);
    let rgba = match info.color_type {
        png::ColorType::Rgba => buf,
        png::ColorType::Rgb => buf
            .as_chunks::<3>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[1], p[2], 255])
            .collect(),
        png::ColorType::GrayscaleAlpha => buf
            .as_chunks::<2>()
            .0
            .iter()
            .flat_map(|p| [p[0], p[0], p[0], p[1]])
            .collect(),
        png::ColorType::Grayscale => buf.iter().flat_map(|&g| [g, g, g, 255]).collect(),
        other => return Err(CodecError::Unsupported(format!("{other:?}"))),
    };
    RgbaImage::new(w, h, rgba).map_err(|e| CodecError::Png(e.to_string()))
}

/// Liest PNG, JPEG oder WebP (am Inhalt erkannt).
pub fn decode_image(bytes: &[u8]) -> Result<RgbaImage, CodecError> {
    if bytes.starts_with(b"\x89PNG") {
        return decode_png(bytes);
    }
    if bytes.starts_with(&[0xFF, 0xD8]) {
        let mut decoder = jpeg_decoder::Decoder::new(std::io::Cursor::new(bytes));
        let pixels = decoder
            .decode()
            .map_err(|e| CodecError::Unsupported(e.to_string()))?;
        let info = decoder
            .info()
            .ok_or_else(|| CodecError::Unsupported("JPEG ohne Kopf".into()))?;
        let rgba: Vec<u8> = match info.pixel_format {
            jpeg_decoder::PixelFormat::RGB24 => pixels
                .as_chunks::<3>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect(),
            jpeg_decoder::PixelFormat::L8 => pixels.iter().flat_map(|&g| [g, g, g, 255]).collect(),
            other => return Err(CodecError::Unsupported(format!("JPEG {other:?}"))),
        };
        return RgbaImage::new(info.width as u32, info.height as u32, rgba)
            .map_err(|e| CodecError::Unsupported(e.to_string()));
    }
    if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        let mut decoder = image_webp::WebPDecoder::new(std::io::Cursor::new(bytes))
            .map_err(|e| CodecError::Unsupported(e.to_string()))?;
        let (w, h) = decoder.dimensions();
        let alpha = decoder.has_alpha();
        let mut buf = vec![0; decoder.output_buffer_size().unwrap_or(0)];
        decoder
            .read_image(&mut buf)
            .map_err(|e| CodecError::Unsupported(e.to_string()))?;
        let rgba = if alpha {
            buf
        } else {
            buf.as_chunks::<3>()
                .0
                .iter()
                .flat_map(|p| [p[0], p[1], p[2], 255])
                .collect()
        };
        return RgbaImage::new(w, h, rgba).map_err(|e| CodecError::Unsupported(e.to_string()));
    }
    Err(CodecError::Unsupported("unbekanntes Bildformat".into()))
}

/// RGBA mit gerader Deckkraft → vormultiplizierte Pixmap.
pub(crate) fn to_pixmap(image: &RgbaImage) -> Option<Pixmap> {
    let mut data = image.data().to_vec();
    for px in data.as_chunks_mut::<4>().0 {
        let a = px[3] as u16;
        if a != 255 {
            for c in &mut px[..3] {
                *c = ((*c as u16 * a + 127) / 255) as u8;
            }
        }
    }
    let size = IntSize::from_wh(image.width(), image.height())?;
    Pixmap::from_vec(data, size)
}

/// Vormultiplizierte Pixmap → RGBA mit gerader Deckkraft.
pub(crate) fn to_rgba_image(pixmap: &Pixmap) -> RgbaImage {
    let mut data = Vec::with_capacity(pixmap.data().len());
    for px in pixmap.pixels() {
        let c = px.demultiply();
        data.extend_from_slice(&[c.red(), c.green(), c.blue(), c.alpha()]);
    }
    RgbaImage::new(pixmap.width(), pixmap.height(), data)
        .expect("Pixmap liefert immer passende Länge")
}
