//! Bildinhalt mit dem echten Modell. Braucht `scripts/fetch-model.sh`, darum
//! nur auf Abruf: `cargo test -p grabmark-vision --features onnx -- --ignored`.
//! Modellordner: `GRABMARK_MODEL_DIR`, sonst `models/siglip2` im Workspace.
#![cfg(feature = "onnx")]

use std::path::PathBuf;
use std::time::Instant;

use grabmark_core::RgbaImage;
use grabmark_vision::{Embedding, ImageEncoder, ImageVectors, TextEncoder};

const BILDER: [&str; 7] = ["wald", "mann", "hunde", "auto", "strand", "katze", "berg"];

fn model_dir() -> PathBuf {
    std::env::var_os("GRABMARK_MODEL_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../models/siglip2"))
}

fn bild(name: &str) -> RgbaImage {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(format!("tests/bilder/{name}.jpg"));
    grabmark_render::decode_image(&std::fs::read(path).unwrap()).unwrap()
}

fn bester(text: &Embedding, bilder: &[Embedding]) -> &'static str {
    let (i, _) = bilder
        .iter()
        .map(|b| text.similarity(b))
        .enumerate()
        .max_by(|a, b| a.1.total_cmp(&b.1))
        .unwrap();
    BILDER[i]
}

#[test]
#[ignore = "braucht das Modell (scripts/fetch-model.sh)"]
fn suchwoerter_finden_das_passende_foto() {
    let dir = model_dir();
    let images = ImageEncoder::load(&dir, 2).unwrap();
    let texts = TextEncoder::load(&dir, 2).unwrap();

    let fotos: Vec<RgbaImage> = BILDER.iter().map(|n| bild(n)).collect();
    images.embed(&fotos[0]).unwrap(); // aufwärmen
    let start = Instant::now();
    let vektoren: Vec<Embedding> = fotos.iter().map(|f| images.embed(f).unwrap()).collect();
    let je_bild = start.elapsed() / fotos.len() as u32;

    let erwartet = [
        ("Wald", "wald"),
        ("Bäume", "wald"),
        ("Mann", "mann"),
        ("Person", "mann"),
        ("Hund", "hunde"),
        ("Tier", "hunde"),
        ("Auto", "auto"),
        ("Fahrzeug", "auto"),
        ("Strand", "strand"),
        ("Meer", "strand"),
        ("Katze", "katze"),
        ("Berg", "berg"),
        ("forest", "wald"),
        ("a dog", "hunde"),
    ];
    let start = Instant::now();
    let mut falsch = Vec::new();
    for (wort, bild) in erwartet {
        let gefunden = bester(&texts.embed(wort).unwrap(), &vektoren);
        if gefunden != bild {
            falsch.push(format!("„{wort}“ → {gefunden} statt {bild}"));
        }
    }
    let je_text = start.elapsed() / erwartet.len() as u32;
    println!("Bild → Vektor: {je_bild:?}, Text → Vektor: {je_text:?}");
    assert!(falsch.is_empty(), "{falsch:?}");
}

#[test]
#[ignore = "braucht das Modell (scripts/fetch-model.sh)"]
fn langer_text_wird_gekuerzt_und_endet_mit_eos() {
    let texts = TextEncoder::load(&model_dir(), 1).unwrap();
    let ids = texts.token_ids(&"wald ".repeat(200)).unwrap();
    assert_eq!(ids.len(), 64);
    assert_eq!(ids[63], 1, "<eos>");
    let kurz = texts.token_ids("Wald").unwrap();
    assert_eq!(
        &kurz[..3],
        &[33346, 1, 0],
        "klein geschrieben, <eos>, <pad>"
    );
}

#[test]
#[ignore = "braucht das Modell (scripts/fetch-model.sh)"]
fn stichwoerter_und_suche_mit_grundwert() {
    let dir = model_dir();
    let images = ImageEncoder::load(&dir, 2).unwrap();
    let texts = TextEncoder::load(&dir, 2).unwrap();
    let start = Instant::now();
    let tagger = grabmark_vision::Tagger::build(&texts).unwrap();
    println!("Begriffe berechnen: {:?}", start.elapsed());

    let start = Instant::now();
    let alle_vektoren: Vec<ImageVectors> = BILDER
        .iter()
        .map(|n| images.embed_all(&bild(n)).unwrap())
        .collect();
    println!(
        "Bild mit Ausschnitten → Vektoren: {:?} je Bild",
        start.elapsed() / BILDER.len() as u32
    );
    let erwartet = [
        ("wald", "forest"),
        ("mann", "man"),
        ("hunde", "dog"),
        ("auto", "car"),
        ("strand", "beach"),
        ("katze", "cat"),
        ("berg", "mountain"),
    ];
    for ((name, key), v) in erwartet.iter().zip(&alle_vektoren) {
        let tags: Vec<_> = tagger.tags(v).into_iter().map(|t| t.0).collect();
        println!("{name}: {tags:?}");
        assert!(tags.contains(key), "{name}: {tags:?}");
    }

    let vektoren: Vec<&Embedding> = alle_vektoren.iter().map(|v| &v.whole).collect();
    let alle: Vec<(&str, &Embedding)> = BILDER.iter().copied().zip(vektoren).collect();
    let suche = |q: &str| -> Vec<&str> {
        tagger
            .search(&texts.embed(q).unwrap(), alle.iter().copied())
            .into_iter()
            .map(|h| h.0)
            .collect()
    };
    assert_eq!(suche("Hund"), vec!["hunde"]);
    assert_eq!(suche("Katze"), vec!["katze"]);
    assert_eq!(suche("Wald")[0], "wald");
    assert!(suche("Elefant").is_empty());
    assert!(suche("Hochzeit").is_empty());
}
