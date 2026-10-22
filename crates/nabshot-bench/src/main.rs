//! Misst die zeitkritischen Pfade ohne Bildschirm und vergleicht sie mit
//! Budgets (docs/produkt.md, „Geschwindigkeit“). Läuft in der CI; ein Wert
//! über Budget lässt den Lauf fehlschlagen.
//!
//! cargo run --profile bench -p nabshot-bench            # messen + prüfen
//! cargo run --profile bench -p nabshot-bench -- --no-fail
//!
//! Wo es ein Produktziel gibt (Vorschau < 150 ms, Editor < 200 ms), ist es
//! das Budget – auch auf den langsameren CI-Runnern. Die übrigen Budgets
//! liegen etwa beim Dreifachen eines aktuellen Macs: Die Messung soll
//! deutliche Rückschritte finden, nicht Rauschen.

mod data;

use std::fmt::Write as _;
use std::time::{Duration, Instant};

use nabshot_core::Document;
use nabshot_library::{Library, NewCapture, thumbnail};
use nabshot_render::{RenderOptions, Renderer, decode_png};
use nabshot_stitch::Stitcher;

/// Ein Messpunkt: Name, Budget, Ergebnis (Median).
struct Measurement {
    name: &'static str,
    budget: Duration,
    median: Duration,
    min: Duration,
}

/// Führt `f` mehrfach aus (erster Lauf = Aufwärmen) und liefert Median und
/// Minimum. `setup` läuft vor jedem Durchgang und wird nicht mitgemessen.
fn measure<S, T>(
    runs: usize,
    mut setup: impl FnMut() -> S,
    mut f: impl FnMut(S) -> T,
) -> (Duration, Duration) {
    std::hint::black_box(f(setup()));
    let mut times: Vec<Duration> = (0..runs)
        .map(|_| {
            let input = setup();
            let start = Instant::now();
            std::hint::black_box(f(input));
            start.elapsed()
        })
        .collect();
    times.sort();
    (times[times.len() / 2], times[0])
}

/// Ein Bild 1080p mit Webcam (720p) auf der Grafikkarte zusammensetzen.
fn video_compose() -> Result<(Duration, Duration), String> {
    use nabshot_core::{VideoDoc, WebcamLayout, WebcamMode, WebcamSection, WebcamTrack};
    use nabshot_video::{Compositor, FrameRef, Source, scene_at};

    let (w, h) = (1920, 1080);
    let mut compositor = Compositor::new(w, h).map_err(|e| e.to_string())?;
    if compositor.is_software() {
        return Err(format!("nur Software-Grafik ({})", compositor.adapter()));
    }
    let screen = vec![128u8; w as usize * h as usize * 4];
    let webcam = vec![200u8; 1280 * 720 * 4];
    let doc = VideoDoc {
        duration_ms: 10_000,
        fps: 30,
        webcam: Some(WebcamTrack {
            layout: WebcamLayout::default(),
            sections: vec![WebcamSection {
                start_ms: 0,
                mode: WebcamMode::Overlay,
            }],
        }),
        ..VideoDoc::default()
    };
    let size = nabshot_core::Size::new(w as f32, h as f32);
    let scene = scene_at(
        &doc,
        0.0,
        size,
        size,
        Some(nabshot_core::Size::new(1280.0, 720.0)),
    );
    let mut failed = None;
    let times = measure(
        31,
        || (),
        |()| {
            compositor.upload(
                Source::Screen,
                FrameRef {
                    width: w,
                    height: h,
                    stride: w as usize * 4,
                    data: &screen,
                },
            );
            compositor.upload(
                Source::Webcam,
                FrameRef {
                    width: 1280,
                    height: 720,
                    stride: 1280 * 4,
                    data: &webcam,
                },
            );
            if let Err(e) = compositor.render(&scene, |data, _| data[0]) {
                failed = Some(e.to_string());
            }
        },
    );
    match failed {
        Some(e) => Err(e),
        None => Ok(times),
    }
}

fn ms(d: Duration) -> f64 {
    d.as_secs_f64() * 1000.0
}

