use std::collections::HashMap;

use nabshot_core::{
    ArrowHead, Color, Corner, LineStyle, Point, Size, Stroke, TextAlign, TextStyle, VerticalAlign,
};

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
        stroke: Stroke::new(Color::BLACK, 0.0),
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
        stroke: Stroke::new(Color::BLACK, 0.0),
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
        stroke: Stroke::new(Color::BLACK, 0.0),
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
        stroke: Stroke::new(Color::BLACK, 4.0),
        head: ArrowHead::Filled,
        head_size: 1.0,
        control: None,
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
        ..Default::default()
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
        fixed_width: false,
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
        ..Default::default()
    };
    let m = r.measure_text("🎉", &style, None);
    assert!(m.width > 16.0, "Emoji ohne Breite: {m:?}");

    let mut doc = Document::from_capture(80, 60);
    doc.push_layer(LayerKind::Text {
        bounds: Rect::new(5.0, 5.0, 70.0, 50.0),
        text: "🎉".into(),
        style,
        fixed_width: false,
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
fn gedrehter_text_steht_senkrecht() {
    let mut r = Renderer::new();
    let mut doc = Document::from_capture(200, 200);
    let id = doc.push_layer(LayerKind::Text {
        bounds: Rect::new(20.0, 85.0, 160.0, 30.0),
        text: "MMMMMMMM".into(),
        style: TextStyle {
            size: 24.0,
            color: Color::WHITE,
            bold: true,
            background: None,
            ..Default::default()
        },
        fixed_width: false,
    });
    let ink = |img: &RgbaImage, xs: std::ops::Range<u32>, ys: std::ops::Range<u32>| {
        xs.flat_map(|x| ys.clone().map(move |y| (x, y)))
            .filter(|&(x, y)| img.pixel(x, y) != Some(BLUE))
            .count()
    };
    let flat = r.render(&doc, &assets(200, 200)).unwrap();
    // Ungedreht: Tinte links und rechts, nicht oben.
    assert!(ink(&flat, 20..60, 85..115) > 50);
    assert_eq!(ink(&flat, 85..115, 0..60), 0);

    doc.layers.iter_mut().find(|l| l.id == id).unwrap().rotation = 90.0;
    let turned = r.render(&doc, &assets(200, 200)).unwrap();
    // Um 90° gedreht: Tinte oben in der Mitte, links nicht mehr.
    assert!(ink(&turned, 85..115, 20..60) > 50);
    assert_eq!(ink(&turned, 0..60, 85..115), 0);
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
        corner_radius: 0.0,
        feather: 0.0,
    });
    let out = Renderer::new().render(&doc, &assets(20, 20)).unwrap();
    assert_eq!(out.pixel(10, 10), Some(BLUE));
    assert!(out.pixel(1, 1).unwrap()[2] < 160);
}

/// Waagrechte Linie in der Mitte; liefert, wie viele Pixel der Linie
/// (y = 20, x = 10..90) den blauen Grund zeigen.
fn luecken_in_linie(style: LineStyle) -> usize {
    let mut doc = Document::from_capture(100, 40);
    doc.push_layer(LayerKind::Line {
        from: Point::new(10.0, 20.0),
        to: Point::new(90.0, 20.0),
        stroke: Stroke {
            style,
            ..Stroke::new(Color::BLACK, 2.0)
        },
    });
    let out = Renderer::new().render(&doc, &assets(100, 40)).unwrap();
    (10..90).filter(|&x| out.pixel(x, 20) == Some(BLUE)).count()
}

#[test]
fn durchgezogen_gestrichelt_gepunktet() {
    assert_eq!(luecken_in_linie(LineStyle::Solid), 0);
    assert!(luecken_in_linie(LineStyle::Dashed) > 10);
    assert!(luecken_in_linie(LineStyle::Dotted) > 10);
}

#[test]
fn doppellinie_ist_in_der_mitte_frei() {
    let mut doc = Document::from_capture(100, 40);
    doc.push_layer(LayerKind::Line {
        from: Point::new(10.0, 20.0),
        to: Point::new(90.0, 20.0),
        stroke: Stroke {
            style: LineStyle::Double,
            ..Stroke::new(Color::BLACK, 4.0)
        },
    });
    let out = Renderer::new().render(&doc, &assets(100, 40)).unwrap();
    // Gesamt 12 px breit (y 14..26), Mitte 4 px frei (y 18..22).
    assert_eq!(out.pixel(50, 15), Some([0, 0, 0, 255]));
    assert_eq!(out.pixel(50, 20), Some(BLUE));
    assert_eq!(out.pixel(50, 24), Some([0, 0, 0, 255]));
}

fn pfeil_mit_spitze(head: ArrowHead) -> RgbaImage {
    let mut doc = Document::from_capture(100, 40);
    doc.push_layer(LayerKind::Arrow {
        from: Point::new(10.0, 20.0),
        to: Point::new(90.0, 20.0),
        stroke: Stroke::new(Color::BLACK, 2.0),
        head,
        // Spitze 20 px lang, 12 px halbe Breite.
        head_size: 2.0,
        control: None,
    });
    Renderer::new().render(&doc, &assets(100, 40)).unwrap()
}

#[test]
fn offene_spitze_ist_innen_frei() {
    assert_eq!(
        pfeil_mit_spitze(ArrowHead::Filled).pixel(80, 23),
        Some([0, 0, 0, 255])
    );
    let open = pfeil_mit_spitze(ArrowHead::Open);
    assert_eq!(open.pixel(80, 23), Some(BLUE));
    assert_eq!(open.pixel(89, 20), Some([0, 0, 0, 255]));
}

