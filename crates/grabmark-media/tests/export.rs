//! Export über das System (macOS): Testvideos schreiben, wieder lesen, mit
//! Webcam zusammensetzen. Der Budget-Test (1080p30 ≥ 2× Echtzeit, ADR 0011)
//! braucht Hardware-Encoder und GPU und läuft darum nur auf Wunsch:
//!
//! cargo test -p grabmark-media --release --test export -- --ignored --nocapture

#![cfg(target_os = "macos")]

use std::path::Path;
use std::time::{Duration, Instant};

use grabmark_core::{
    VideoDoc, WebcamBackground, WebcamLayout, WebcamMode, WebcamSection, WebcamTrack,
};
use grabmark_video::{ExportExtras, VideoError};

/// Schreibt ein Testvideo: `frames` Bilder ab `start`, Farbe wechselt pro Bild.
fn write_video(path: &Path, size: (u32, u32), fps: u32, frames: u32, start: Duration) {
    let duration = start + Duration::from_secs_f64(frames as f64 / fps as f64);
    let mut sink =
        grabmark_media::create_mp4(path, size, fps, &Default::default(), &[], &[], duration)
            .expect("Writer");
    let stride = size.0 as usize * 4;
    let mut data = vec![0u8; stride * size.1 as usize];
    for i in 0..frames {
        for (n, px) in data.as_chunks_mut::<4>().0.iter_mut().enumerate() {
            let x = (n % size.0 as usize) as u32;
            let v = ((x + i * 7) % 256) as u8;
            px.copy_from_slice(&[v, 255 - v, (i * 9 % 256) as u8, 255]);
        }
        let pts = start + Duration::from_secs_f64(i as f64 / fps as f64);
        sink.push(&data, stride, pts).expect("Bild");
    }
    sink.finish().expect("abschliessen");
}

fn read_all(path: &Path) -> Vec<Duration> {
    let mut source = grabmark_media::open_video(path).expect("lesen");
    let mut times = Vec::new();
    while let Some(f) = source.next_frame().expect("Bild") {
        assert_eq!((f.width, f.height), source.size());
        assert!(f.stride >= f.width as usize * 4);
        times.push(f.pts);
    }
    times
}

#[test]
fn geschriebenes_video_wird_gelesen() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("a.mp4");
    write_video(&path, (320, 180), 30, 15, Duration::ZERO);
    let times = read_all(&path);
    assert_eq!(times.len(), 15);
    assert_eq!(times[0], Duration::ZERO);
    assert!((times[14].as_secs_f64() - 14.0 / 30.0).abs() < 0.002);
}

/// Einstellungen *Codec* und *Auflösung*: HEVC in kleinerer Grösse wird
/// geschrieben und lässt sich wieder lesen.
#[test]
fn hevc_kleiner_wird_gelesen() {
    let dir = tempfile::tempdir().unwrap();
    let screen = dir.path().join("screen.mp4");
    write_video(&screen, (1280, 720), 30, 10, Duration::ZERO);
    let encoding = grabmark_core::VideoEncoding {
        resolution: grabmark_core::VideoResolution::P720,
        codec: grabmark_core::VideoCodec::Hevc,
        quality: grabmark_core::VideoQuality::Small,
    };
    let size = grabmark_video::export_size((1280, 720), grabmark_core::VideoResolution::P720);
    assert_eq!(size, (1280, 720));
    let half = grabmark_video::export_size((2560, 1440), encoding.resolution);
    assert_eq!(half, (1280, 720));
    let out = dir.path().join("out.mp4");
    let doc = VideoDoc {
        duration_ms: 333,
        fps: 30,
        ..VideoDoc::default()
    };
    let mut sink = grabmark_media::create_mp4(
        &out,
        (640, 360),
        30,
        &encoding,
        &[],
        &[],
        Duration::from_millis(333),
    )
    .unwrap();
    let extras = ExportExtras {
        size: Some((640, 360)),
        ..ExportExtras::default()
    };
    let source = grabmark_media::open_video(&screen).unwrap();
    match grabmark_video::export_with(&doc, source, None, extras, sink.as_mut(), &mut |_| true) {
        Ok(_) | Err(VideoError::NoGpu(_)) => {}
        Err(e) => panic!("{e}"),
    }
    if out.exists() {
        assert_eq!(grabmark_media::open_video(&out).unwrap().size(), (640, 360));
        assert!(!read_all(&out).is_empty());
    }
}

/// Webcam, die erst während der Aufnahme eingeschaltet wurde: Die Spur
/// beginnt später, die Zeit zählt trotzdem ab dem gemeinsamen Nullpunkt.
#[test]
fn spaeter_beginnende_spur_behaelt_ihre_zeit() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("cam.mp4");
    write_video(&path, (320, 240), 30, 6, Duration::from_millis(1500));
    // Die Leerstelle davor meldet AVAssetReader als ein Bild bei 0; im
    // Export ist die Webcam dort ohnehin ausgeblendet (Abschnitte).
    let times: Vec<Duration> = read_all(&path)
        .into_iter()
        .filter(|t| !t.is_zero())
        .collect();
    assert_eq!(times.len(), 6);
    assert!(
        (times[0].as_secs_f64() - 1.5).abs() < 0.002,
        "erstes Bild bei {:?}",
        times[0]
    );
}

