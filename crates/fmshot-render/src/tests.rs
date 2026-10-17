use std::collections::HashMap;

use fmshot_core::{Color, Point, Size, Stroke, TextStyle};

use super::*;

const BLUE: [u8; 4] = [0, 0, 255, 255];
const WHITE: [u8; 4] = [255, 255, 255, 255];

fn assets(w: u32, h: u32) -> HashMap<AssetRef, RgbaImage> {
    HashMap::from([(AssetRef::Original, RgbaImage::filled(w, h, BLUE))])
}

/// Bild mit vertikalen Streifen (abwechselnd schwarz/weiss, 1 px breit).
fn stripes(w: u32, h: u32) -> RgbaImage {
    let mut data = Vec::with_capacity((w * h * 4) as usize);
    for _y in 0..h {
        for x in 0..w {
            data.extend_from_slice(if x % 2 == 0 { &[0, 0, 0, 255] } else { &WHITE });
        }
    }
    RgbaImage::new(w, h, data).unwrap()
}

#[test]
fn screenshot_ohne_ebenen_bleibt_pixelgleich() {
    let doc = Document::from_capture(40, 30);
    let a = assets(40, 30);
    let out = Renderer::new().render(&doc, &a).unwrap();
    assert_eq!(&out, a.get(&AssetRef::Original).unwrap());
}

#[test]
fn gefuelltes_rechteck_landet_an_richtiger_stelle() {
    let mut doc = Document::from_capture(40, 30);
    doc.push_layer(LayerKind::Rectangle {
        bounds: Rect::new(10.0, 10.0, 10.0, 10.0),
        stroke: Stroke {
            color: Color::BLACK,
            width: 0.0,
        },
        fill: Some(Color::WHITE),
        corner_radius: 0.0,
    });
    let out = Renderer::new().render(&doc, &assets(40, 30)).unwrap();
    assert_eq!(out.pixel(15, 15), Some(WHITE));
    assert_eq!(out.pixel(5, 5), Some(BLUE));
}

#[test]
fn verschobene_zeichenflaeche_mit_hintergrund() {
    let mut doc = Document::from_capture(40, 30);
    doc.artboard.rect = Rect::new(-10.0, 0.0, 50.0, 30.0);
    doc.artboard.background = Some(Fill::Solid {
        color: Color::WHITE,
    });
    let out = Renderer::new().render(&doc, &assets(40, 30)).unwrap();
    assert_eq!(out.width(), 50);
    assert_eq!(out.pixel(5, 5), Some(WHITE));
    assert_eq!(out.pixel(15, 5), Some(BLUE));
}

#[test]
fn ausschnitt_und_massstab() {
    let mut doc = Document::from_capture(100, 100);
    doc.push_layer(LayerKind::Rectangle {
        bounds: Rect::new(50.0, 50.0, 50.0, 50.0),
        stroke: Stroke {
            color: Color::BLACK,
            width: 0.0,
        },
        fill: Some(Color::WHITE),
        corner_radius: 0.0,
    });
    let opts = RenderOptions {
        region: Rect::new(40.0, 40.0, 40.0, 40.0),
        scale: 0.5,
        ..RenderOptions::full(&doc)
    };
    let out = Renderer::new()
        .render_with(&doc, &assets(100, 100), &opts)
        .unwrap();
    assert_eq!((out.width(), out.height()), (20, 20));
    assert_eq!(out.pixel(2, 2), Some(BLUE)); // Dokument (44, 44)
    assert_eq!(out.pixel(15, 15), Some(WHITE)); // Dokument (70, 70)
}

#[test]
fn nur_einzelne_ebenen_auf_durchsichtigem_grund() {
    let mut doc = Document::from_capture(20, 20);
    doc.push_layer(LayerKind::Rectangle {
        bounds: Rect::new(0.0, 0.0, 5.0, 5.0),
        stroke: Stroke {
            color: Color::BLACK,
            width: 0.0,
        },
        fill: Some(Color::WHITE),
        corner_radius: 0.0,
    });
    let opts = RenderOptions {
        layers: 1..2,
        ..RenderOptions::full(&doc)
    };
    let out = Renderer::new()
        .render_with(&doc, &assets(20, 20), &opts)
        .unwrap();
    assert_eq!(out.pixel(2, 2), Some(WHITE));
    assert_eq!(out.pixel(10, 10), Some([0, 0, 0, 0]));
}

#[test]
fn fehlendes_bild_ist_ein_fehler() {
    let mut doc = Document::from_capture(10, 10);
    doc.push_layer(LayerKind::Image {
        asset: AssetRef::Asset("fehlt".into()),
        bounds: Rect::new(0.0, 0.0, 5.0, 5.0),
        crop: None,
        source_size: Size::new(5.0, 5.0),
        corner_radius: 0.0,
        shadow: None,
    });
    let err = Renderer::new().render(&doc, &assets(10, 10)).unwrap_err();
    assert_eq!(
        err,
        RenderError::MissingAsset(AssetRef::Asset("fehlt".into()))
    );
}

#[test]
fn pfeil_zeichnet_spitze_am_ziel() {
    let mut doc = Document::from_capture(100, 40);
    doc.push_layer(LayerKind::Arrow {
        from: Point::new(10.0, 20.0),
        to: Point::new(90.0, 20.0),
        stroke: Stroke {
            color: Color::BLACK,
            width: 4.0,
        },
    });
    let out = Renderer::new().render(&doc, &assets(100, 40)).unwrap();
    assert_eq!(out.pixel(85, 20), Some([0, 0, 0, 255]));
    assert_eq!(out.pixel(85, 5), Some(BLUE));
}