fn spotlight(corner_radius: f32, feather: f32) -> RgbaImage {
    let mut doc = Document::from_capture(80, 80);
    doc.push_layer(LayerKind::Spotlight {
        bounds: Rect::new(20.0, 20.0, 40.0, 40.0),
        dim: 0.5,
        corner_radius,
        feather,
    });
    Renderer::new().render(&doc, &assets(80, 80)).unwrap()
}

#[test]
fn spotlight_mit_runden_ecken() {
    assert_eq!(spotlight(0.0, 0.0).pixel(20, 20), Some(BLUE));
    assert!(spotlight(10.0, 0.0).pixel(20, 20).unwrap()[2] < 160);
    assert_eq!(spotlight(10.0, 0.0).pixel(40, 40), Some(BLUE));
}

#[test]
fn spotlight_mit_weichem_rand() {
    let hart = spotlight(0.0, 0.0);
    let weich = spotlight(0.0, 4.0);
    // Innen am Rand: hart hell, weich schon etwas dunkler.
    assert_eq!(hart.pixel(21, 40), Some(BLUE));
    assert!(weich.pixel(21, 40).unwrap()[2] < 250);
    // Aussen am Rand: weich heller als hart.
    assert!(weich.pixel(18, 40).unwrap()[2] > hart.pixel(18, 40).unwrap()[2]);
    // Mitte und weit aussen unverändert.
    assert_eq!(weich.pixel(40, 40), Some(BLUE));
    assert_eq!(weich.pixel(1, 1), hart.pixel(1, 1));
}

#[test]
fn lupe_mit_griff() {
    let lupe = |handle| {
        let mut doc = Document::from_capture(100, 100);
        doc.push_layer(LayerKind::Magnifier {
            bounds: Rect::new(10.0, 10.0, 40.0, 40.0),
            source: Rect::new(20.0, 20.0, 20.0, 20.0),
            stroke: Stroke::new(Color::BLACK, 2.0),
            handle,
        });
        Renderer::new().render(&doc, &assets(100, 100)).unwrap()
    };
    // Mitte (30, 30), Radius 20: Griff nach rechts unten bis ~(55, 55).
    assert_eq!(lupe(None).pixel(50, 50), Some(BLUE));
    assert_eq!(
        lupe(Some(Corner::BottomRight)).pixel(50, 50),
        Some([0, 0, 0, 255])
    );
    assert_eq!(lupe(Some(Corner::TopLeft)).pixel(50, 50), Some(BLUE));
}

/// Erster dunkler Pixel (x, y) eines Textes in einem 200×100-Kasten.
fn text_lage(align: TextAlign, valign: VerticalAlign) -> (u32, u32) {
    let mut doc = Document::from_capture(200, 100);
    doc.push_layer(LayerKind::Text {
        bounds: Rect::new(0.0, 0.0, 200.0, 100.0),
        text: "Hi".into(),
        style: TextStyle {
            size: 20.0,
            color: Color::BLACK,
            align,
            valign,
            ..Default::default()
        },
        fixed_width: true,
    });
    let out = Renderer::new().render(&doc, &assets(200, 100)).unwrap();
    let dark = |x, y| out.pixel(x, y).is_some_and(|p| p[2] < 100);
    let x = (0..200).find(|&x| (0..100).any(|y| dark(x, y))).unwrap();
    let y = (0..100).find(|&y| (0..200).any(|x| dark(x, y))).unwrap();
    (x, y)
}

#[test]
fn text_ausrichten() {
    let (links, oben) = text_lage(TextAlign::Left, VerticalAlign::Top);
    let (mitte, mittig) = text_lage(TextAlign::Center, VerticalAlign::Middle);
    let (rechts, unten) = text_lage(TextAlign::Right, VerticalAlign::Bottom);
    assert!(links < 10, "{links}");
    assert!(mitte > 80 && mitte < 100, "{mitte}");
    assert!(rechts > 160, "{rechts}");
    assert!(oben < 15, "{oben}");
    assert!(mittig > 30 && mittig < 50, "{mittig}");
    assert!(unten > 70, "{unten}");
}

#[test]
fn gebogener_pfeil() {
    let pfeil = |control| {
        let mut doc = Document::from_capture(100, 60);
        doc.push_layer(LayerKind::Arrow {
            from: Point::new(10.0, 50.0),
            to: Point::new(90.0, 50.0),
            stroke: Stroke::new(Color::BLACK, 3.0),
            head: ArrowHead::Filled,
            head_size: 1.0,
            control,
        });
        Renderer::new().render(&doc, &assets(100, 60)).unwrap()
    };
    // Gerade: Mitte auf y = 50. Gebogen über (50, 10): Scheitel bei y = 30.
    assert_eq!(pfeil(None).pixel(50, 50), Some([0, 0, 0, 255]));
    let bogen = pfeil(Some(Point::new(50.0, 10.0)));
    assert_eq!(bogen.pixel(50, 50), Some(BLUE));
    assert_eq!(bogen.pixel(50, 30), Some([0, 0, 0, 255]));
    // Umschliessendes Rechteck reicht bis zum Scheitel, nicht bis zum Kontrollpunkt.
    let k = LayerKind::Arrow {
        from: Point::new(10.0, 50.0),
        to: Point::new(90.0, 50.0),
        stroke: Stroke::default(),
        head: ArrowHead::Filled,
        head_size: 1.0,
        control: Some(Point::new(50.0, 10.0)),
    };
    assert_eq!(k.bounds(), Rect::new(10.0, 30.0, 80.0, 20.0));
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
