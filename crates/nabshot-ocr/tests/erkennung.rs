//! Echte Texterkennung: Text mit dem eigenen Renderer zeichnen und lesen.

use nabshot_core::{Color, Document, LayerKind, Rect, RgbaImage, TextStyle};
use std::collections::HashMap;

fn bild_mit_text(text: &str) -> RgbaImage {
    let mut doc = Document::from_capture(900, 120);
    doc.push_layer(LayerKind::Text {
        bounds: Rect::new(20.0, 20.0, 860.0, 80.0),
        text: text.into(),
        style: TextStyle {
            size: 48.0,
            color: Color::BLACK,
            bold: false,
            background: None,
            ..Default::default()
        },
        fixed_width: false,
    });
    let assets = HashMap::from([(
        nabshot_core::AssetRef::Original,
        RgbaImage::filled(900, 120, [255, 255, 255, 255]),
    )]);
    nabshot_render::Renderer::new()
        .render(&doc, &assets)
        .unwrap()
}

#[test]
#[cfg_attr(
    not(target_os = "macos"),
    ignore = "Texterkennung nur auf macOS umgesetzt"
)]
fn erkennt_deutschen_text() {
    let engine = nabshot_ocr::platform_engine().expect("Texterkennung verfügbar");
    let blocks = engine
        .recognize(&bild_mit_text("Fehler 404: Datei nicht gefunden"))
        .unwrap();
    let text = nabshot_ocr::join_text(&blocks);
    assert!(text.contains("Datei nicht gefunden"), "erkannt: {text:?}");
    let b = blocks[0].bounds;
    assert!(b.x < 60.0 && b.y < 60.0 && b.width > 400.0, "{b:?}");
}
