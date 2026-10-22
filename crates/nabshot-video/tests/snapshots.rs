//! Snapshot-Tests des Exports (ADR 0011): Für feste Zeitpunkte wird ein Bild
//! zusammengesetzt und mit einem Referenzbild in `tests/snapshots/`
//! verglichen. Die Spuren sind erzeugte Testbilder, damit der Test ohne
//! Videodateien und auf jeder Plattform läuft.
//!
//! Referenzen neu schreiben: `UPDATE_SNAPSHOTS=1 cargo test -p nabshot-video`
//! Ohne Grafikkarte (z. B. Linux-CI) wird der Test übersprungen.

#![cfg(feature = "gpu")]

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::time::Duration;

use nabshot_core::{
    VideoDoc, WebcamBorder, WebcamLayout, WebcamMode, WebcamPosition, WebcamSection, WebcamShape,
    WebcamTrack,
};
use nabshot_video::{FrameSink, FrameSource, VideoError, VideoFrame, export};

/// Erzeugte Spur: Bilder zu festen Zeiten, `paint` malt Pixel (x, y, Bild).
struct Synthetic {
    size: (u32, u32),
    times_ms: Vec<u64>,
    next: usize,
    paint: fn(u32, u32, usize) -> [u8; 3],
}

impl FrameSource for Synthetic {
    fn size(&self) -> (u32, u32) {
        self.size
    }

    fn next_frame(&mut self) -> Result<Option<VideoFrame>, String> {
        let Some(&t) = self.times_ms.get(self.next) else {
            return Ok(None);
        };
        let index = self.next;
        self.next += 1;
        let (w, h) = self.size;
        let mut data = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            for x in 0..w {
                let [r, g, b] = (self.paint)(x, y, index);
                data.extend_from_slice(&[b, g, r, 255]);
            }
        }
        Ok(Some(VideoFrame {
            pts: Duration::from_millis(t),
            width: w,
            height: h,
            stride: (w * 4) as usize,
            data,
        }))
    }
}

/// Bildschirm: Verlauf mit Raster und einem Quadrat, das pro Bild wandert.
fn screen_pixel(x: u32, y: u32, index: usize) -> [u8; 3] {
    let square = (40 + index as u32 * 60, 120);
    if x >= square.0 && x < square.0 + 50 && y >= square.1 && y < square.1 + 50 {
        return [230, 57, 53];
    }
    if x.is_multiple_of(40) || y.is_multiple_of(40) {
        return [40, 40, 48];
    }
    [(x * 255 / 640) as u8, (y * 255 / 360) as u8, 200]
}

/// Webcam (4:3): vier Farbfelder, links oben gelb – zeigt Spiegeln und Ausschnitt.
fn webcam_pixel(x: u32, y: u32, _index: usize) -> [u8; 3] {
    match (x < 160, y < 120) {
        (true, true) => [250, 210, 40],
        (false, true) => [40, 160, 90],
        (true, false) => [60, 90, 220],
        (false, false) => [200, 70, 160],
    }
}

/// Merkt sich die Bilder zu den gewünschten Zeiten (als RGBA).
struct Collect {
    wanted: Vec<Duration>,
    size: (u32, u32),
    frames: BTreeMap<u128, Vec<u8>>,
    finished: bool,
}

impl FrameSink for Collect {
    fn push(&mut self, bgra: &[u8], stride: usize, pts: Duration) -> Result<(), String> {
        if !self.wanted.contains(&pts) {
            return Ok(());
        }
        let (w, h) = self.size;
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for row in bgra.chunks(stride).take(h as usize) {
            for px in row[..(w * 4) as usize].as_chunks::<4>().0 {
                rgba.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
            }
        }
        self.frames.insert(pts.as_millis(), rgba);
        Ok(())
    }

    fn finish(&mut self) -> Result<(), String> {
        self.finished = true;
        Ok(())
    }

    fn cancel(&mut self) {}
}

fn snapshot_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/snapshots")
}

fn write_png(path: &PathBuf, (w, h): (u32, u32), rgba: &[u8]) {
    let file = std::fs::File::create(path).unwrap();
    let mut enc = png::Encoder::new(std::io::BufWriter::new(file), w, h);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    enc.write_header().unwrap().write_image_data(rgba).unwrap();
}

fn read_png(path: &PathBuf) -> Option<((u32, u32), Vec<u8>)> {
    let file = std::fs::File::open(path).ok()?;
    let mut reader = png::Decoder::new(std::io::BufReader::new(file))
        .read_info()
        .ok()?;
    let mut buf = vec![0; reader.output_buffer_size()?];
    let info = reader.next_frame(&mut buf).ok()?;
    buf.truncate(info.buffer_size());
    Some(((info.width, info.height), buf))
}

