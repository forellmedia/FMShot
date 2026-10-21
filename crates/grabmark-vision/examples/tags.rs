//! Stichwörter für Bilder mit dem echten Modell – zum Eichen der Schwellen:
//! `cargo run -p grabmark-vision --release --features onnx --example tags -- BILD …`
//! Modellordner wie im Test: `GRABMARK_MODEL_DIR`, sonst `models/siglip2`.

use std::path::PathBuf;
use std::time::Instant;

use grabmark_vision::{ImageEncoder, Tagger, TextEncoder};

fn main() {
    let dir = std::env::var_os("GRABMARK_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models/siglip2"));
    let images = ImageEncoder::load(&dir, 2).expect("Bildmodell");
    let tagger = Tagger::build(&TextEncoder::load(&dir, 4).expect("Textmodell")).expect("Begriffe");
    for path in std::env::args().skip(1) {
        let Ok(bytes) = std::fs::read(&path) else {
            eprintln!("{path}: nicht lesbar");
            continue;
        };
        let image = grabmark_render::decode_image(&bytes).expect("Bild");
        let start = Instant::now();
        let vectors = images.embed_all(&image).expect("Vektoren");
        let tags: Vec<String> = tagger
            .tags(&vectors)
            .iter()
            .map(|(key, l)| format!("{key} {l:.1}"))
            .collect();
        println!(
            "{path}: {} ({} Ausschnitte, {:?})",
            tags.join(", "),
            vectors.parts.len(),
            start.elapsed()
        );
    }
}
