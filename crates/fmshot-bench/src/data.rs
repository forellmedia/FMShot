//! Künstliche, aber realistische Testdaten: Bildschirmfotos mit „Text“,
//! Anmerkungen wie im Alltag, Scroll-Bilder und ein gefüllter Index.

use std::collections::HashMap;

use fmshot_core::{
    AssetRef, Color, Document, LayerKind, Point, Rect, RgbaImage, Stroke, TextStyle,
};
use fmshot_library::{Library, NewCapture};

/// Einfacher Pseudo-Zufall (reproduzierbar, ohne Abhängigkeit).
struct Lcg(u64);

impl Lcg {
    fn next(&mut self) -> u32 {
        self.0 = self
            .0
            .wrapping_mul(6_364_136_223_846_793_005)
            .wrapping_add(1);
        (self.0 >> 33) as u32
    }
}

/// Heller Hintergrund mit Zeilen aus „Wörtern“ – komprimiert ähnlich wie
/// ein echtes Fenster mit Text (reine Flächen wären unrealistisch schnell).
pub fn screenshot(width: u32, height: u32) -> RgbaImage {
    page(width, height, 0)
}

fn page(width: u32, height: u32, offset: u32) -> RgbaImage {
    let mut data = vec![0u8; (width * height * 4) as usize];
    for y in 0..height {
        let line = (y + offset) / 36;
        let in_line = (y + offset) % 36;
        let mut rng = Lcg(u64::from(line) * 7919 + 17);
        // Wortgrenzen dieser Zeile
        let mut words = Vec::new();
        let mut x = 40 + rng.next() % 40;
        while x < width.saturating_sub(80) {
            let len = 20 + rng.next() % 120;
            words.push((x, x + len));
            x += len + 12 + rng.next() % 10;
        }
        for xx in 0..width {
            let i = ((y * width + xx) * 4) as usize;
            let ink = (8..26).contains(&in_line)
                && words.iter().any(|&(a, b)| xx >= a && xx < b)
                && !(xx * 7 + y * 3 + line).is_multiple_of(5);
            let v = if ink { 40 } else { 250 - (line % 3) as u8 * 4 };
            data[i..i + 4].copy_from_slice(&[v, v, v.saturating_add(3), 255]);
        }
    }
    RgbaImage::new(width, height, data).expect("Grösse passt")
}

/// Anmerkungen wie in einer typischen Anleitung.
pub fn annotate(doc: &mut Document) {
    let red = Color::rgb(0xe5, 0x39, 0x35);
    let stroke = Stroke {
        color: red,
        width: 6.0,
    };
    for i in 0..4 {
        let y = 200.0 + i as f32 * 380.0;
        doc.push_layer(LayerKind::Rectangle {
            bounds: Rect::new(180.0, y, 900.0, 160.0),
            stroke,
            fill: None,
            corner_radius: 8.0,
        });
        doc.push_layer(LayerKind::Arrow {
            from: Point::new(1600.0, y + 300.0),
            to: Point::new(1100.0, y + 80.0),
            stroke,
        });
        doc.push_layer(LayerKind::Step {
            center: Point::new(150.0, y),
            radius: 28.0,
            number: i + 1,
            fill: red,
            text_color: Color::WHITE,
        });
    }
    doc.push_layer(LayerKind::Text {
        bounds: Rect::new(1700.0, 300.0, 900.0, 200.0),
        text: "Hier klicken, dann „Speichern“ wählen 👍".into(),
        style: TextStyle {
            size: 44.0,
            color: red,
            bold: true,
            background: Some(Color::WHITE),
        },
    });
    doc.push_layer(LayerKind::Highlight {
        bounds: Rect::new(200.0, 1500.0, 1400.0, 60.0),
        color: Color {
            r: 255,
            g: 235,
            b: 59,
            a: 110,
        },
    });
    doc.push_layer(LayerKind::Pixelate {
        bounds: Rect::new(2000.0, 1200.0, 700.0, 120.0),
        block: 16.0,
    });
    doc.push_layer(LayerKind::Blur {
        bounds: Rect::new(2000.0, 1450.0, 700.0, 120.0),
        radius: 16.0,
    });
    // Zuletzt eine Form, die „gezogen“ wird.
    doc.push_layer(LayerKind::Ellipse {
        bounds: Rect::new(1300.0, 900.0, 500.0, 300.0),
        stroke,
        fill: None,
    });
}

pub fn assets(original: RgbaImage) -> HashMap<AssetRef, RgbaImage> {
    HashMap::from([(AssetRef::Original, original)])
}

/// `count` Bilder einer scrollenden Seite, je `step` Pixel weiter.
pub fn scroll_frames(width: u32, height: u32, count: u32, step: u32) -> Vec<RgbaImage> {
    (0..count).map(|i| page(width, height, i * step)).collect()
}

/// Index mit vielen kleinen Aufnahmen und erkanntem Text füllen.
pub fn fill_index(library: &Library, count: u32) {
    const WORDS: [&str; 12] = [
        "Rechnung",
        "Bestellung",
        "Einstellungen",
        "Fehler",
        "Konto",
        "Passwort",
        "Übersicht",
        "Bericht",
        "Termin",
        "Projekt",
        "Vertrag",
        "Zahlung",
    ];
    let small = RgbaImage::filled(64, 40, [240, 240, 240, 255]);
    let mut rng = Lcg(42);
    for i in 0..count {
        let rec = library
            .save_capture(&NewCapture {
                image: small.clone(),
                scale_factor: 1.0,
                mode: "region".into(),
                created_at_ms: 1_700_000_000_000 + i64::from(i) * 1000,
            })
            .expect("speichern");
        let text: Vec<&str> = (0..40)
            .map(|_| WORDS[rng.next() as usize % WORDS.len()])
            .collect();
        library
            .set_ocr_text(&rec.id, &text.join(" "))
            .expect("Text speichern");
    }
}
