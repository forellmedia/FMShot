//! Zeichnet App- und Menüleisten-Symbol mit dem eigenen Renderer.
//!
//! cargo run -p nabshot-render --example icons -- <ausgabeordner>
//! Danach: `npx tauri icon <ordner>/app-icon.png` im App-Ordner.

use std::collections::HashMap;

use nabshot_core::{AssetRef, Color, Document, Fill, LayerKind, Point, Rect, RgbaImage, Stroke};
use nabshot_render::Renderer;

fn main() {
    let out = std::env::args().nth(1).unwrap_or_else(|| ".".into());
    let mut r = Renderer::new();
    let none: HashMap<AssetRef, RgbaImage> = HashMap::new();

    // App-Symbol 1024×1024: abgerundetes Quadrat mit Verlauf, weisse Sucher-Ecken,
    // roter Auslöse-Punkt.
    // Verlauf zuerst als eigenes Bild zeichnen, dann als Bild-Ebene mit
    // runden Ecken und Schatten einsetzen.
    let mut doc = empty(1024.0);
    let mut bg = empty(1024.0);
    bg.artboard.background = Some(Fill::LinearGradient {
        from: Color::rgb(0x2b, 0x2d, 0x42),
        to: Color::rgb(0x8d, 0x2b, 0x4a),
        angle: 135.0,
    });
    let gradient = r.render(&bg, &none).unwrap();
    let assets = HashMap::from([(AssetRef::Original, gradient)]);
    doc.layers.push(layer(
        &doc,
        LayerKind::Image {
            asset: AssetRef::Original,
            bounds: Rect::new(80.0, 80.0, 864.0, 864.0),
            crop: Some(Rect::new(80.0, 80.0, 864.0, 864.0)),
            source_size: nabshot_core::Size::new(1024.0, 1024.0),
            corner_radius: 190.0,
            shadow: Some(nabshot_core::Shadow {
                color: Color::rgba(0, 0, 0, 90),
                blur: 24.0,
                offset_y: 12.0,
            }),
        },
    ));
    corners(
        &mut doc,
        Rect::new(250.0, 250.0, 524.0, 524.0),
        120.0,
        46.0,
        Color::WHITE,
    );
    doc.layers.push(layer(
        &doc,
        LayerKind::Ellipse {
            bounds: Rect::new(452.0, 452.0, 120.0, 120.0),
            stroke: Stroke::new(Color::WHITE, 0.0),
            fill: Some(Color::rgb(0xff, 0x4d, 0x4d)),
        },
    ));
    save(
        &r.render(&doc, &assets).unwrap(),
        &format!("{out}/app-icon.png"),
    );

    // Menüleiste (Vorlage, schwarz auf durchsichtig): 36×36 = 18 pt @2x.
    let mut tray = empty(36.0);
    corners(
        &mut tray,
        Rect::new(5.0, 5.0, 26.0, 26.0),
        8.0,
        3.2,
        Color::BLACK,
    );
    tray.layers.push(layer(
        &tray,
        LayerKind::Ellipse {
            bounds: Rect::new(14.0, 14.0, 8.0, 8.0),
            stroke: Stroke::new(Color::BLACK, 0.0),
            fill: Some(Color::BLACK),
        },
    ));
    save(&r.render(&tray, &none).unwrap(), &format!("{out}/tray.png"));
    println!("Symbole geschrieben nach {out}");
}

fn empty(size: f32) -> Document {
    let mut d = Document::from_capture(size as u32, size as u32);
    d.layers.clear();
    d
}

fn layer(doc: &Document, kind: LayerKind) -> nabshot_core::Layer {
    nabshot_core::Layer {
        id: doc.next_layer_id(),
        name: None,
        visible: true,
        locked: false,
        opacity: 1.0,
        rotation: 0.0,
        group: None,
        kind,
    }
}

/// Vier Sucher-Ecken (wie das Werkzeug „Bereich“).
fn corners(doc: &mut Document, r: Rect, len: f32, width: f32, color: Color) {
    let stroke = Stroke::new(color, width);
    let (x0, y0, x1, y1) = (r.x, r.y, r.right(), r.bottom());
    let pts = [
        [(x0, y0 + len), (x0, y0), (x0 + len, y0)],
        [(x1 - len, y0), (x1, y0), (x1, y0 + len)],
        [(x1, y1 - len), (x1, y1), (x1 - len, y1)],
        [(x0 + len, y1), (x0, y1), (x0, y1 - len)],
    ];
    for c in pts {
        let kind = LayerKind::Pen {
            points: c.iter().map(|&(x, y)| Point::new(x, y)).collect(),
            stroke,
        };
        let l = layer(doc, kind);
        doc.layers.push(l);
    }
}

fn save(img: &RgbaImage, path: &str) {
    let png = encode(img);
    std::fs::write(path, png).unwrap();
}

fn encode(img: &RgbaImage) -> Vec<u8> {
    let mut out = Vec::new();
    {
        let mut enc = png::Encoder::new(&mut out, img.width(), img.height());
        enc.set_color(png::ColorType::Rgba);
        enc.set_depth(png::BitDepth::Eight);
        enc.write_header()
            .unwrap()
            .write_image_data(img.data())
            .unwrap();
    }
    out
}