#[test]
fn text_wird_gezeichnet_und_gemessen() {
    let mut r = Renderer::new();
    let style = TextStyle {
        size: 20.0,
        color: Color::WHITE,
        bold: false,
        background: None,
    };
    let one = r.measure_text("Hallo", &style, None);
    let two = r.measure_text("Hallo\nWelt", &style, None);
    assert!(one.width > 20.0 && one.width < 100.0, "{one:?}");
    assert!((two.height - 2.0 * one.height).abs() < 0.01);

    let mut doc = Document::from_capture(120, 40);
    doc.push_layer(LayerKind::Text {
        bounds: Rect::new(5.0, 5.0, 110.0, 30.0),
        text: "Hallo Ü".into(),
        style,
    });
    let out = r.render(&doc, &assets(120, 40)).unwrap();
    let changed = (0..120)
        .flat_map(|x| (0..40).map(move |y| (x, y)))
        .filter(|&(x, y)| out.pixel(x, y) != Some(BLUE))
        .count();
    assert!(changed > 50, "nur {changed} Pixel geändert");
}

#[test]
fn emoji_werden_farbig_gezeichnet() {
    let mut r = Renderer::new();
    let style = TextStyle {
        size: 32.0,
        color: Color::WHITE,
        bold: false,
        background: None,
    };
    let m = r.measure_text("🎉", &style, None);
    assert!(m.width > 16.0, "Emoji ohne Breite: {m:?}");

    let mut doc = Document::from_capture(80, 60);
    doc.push_layer(LayerKind::Text {
        bounds: Rect::new(5.0, 5.0, 70.0, 50.0),
        text: "🎉".into(),
        style,
    });
    let out = r.render(&doc, &assets(80, 60)).unwrap();
    // Weisser Text auf Blau ergibt nur Blau/Weiss-Mischungen (R == G);
    // farbige Emoji-Pixel weichen davon ab.
    let colored = (0..80)
        .flat_map(|x| (0..60).map(move |y| (x, y)))
        .filter_map(|(x, y)| out.pixel(x, y))
        .filter(|p| (p[0] as i32 - p[1] as i32).abs() > 40)
        .count();
    assert!(colored > 30, "nur {colored} farbige Pixel");
}

#[test]
fn verpixeln_macht_bloecke_einfarbig() {
    let mut doc = Document::from_capture(16, 16);
    doc.push_layer(LayerKind::Pixelate {
        bounds: Rect::new(0.0, 0.0, 16.0, 16.0),
        block: 4.0,
    });
    let a = HashMap::from([(AssetRef::Original, stripes(16, 16))]);
    let out = Renderer::new().render(&doc, &a).unwrap();
    let p = out.pixel(0, 0).unwrap();
    assert_eq!(out.pixel(3, 3), Some(p));
    assert!(p[0] > 100 && p[0] < 160, "Mittelwert erwartet: {p:?}");
}

#[test]
fn weichzeichnen_glaettet_streifen() {
    let mut doc = Document::from_capture(32, 8);
    doc.push_layer(LayerKind::Blur {
        bounds: Rect::new(0.0, 0.0, 32.0, 8.0),
        radius: 3.0,
    });
    let a = HashMap::from([(AssetRef::Original, stripes(32, 8))]);
    let out = Renderer::new().render(&doc, &a).unwrap();
    let p = out.pixel(16, 4).unwrap();
    assert!(p[0] > 90 && p[0] < 170, "{p:?}");
}

#[test]
fn spotlight_dunkelt_nur_aussen_ab() {
    let mut doc = Document::from_capture(20, 20);
    doc.push_layer(LayerKind::Spotlight {
        bounds: Rect::new(5.0, 5.0, 10.0, 10.0),
        dim: 0.5,
    });
    let out = Renderer::new().render(&doc, &assets(20, 20)).unwrap();
    assert_eq!(out.pixel(10, 10), Some(BLUE));
    assert!(out.pixel(1, 1).unwrap()[2] < 160);
}

#[test]
fn nummern_kreis() {
    let mut doc = Document::from_capture(40, 40);
    doc.push_layer(LayerKind::Step {
        center: Point::new(20.0, 20.0),
        radius: 15.0,
        number: 3,
        fill: Color::ACCENT,
        text_color: Color::WHITE,
    });
    let out = Renderer::new().render(&doc, &assets(40, 40)).unwrap();
    assert_ne!(out.pixel(8, 20), Some(BLUE));
    assert_eq!(out.pixel(1, 1), Some(BLUE));
}

#[test]
fn png_lesen() {
    let img = RgbaImage::filled(3, 2, [1, 2, 3, 255]);
    let mut bytes = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut bytes, 3, 2);
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()
            .unwrap()
            .write_image_data(img.data())
            .unwrap();
    }
    assert_eq!(decode_png(&bytes).unwrap(), img);
    assert!(decode_png(b"kein png").is_err());
}

#[test]
fn effekt_ueber_ebene_wird_erkannt() {
    let mut doc = Document::from_capture(10, 10);
    doc.push_layer(LayerKind::Blur {
        bounds: Rect::new(0.0, 0.0, 5.0, 5.0),
        radius: 2.0,
    });
    assert!(has_effect_above(&doc, 0));
    assert!(!has_effect_above(&doc, 1));
}
