//! Misst das Zusammensetzen einer echten Aufnahme Schritt für Schritt –
//! wie die App nach der Aufnahme (ADR 0011). Liest nur, schreibt nach `out`.
//!
//! cargo run --release -p grabmark-media --example export_timing -- \
//!     <Library/2026/10/<id>> <out.mp4> [original|2160|1440|1080|720] [h264|hevc] [sekunden]

#[cfg(target_os = "macos")]
fn main() {
    use std::collections::HashMap;
    use std::path::PathBuf;
    use std::time::{Duration, Instant};

    use grabmark_core::{
        Document, FaceTrack, InputTrack, VideoCodec, VideoEncoding, VideoResolution,
        WebcamBackground, WebcamFraming,
    };
    use grabmark_media::AudioInput;
    use grabmark_video::timeline::{kept, output_duration};
    use grabmark_video::{CursorTrack, ExportExtras, HeadPath};

    let args: Vec<String> = std::env::args().collect();
    let base = PathBuf::from(&args[1]);
    let out = PathBuf::from(&args[2]);
    let resolution = match args.get(3).map(String::as_str) {
        Some("2160") => VideoResolution::P2160,
        Some("1440") => VideoResolution::P1440,
        Some("1080") => VideoResolution::P1080,
        Some("720") => VideoResolution::P720,
        _ => VideoResolution::Original,
    };
    let codec = match args.get(4).map(String::as_str) {
        Some("hevc") => VideoCodec::Hevc,
        _ => VideoCodec::H264,
    };
    let path = |suffix: &str| {
        let p = PathBuf::from(format!("{}{suffix}", base.display()));
        p.exists().then_some(p)
    };
    let read = |p: &PathBuf| std::fs::read(p).unwrap();

    let t0 = Instant::now();
    let mut doc: Document = serde_json::from_slice(&read(&path(".json").unwrap())).unwrap();
    let mut video = doc.video.clone().unwrap();
    // Nur die ersten n Sekunden (für 5-s- und 30-s-Messungen).
    if let Some(secs) = args.get(5).and_then(|s| s.parse::<f64>().ok()) {
        video.duration_ms = video.duration_ms.min((secs * 1000.0) as u32);
        doc.video = Some(video.clone());
    }
    let encoding = VideoEncoding {
        resolution,
        codec,
        ..VideoEncoding::default()
    };
    let screen_path = path("_screen.mp4").unwrap();
    let frame =
        grabmark_video::output_size(grabmark_media::open_video(&screen_path).unwrap().size());
    let size = grabmark_video::encode_size(frame, &encoding);

    let head = video
        .webcam
        .as_ref()
        .filter(|w| w.layout.framing == WebcamFraming::Head)
        .and_then(|w| {
            let faces: FaceTrack = serde_json::from_slice(&read(&path("_faces.json")?)).ok()?;
            HeadPath::new(&faces, w.layout.follow)
        });
    let segmenter = video
        .webcam
        .as_ref()
        .filter(|w| w.layout.background != WebcamBackground::Original)
        .and_then(|_| grabmark_media::segmenter());
    let cursor = path("_input.json")
        .and_then(|p| serde_json::from_slice::<InputTrack>(&read(&p)).ok())
        .map(|t| CursorTrack::new(&t))
        .filter(|t| !t.is_empty());
    let overlay = grabmark_video::overlay::LayerOverlay::new(&doc, HashMap::new())
        .map(|o| Box::new(o) as Box<dyn grabmark_video::OverlaySource>);
    let extras = ExportExtras {
        head,
        segmenter,
        cursor,
        cursor_px: grabmark_video::cursor::CURSOR_POINTS * 2.0,
        overlay,
        size: Some(size),
    };
    let audio: Vec<AudioInput> = [
        (path("_system.m4a"), video.audio.system),
        (path("_mic.m4a"), video.audio.microphone),
    ]
    .into_iter()
    .filter_map(|(p, volume)| Some(AudioInput { path: p?, volume }))
    .collect();
    let prepared = t0.elapsed();

    let mut sink = grabmark_media::create_mp4(
        &out,
        size,
        video.fps,
        &encoding,
        &audio,
        &kept(&video),
        Duration::from_millis(output_duration(&video) as u64),
    )
    .unwrap();
    let screen = grabmark_media::open_video(&screen_path).unwrap();
    let webcam = match (&video.webcam, path("_webcam.mp4")) {
        (Some(_), Some(p)) => Some(grabmark_media::open_video(&p).unwrap()),
        _ => None,
    };
    let opened = t0.elapsed() - prepared;
    let stats =
        grabmark_video::export_with(&video, screen, webcam, extras, sink.as_mut(), &mut |_| true)
            .unwrap();
    let poster_started = Instant::now();
    let poster = grabmark_media::frame_at(&out, Duration::ZERO).unwrap();
    let poster_time = poster_started.elapsed();
    let png_started = Instant::now();
    let png = grabmark_library::encode_png(&poster, 2.0).unwrap();
    let png_time = png_started.elapsed();

    let secs = output_duration(&video) as f64 / 1000.0;
    let total = t0.elapsed();
    println!(
        "{:.1} s Video, {}×{} → {}×{} {:?}, {} Bilder, {}",
        secs, frame.0, frame.1, size.0, size.1, codec, stats.frames, stats.adapter
    );
    println!(
        "  vorbereiten {} ms, öffnen {} ms",
        prepared.as_millis(),
        opened.as_millis()
    );
    println!("  {}", stats.times);
    println!(
        "  Standbild lesen {} ms, PNG {} ms ({} KB)",
        poster_time.as_millis(),
        png_time.as_millis(),
        png.len() / 1024
    );
    println!(
        "  Export {:.0} ms, gesamt {:.0} ms → {:.2}× Echtzeit",
        stats.elapsed.as_secs_f64() * 1000.0,
        total.as_secs_f64() * 1000.0,
        secs / total.as_secs_f64()
    );
}

#[cfg(not(target_os = "macos"))]
fn main() {}
