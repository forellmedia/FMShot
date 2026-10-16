use fmshot_core::RgbaImage;

/// PNG mit schneller Kompression. `scale_factor` landet als Auflösung im
/// pHYs-Block – so fügen Programme Retina-Aufnahmen in richtiger Grösse ein.
pub fn encode_png(image: &RgbaImage, scale_factor: f32) -> Result<Vec<u8>, png::EncodingError> {
    let mut out = Vec::with_capacity(image.data().len() / 3);
    {
        let mut enc = png::Encoder::new(&mut out, image.width(), image.height());
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.set_compression(png::Compression::Fast);
        // 72 dpi × Skalierung, umgerechnet in Pixel pro Meter.
        let ppm = (72.0 * scale_factor.max(1.0) / 0.0254).round() as u32;
        enc.set_pixel_dims(Some(png::PixelDimensions {
            xppu: ppm,
            yppu: ppm,
            unit: png::Unit::Meter,
        }));
        let mut writer = enc.write_header()?;
        writer.write_image_data(image.data())?;
    }
    Ok(out)
}

/// Verkleinert auf höchstens `max` Pixel an der längsten Kante (Flächenmittel).
/// Kleinere Bilder bleiben unverändert.
pub fn thumbnail(image: &RgbaImage, max: u32) -> RgbaImage {
    let (w, h) = (image.width(), image.height());
    let longest = w.max(h);
    if longest <= max || w == 0 || h == 0 {
        return image.clone();
    }
    let tw = ((w as u64 * max as u64) / longest as u64).max(1) as u32;
    let th = ((h as u64 * max as u64) / longest as u64).max(1) as u32;
    let src = image.data();
    let mut out = Vec::with_capacity(tw as usize * th as usize * 4);
    for ty in 0..th {
        let y0 = (ty as u64 * h as u64 / th as u64) as u32;
        let y1 = (((ty + 1) as u64 * h as u64 / th as u64) as u32).max(y0 + 1);
        for tx in 0..tw {
            let x0 = (tx as u64 * w as u64 / tw as u64) as u32;
            let x1 = (((tx + 1) as u64 * w as u64 / tw as u64) as u32).max(x0 + 1);
            let mut sum = [0u64; 4];
            for y in y0..y1 {
                let row = y as usize * w as usize * 4;
                for x in x0..x1 {
                    let i = row + x as usize * 4;
                    for c in 0..4 {
                        sum[c] += src[i + c] as u64;
                    }
                }
            }
            let n = ((y1 - y0) * (x1 - x0)) as u64;
            out.extend(sum.iter().map(|v| ((v + n / 2) / n) as u8));
        }
    }
    RgbaImage::new(tw, th, out).expect("Länge passt zur Grösse")
}

/// Verlustfreies WebP für Vorschaubilder.
pub(crate) fn encode_webp(image: &RgbaImage) -> Result<Vec<u8>, image_webp::EncodingError> {
    let mut out = Vec::new();
    image_webp::WebPEncoder::new(&mut out).encode(
        image.data(),
        image.width(),
        image.height(),
        image_webp::ColorType::Rgba8,
    )?;
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn png_beginnt_mit_signatur() {
        let png = encode_png(&RgbaImage::filled(3, 2, [1, 2, 3, 255]), 2.0).unwrap();
        assert_eq!(&png[..8], b"\x89PNG\r\n\x1a\n");
    }

    #[test]
    fn vorschau_behaelt_seitenverhaeltnis() {
        let t = thumbnail(&RgbaImage::filled(1000, 500, [10, 20, 30, 255]), 320);
        assert_eq!((t.width(), t.height()), (320, 160));
        assert_eq!(t.pixel(100, 100), Some([10, 20, 30, 255]));
    }

    #[test]
    fn kleines_bild_bleibt_gleich() {
        let img = RgbaImage::filled(100, 50, [0, 0, 0, 255]);
        assert_eq!(thumbnail(&img, 320), img);
    }
}