fn doc(duration_ms: u32, fps: u32, background: WebcamBackground) -> VideoDoc {
    VideoDoc {
        duration_ms,
        fps,
        webcam: Some(WebcamTrack {
            layout: WebcamLayout {
                background,
                ..WebcamLayout::default()
            },
            sections: vec![
                WebcamSection {
                    start_ms: 0,
                    mode: WebcamMode::Overlay,
                },
                WebcamSection {
                    start_ms: duration_ms / 2,
                    mode: WebcamMode::Large,
                },
            ],
        }),
        ..VideoDoc::default()
    }
}

/// Setzt zusammen; `None`, wenn es keine Grafikkarte gibt. Mit einem
/// anderen Hintergrund als Original rechnet Vision die Maske je Bild.
fn export(
    dir: &Path,
    screen: (u32, u32),
    seconds: u32,
    background: WebcamBackground,
) -> Option<(Duration, u64)> {
    let fps = 30;
    let screen_path = dir.join("screen.mp4");
    let cam_path = dir.join("webcam.mp4");
    write_video(&screen_path, screen, fps, seconds * fps, Duration::ZERO);
    write_video(&cam_path, (1280, 720), fps, seconds * fps, Duration::ZERO);

    let out = dir.join("video.mp4");
    let screen_src = grabmark_media::open_video(&screen_path).unwrap();
    let size = grabmark_video::output_size(screen_src.size());
    let duration = Duration::from_secs(seconds as u64);
    let mut sink =
        grabmark_media::create_mp4(&out, size, fps, &Default::default(), &[], &[], duration)
            .unwrap();
    let started = Instant::now();
    let extras = ExportExtras {
        segmenter: (background != WebcamBackground::Original)
            .then(grabmark_media::segmenter)
            .flatten(),
        ..ExportExtras::default()
    };
    let stats = match grabmark_video::export_with(
        &doc(seconds * 1000, fps, background),
        screen_src,
        Some(grabmark_media::open_video(&cam_path).unwrap()),
        extras,
        sink.as_mut(),
        &mut |_| true,
    ) {
        Ok(s) => s,
        Err(VideoError::NoGpu(e)) => {
            eprintln!("Übersprungen, keine Grafikkarte: {e}");
            return None;
        }
        Err(e) => panic!("{e}"),
    };
    let elapsed = started.elapsed();
    let times = read_all(&out);
    assert_eq!(times.len() as u64, stats.frames);
    eprintln!(
        "Export {}×{} {} s: {:.2} s ({:.1}× Echtzeit) auf {}",
        size.0,
        size.1,
        seconds,
        elapsed.as_secs_f64(),
        seconds as f64 / elapsed.as_secs_f64(),
        stats.adapter
    );
    Some((elapsed, stats.frames))
}

#[test]
fn export_mit_webcam_ergibt_lesbares_video() {
    let dir = tempfile::tempdir().unwrap();
    if let Some((_, frames)) = export(dir.path(), (640, 360), 1, WebcamBackground::Original) {
        assert_eq!(frames, 30);
    }
}

#[test]
#[ignore = "Budget-Messung, braucht Hardware-Encoder und GPU"]
fn budget_export_1080p30_mindestens_doppelte_echtzeit() {
    let dir = tempfile::tempdir().unwrap();
    let seconds = 10;
    if let Some((elapsed, _)) = export(
        dir.path(),
        (1920, 1080),
        seconds,
        WebcamBackground::Original,
    ) {
        let budget = Duration::from_secs(seconds as u64) / 2;
        assert!(
            elapsed <= budget,
            "Export dauerte {elapsed:?}, Budget {budget:?}"
        );
    }
}

#[test]
fn export_mit_unscharfem_hintergrund_ergibt_lesbares_video() {
    let dir = tempfile::tempdir().unwrap();
    if let Some((_, frames)) = export(dir.path(), (640, 360), 1, WebcamBackground::Blur) {
        assert_eq!(frames, 30);
    }
}

#[test]
#[ignore = "Budget-Messung, braucht Hardware-Encoder, GPU und Neural Engine"]
fn budget_export_1080p30_mit_maske_mindestens_doppelte_echtzeit() {
    let dir = tempfile::tempdir().unwrap();
    let seconds = 10;
    if let Some((elapsed, _)) = export(dir.path(), (1920, 1080), seconds, WebcamBackground::Blur) {
        let budget = Duration::from_secs(seconds as u64) / 2;
        assert!(
            elapsed <= budget,
            "Export dauerte {elapsed:?}, Budget {budget:?}"
        );
    }
}
