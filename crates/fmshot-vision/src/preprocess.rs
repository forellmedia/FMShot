//! Bild für das Modell vorbereiten – wie beim Training (`SiglipImageProcessor`):
//! auf 224×224 skalieren (Seitenverhältnis wird nicht beibehalten), bilinear
//! mit Tiefpass wie Pillow, Werte auf −1 … 1, Kanäle nacheinander (CHW).

use fmshot_core::RgbaImage;

/// Kantenlänge des Modellbildes.
pub const SIZE: usize = 224;

/// Pixelwerte für den Eingang `pixel_values` (3 × 224 × 224).
/// Durchsichtige Stellen werden auf Weiss gelegt.
pub fn pixel_values(image: &RgbaImage) -> Vec<f32> {
    let (w, h) = (image.width() as usize, image.height() as usize);
    let rgb = opaque_rgb(image);
    let horizontal = resample(&rgb, w, h, SIZE, Axis::X);
    let resized = resample(&horizontal, SIZE, h, SIZE, Axis::Y);

    let plane = SIZE * SIZE;
    let mut out = vec![0.0; 3 * plane];
    for (i, px) in resized.as_chunks::<3>().0.iter().enumerate() {
        for c in 0..3 {
            out[c * plane + i] = px[c] / 255.0 * 2.0 - 1.0;
        }
    }
    out
}

fn opaque_rgb(image: &RgbaImage) -> Vec<f32> {
    image
        .data()
        .as_chunks::<4>()
        .0
        .iter()
        .flat_map(|p| {
            let a = f32::from(p[3]) / 255.0;
            let white = 255.0 * (1.0 - a);
            [0, 1, 2].map(|c| f32::from(p[c]) * a + white)
        })
        .collect()
}

#[derive(Clone, Copy)]
enum Axis {
    X,
    Y,
}

/// Eine Achse auf `out` Pixel bringen (RGB, f32). Dreiecksfilter, beim
/// Verkleinern auf den Massstab verbreitert – wie Pillows `BILINEAR`.
fn resample(src: &[f32], w: usize, h: usize, out: usize, axis: Axis) -> Vec<f32> {
    let len = match axis {
        Axis::X => w,
        Axis::Y => h,
    };
    let taps = weights(len, out);
    let (ow, oh) = match axis {
        Axis::X => (out, h),
        Axis::Y => (w, out),
    };
    let mut dst = vec![0.0; ow * oh * 3];
    for y in 0..oh {
        for x in 0..ow {
            let (i, fixed) = match axis {
                Axis::X => (x, y),
                Axis::Y => (y, x),
            };
            let (start, ref ws) = taps[i];
            let mut acc = [0.0f32; 3];
            for (k, wgt) in ws.iter().enumerate() {
                let s = match axis {
                    Axis::X => (fixed * w + start + k) * 3,
                    Axis::Y => ((start + k) * w + fixed) * 3,
                };
                for c in 0..3 {
                    acc[c] += src[s + c] * wgt;
                }
            }
            dst[(y * ow + x) * 3..][..3].copy_from_slice(&acc);
        }
    }
    dst
}

/// Je Zielpixel: erster Quellpixel und normierte Gewichte.
fn weights(len: usize, out: usize) -> Vec<(usize, Vec<f32>)> {
    let scale = len as f32 / out as f32;
    let filter_scale = scale.max(1.0);
    let support = filter_scale;
    (0..out)
        .map(|i| {
            let center = (i as f32 + 0.5) * scale;
            let start = ((center - support + 0.5).floor().max(0.0)) as usize;
            let end = ((center + support + 0.5).floor() as usize)
                .min(len)
                .max(start + 1);
            let mut ws: Vec<f32> = (start..end)
                .map(|j| (1.0 - ((j as f32 + 0.5 - center) / filter_scale).abs()).max(0.0))
                .collect();
            let sum: f32 = ws.iter().sum();
            if sum > 0.0 {
                ws.iter_mut().for_each(|v| *v /= sum);
            } else {
                ws.iter_mut().for_each(|v| *v = 1.0 / (end - start) as f32);
            }
            (start, ws)
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn einfarbig_bleibt_einfarbig_in_jeder_groesse() {
        for (w, h) in [(1, 1), (100, 30), (2880, 1800), (224, 224)] {
            let v = pixel_values(&RgbaImage::filled(w, h, [255, 0, 128, 255]));
            assert_eq!(v.len(), 3 * SIZE * SIZE);
            let plane = SIZE * SIZE;
            assert!(
                v[..plane].iter().all(|x| (x - 1.0).abs() < 1e-4),
                "{w}×{h} rot"
            );
            assert!(
                v[plane..2 * plane].iter().all(|x| (x + 1.0).abs() < 1e-4),
                "{w}×{h} grün"
            );
            assert!(
                v[2 * plane..].iter().all(|x| (x - 0.0039).abs() < 1e-2),
                "{w}×{h} blau"
            );
        }
    }

    #[test]
    fn durchsichtig_wird_weiss() {
        let v = pixel_values(&RgbaImage::filled(10, 10, [0, 0, 0, 0]));
        assert!(v.iter().all(|x| (x - 1.0).abs() < 1e-4));
    }

    #[test]
    fn linke_und_rechte_haelfte_bleiben_getrennt() {
        // Links schwarz, rechts weiss: nach dem Verkleinern links dunkel, rechts hell.
        let (w, h) = (1000u32, 10u32);
        let data = (0..w * h)
            .flat_map(|i| {
                if i % w < w / 2 {
                    [0, 0, 0, 255]
                } else {
                    [255, 255, 255, 255]
                }
            })
            .collect();
        let v = pixel_values(&RgbaImage::new(w, h, data).unwrap());
        assert!(v[0] < -0.99 && v[SIZE - 1] > 0.99);
        assert!(v[SIZE / 2 - 2] < -0.99 && v[SIZE / 2 + 1] > 0.99);
    }
}
