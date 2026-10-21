//! Ausschnitte eines Bildes für die Stichwörter. Das Modell sieht nur
//! 224×224 Pixel: Auf einem ganzen Bildschirmfoto gehen Personen oder Dinge
//! neben viel Text unter („Webshop“ statt „Mann, Schuh“). In quadratischen
//! Ausschnitten erkennt es sie.

use grabmark_core::RgbaImage;

/// Kleinere Ausschnitte bringen nichts – das Modell sieht ohnehin 224 Pixel.
const MIN_SIDE: u32 = 200;
/// Höchstens so viele Quadrate über die lange Seite (lange Scroll-Aufnahmen).
const MAX_ROW: u32 = 4;
/// Höchstens so viele Felder im feinen Raster.
const MAX_GRID: u32 = 10;

/// Quadratische Ausschnitte ohne das ganze Bild: eine Reihe mit der kurzen
/// Seite als Kante, dazu ein Raster mit halber Kante. Bei 16:9 sind das
/// 2 + 8 Ausschnitte, bei einem quadratischen Bild 4.
pub fn parts(image: &RgbaImage) -> Vec<RgbaImage> {
    let (w, h) = (image.width(), image.height());
    let side = w.min(h);
    let mut out = Vec::new();
    for level in [1, 2] {
        let s = side / level;
        if s < MIN_SIDE {
            break;
        }
        let nx = w.div_ceil(s);
        let ny = h.div_ceil(s);
        let (nx, ny) = match level {
            // Ein Quadrat über alles wäre das ganze Bild.
            1 if nx * ny == 1 => continue,
            1 => (nx.min(MAX_ROW), ny.min(MAX_ROW)),
            _ if nx * ny > MAX_GRID => continue,
            _ => (nx, ny),
        };
        for iy in 0..ny {
            for ix in 0..nx {
                // Gleichmässig verteilt, das letzte Feld schliesst bündig ab.
                let x = if nx > 1 { (w - s) * ix / (nx - 1) } else { 0 };
                let y = if ny > 1 { (h - s) * iy / (ny - 1) } else { 0 };
                out.extend(image.crop(x, y, s, s));
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn image(w: u32, h: u32) -> RgbaImage {
        RgbaImage::filled(w, h, [0; 4])
    }

    #[test]
    fn breitbild_hat_reihe_und_raster() {
        let p = parts(&image(1920, 1080));
        assert_eq!(p.len(), 2 + 8);
        assert!(
            p[..2]
                .iter()
                .all(|i| i.width() == 1080 && i.height() == 1080)
        );
        assert!(p[2..].iter().all(|i| i.width() == 540));
    }

    #[test]
    fn quadrat_nur_raster_und_kleines_bild_gar_nicht() {
        assert_eq!(parts(&image(800, 800)).len(), 4);
        assert!(parts(&image(300, 300)).is_empty());
        assert!(parts(&image(1200, 150)).is_empty());
    }

    #[test]
    fn lange_scroll_aufnahme_bleibt_begrenzt() {
        let p = parts(&image(500, 6000));
        assert_eq!(p.len(), MAX_ROW as usize);
        assert!(p.iter().all(|i| i.width() == 500 && i.height() == 500));
    }
}