fn main() {
    let no_fail = std::env::args().any(|a| a == "--no-fail");
    let mut results = Vec::new();
    let mut add = |name, budget_ms: u64, (median, min): (Duration, Duration)| {
        results.push(Measurement {
            name,
            budget: Duration::from_millis(budget_ms),
            median,
            min,
        });
    };

    // Retina-Vollbild eines 14-Zoll-MacBook Pro.
    let (w, h) = (3024, 1964);
    let shot = data::screenshot(w, h);
    let dir = tempfile::tempdir().expect("Temp-Ordner");
    let library = Library::open(dir.path()).expect("Bibliothek");
    let mut t = 1_760_000_000_000_i64;
    let mut next_capture = || {
        t += 1000;
        NewCapture {
            image: shot.clone(),
            scale_factor: 2.0,
            mode: "screen".into(),
            created_at_ms: t,
        }
    };

    // Aufnahme → Vorschau (Ziel < 150 ms): speichern (PNG, Dokument,
    // Vorschaubild, Index) plus das Bild für die schwebende Vorschau.
    add(
        "Aufnahme → Vorschau (speichern + Vorschaubild)",
        150,
        measure(9, &mut next_capture, |c| {
            library.save_capture(&c).expect("speichern");
            thumbnail(&c.image, 520)
        }),
    );

    // Editor öffnen (Ziel < 200 ms): Dokument + Original laden, dekodieren,
    // ersten Ausschnitt zeichnen.
    let rec = library.save_capture(&next_capture()).expect("speichern");
    let mut doc = library.load_document(&rec.id).expect("Dokument");
    data::annotate(&mut doc);
    library.save_document(&rec.id, &doc, None).ok();
    let mut renderer = Renderer::new();
    let viewport = |doc: &Document| RenderOptions {
        region: doc.artboard.rect,
        // Fenster 1400 Punkte breit auf Retina: ~0.9 Ausgabepixel je Dokumentpixel.
        scale: 2800.0 / doc.artboard.rect.width,
        interactive: true,
        ..RenderOptions::full(doc)
    };
    add(
        "Editor öffnen (laden + dekodieren + zeichnen)",
        200,
        measure(
            7,
            || (),
            |()| {
                let doc = library.load_document(&rec.id).expect("Dokument");
                let png = library.read_original(&rec.id).expect("Original");
                let original = decode_png(&png).expect("PNG");
                let assets = data::assets(original);
                renderer
                    .render_with(&doc, &assets, &viewport(&doc))
                    .expect("zeichnen")
            },
        ),
    );

    // Zeichnen im Editor: ganzer Ausschnitt (z. B. nach Zoom) und nur die
    // aktive Ebene beim Ziehen (Ziel: flüssig, < 16 ms).
    let assets = data::assets(shot.clone());
    add(
        "Editor: Ausschnitt zeichnen (alle Ebenen)",
        150,
        measure(
            15,
            || (),
            |()| {
                renderer
                    .render_with(&doc, &assets, &viewport(&doc))
                    .expect("zeichnen")
            },
        ),
    );
    let last = doc.layers.len() - 1;
    add(
        "Editor: aktive Ebene beim Ziehen",
        60,
        measure(
            31,
            || (),
            |()| {
                let opts = RenderOptions {
                    layers: last..last + 1,
                    background: false,
                    ..viewport(&doc)
                };
                renderer
                    .render_with(&doc, &assets, &opts)
                    .expect("zeichnen")
            },
        ),
    );

    // Export in Originalgrösse (Kopieren, PNG, Auto-Export).
    add(
        "Export: ganzes Dokument rechnen",
        200,
        measure(
            5,
            || (),
            |()| renderer.render(&doc, &assets).expect("zeichnen"),
        ),
    );

    // Scroll-Aufnahme: 15 Bilder einer 1600×1000-Ansicht zusammensetzen.
    let frames = data::scroll_frames(1600, 1000, 15, 180);
    add(
        "Scroll: 15 Bilder zusammensetzen",
        150,
        measure(
            3,
            || frames.clone(),
            |frames| {
                let mut s = Stitcher::new(40_000);
                for f in frames {
                    s.push(f);
                }
                s.finish()
            },
        ),
    );

    // Galerie-Suche über erkannten Text (FTS5).
    data::fill_index(&library, 1500);
    add(
        "Galerie: Volltextsuche (1500 Aufnahmen)",
        20,
        measure(
            21,
            || (),
            |()| library.search("rechnung", 200).expect("suchen"),
        ),
    );

    // Video-Export (ADR 0011, Ziel ≥ 2× Echtzeit bei 1080p30 = 16,7 ms je
    // Bild): Bildschirm und Webcam hochladen, zusammensetzen, zurücklesen.
    // Dekodieren und Kodieren laufen nebenher in der Hardware des Systems;
    // gemessen wird der Teil, der Bild für Bild nacheinander geschieht. Ohne
    // echte Grafikkarte (CI unter Linux/Windows) entfällt die Messung.
    match video_compose() {
        Ok(times) => add(
            "Video-Export: 1080p-Bild mit Webcam zusammensetzen",
            16,
            times,
        ),
        Err(why) => println!("Video-Export übersprungen: {why}"),
    }

    // Bericht
    let mut table =
        String::from("| Messung | Median | Minimum | Budget | |\n|---|---:|---:|---:|---|\n");
    let mut failed = 0;
    for m in &results {
        let ok = m.median <= m.budget;
        if !ok {
            failed += 1;
        }
        let _ = writeln!(
            table,
            "| {} | {:.1} ms | {:.1} ms | {} ms | {} |",
            m.name,
            ms(m.median),
            ms(m.min),
            m.budget.as_millis(),
            if ok { "✅" } else { "❌ über Budget" }
        );
    }
    println!("{table}");
    if let Ok(path) = std::env::var("GITHUB_STEP_SUMMARY") {
        let summary = format!("## Geschwindigkeit ({})\n\n{table}\n", std::env::consts::OS);
        let _ = std::fs::OpenOptions::new()
            .append(true)
            .create(true)
            .open(path)
            .and_then(|mut f| std::io::Write::write_all(&mut f, summary.as_bytes()));
    }
    if failed > 0 && !no_fail {
        eprintln!("{failed} Messung(en) über Budget");
        std::process::exit(1);
    }
}