/// Vergleicht mit der Referenz. Grafikkarten glätten Kanten leicht
/// verschieden; darum dürfen wenige Pixel etwas abweichen.
fn assert_snapshot(name: &str, size: (u32, u32), rgba: &[u8]) {
    let dir = snapshot_dir();
    let path = dir.join(format!("{name}.png"));
    if std::env::var_os("UPDATE_SNAPSHOTS").is_some() || !path.exists() {
        std::fs::create_dir_all(&dir).unwrap();
        write_png(&path, size, rgba);
        eprintln!("Referenz geschrieben: {}", path.display());
        return;
    }
    let (ref_size, reference) = read_png(&path).expect("Referenzbild lesbar");
    assert_eq!(ref_size, size, "{name}: Grösse");
    let differing = rgba
        .as_chunks::<4>()
        .0
        .iter()
        .zip(reference.as_chunks::<4>().0)
        .filter(|(a, b)| a.iter().zip(*b).any(|(x, y)| x.abs_diff(*y) > 8))
        .count();
    let allowed = (size.0 * size.1) as usize / 200;
    if differing > allowed {
        let actual = std::env::temp_dir().join(format!("nabshot-{name}-actual.png"));
        write_png(&actual, size, rgba);
        panic!(
            "{name}: {differing} Pixel weichen ab (erlaubt {allowed}); Ergebnis: {}",
            actual.display()
        );
    }
}

fn sources() -> (Box<dyn FrameSource>, Box<dyn FrameSource>) {
    // Bildschirm mit wechselnder Bildrate wie bei ScreenCaptureKit
    let screen = Synthetic {
        size: (640, 360),
        times_ms: vec![0, 150, 400, 700, 1000, 1400, 1800],
        next: 0,
        paint: screen_pixel,
    };
    let webcam = Synthetic {
        size: (320, 240),
        times_ms: (0..20).map(|i| i * 100).collect(),
        next: 0,
        paint: webcam_pixel,
    };
    (Box::new(screen), Box::new(webcam))
}

fn run(name: &str, layout: WebcamLayout, sections: &[(u32, WebcamMode)], times_ms: &[u64]) {
    let doc = VideoDoc {
        duration_ms: 2000,
        fps: 10,
        webcam: Some(WebcamTrack {
            layout,
            sections: sections
                .iter()
                .map(|&(start_ms, mode)| WebcamSection { start_ms, mode })
                .collect(),
        }),
        ..VideoDoc::default()
    };
    let (screen, webcam) = sources();
    let mut sink = Collect {
        wanted: times_ms.iter().map(|&t| Duration::from_millis(t)).collect(),
        size: (640, 360),
        frames: BTreeMap::new(),
        finished: false,
    };
    let stats = match export(&doc, screen, Some(webcam), &mut sink, &mut |_| true) {
        Ok(s) => s,
        Err(VideoError::NoGpu(e)) => {
            eprintln!("Übersprungen, keine Grafikkarte: {e}");
            return;
        }
        Err(e) => panic!("{e}"),
    };
    assert_eq!(stats.frames, 20);
    assert!(sink.finished);
    for t in times_ms {
        let frame = sink.frames.get(&(*t as u128)).expect("Bild zum Zeitpunkt");
        assert_snapshot(&format!("{name}_{t:04}ms"), (640, 360), frame);
    }
}

#[test]
fn kreis_gross_klein_und_aus() {
    run(
        "kreis",
        WebcamLayout::default(),
        &[
            (0, WebcamMode::Overlay),
            (800, WebcamMode::Large),
            (1500, WebcamMode::Hidden),
        ],
        // ruhig, mitten im Übergang, gross, ausgeblendet
        &[0, 500, 900, 1200, 1900],
    );
}

#[test]
fn hochformat_frei_ohne_schatten() {
    run(
        "hochformat",
        WebcamLayout {
            position: WebcamPosition::Free { x: 0.3, y: 0.4 },
            size: 0.18,
            shape: WebcamShape::Portrait,
            corner_radius: 0.2,
            border: Some(WebcamBorder {
                width: 0.04,
                color: nabshot_core::Color::rgb(229, 57, 53),
            }),
            shadow: false,
            mirror: false,
            ..WebcamLayout::default()
        },
        &[(0, WebcamMode::Overlay)],
        &[300],
    );
}

#[test]
fn webcam_spaeter_eingeschaltet() {
    run(
        "spaeter",
        WebcamLayout {
            position: WebcamPosition::TopLeft,
            shape: WebcamShape::Landscape,
            border: None,
            ..WebcamLayout::default()
        },
        &[(0, WebcamMode::Hidden), (1000, WebcamMode::Overlay)],
        &[500, 1100, 1500],
    );
}

#[test]
fn nur_webcam_hochformat() {
    let doc = VideoDoc {
        duration_ms: 2000,
        fps: 10,
        webcam: Some(WebcamTrack {
            layout: WebcamLayout::default(),
            sections: vec![WebcamSection {
                start_ms: 0,
                mode: WebcamMode::Large,
            }],
        }),
        camera_only: Some(nabshot_core::CameraFormat::Portrait),
        ..VideoDoc::default()
    };
    let size = (180, 320);
    let (_, webcam) = sources();
    let mut sink = Collect {
        wanted: vec![Duration::from_millis(500)],
        size,
        frames: BTreeMap::new(),
        finished: false,
    };
    let screen = Box::new(nabshot_video::BlankSource::new(size));
    match export(&doc, screen, Some(webcam), &mut sink, &mut |_| true) {
        Ok(stats) => assert_eq!(stats.frames, 20),
        Err(VideoError::NoGpu(e)) => {
            eprintln!("Übersprungen, keine Grafikkarte: {e}");
            return;
        }
        Err(e) => panic!("{e}"),
    }
    let frame = sink.frames.get(&500).expect("Bild zum Zeitpunkt");
    assert_snapshot("nur_webcam_0500ms", size, frame);
}
