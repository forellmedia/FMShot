//! Misst die Aufnahme aller Monitore: `cargo run -p fmshot-capture --example capture_all --release`

use std::time::Instant;

fn main() {
    let capturer = fmshot_capture::platform_capturer();
    if !capturer.has_permission() {
        eprintln!("Keine Freigabe für Bildschirmaufnahme – frage an …");
        capturer.request_permission();
        return;
    }
    for d in capturer.displays().expect("Monitore") {
        println!("{d:?}");
    }
    for run in 1..=3 {
        let start = Instant::now();
        let frames = capturer.capture_all().expect("Aufnahme");
        let ms = start.elapsed().as_secs_f64() * 1000.0;
        let sizes: Vec<_> = frames
            .iter()
            .map(|f| format!("{}×{}", f.image.width(), f.image.height()))
            .collect();
        println!("Lauf {run}: {ms:.1} ms – {}", sizes.join(", "));
    }
}
