//! Video-Aufnahme (ADR 0011): Ziel wählen (Bereich, Fenster, Bildschirm),
//! Countdown, Aufnahmeleiste mit Pause/Stopp und Webcam, danach die Spuren
//! zusammensetzen und in die Bibliothek legen. Die Aufnahme macht
//! `fmshot-media`, Layout und Export `fmshot-video`; hier wird nur verdrahtet.

use std::cell::RefCell;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fmshot_capture::{PrivacyPane, display_at};
use fmshot_core::{
    InputSample, InputTrack, Rect, Size, VideoDoc, WebcamLayout, WebcamMode, WebcamShape,
    WebcamTrack,
};
use fmshot_library::VideoSlot;
use fmshot_media::{MediaError, Permission, RecordFiles, RecordOptions, RecordTarget, Recording};
use fmshot_overlay::{
    BubbleStyle, OverlayHandle, OverlayMode, OverlayResult, RecordAction, RecordLabels, RecordUi,
    WebcamBubble, WindowTarget,
};
use fmshot_settings::{VideoSettings, VideoTarget};
use fmshot_video::SectionLog;
use tauri::{AppHandle, Manager};

use crate::capture::{self, CaptureState, Mode};
use crate::i18n::t;

/// Wie oft die Leiste (Zeit, Pegel) nachgeführt wird.
const TICK: Duration = Duration::from_millis(200);
/// Stufen der Pegelanzeige.
const METER_STEPS: u32 = 5;

/// Laufende Aufnahme – erlaubt „Stopp“ per Tastenkürzel.
#[derive(Default)]
pub struct VideoState {
    session: Mutex<Option<Flags>>,
    /// Zwischen Tastendruck und Aufnahmebeginn (Auswahl, Freigaben).
    starting: AtomicBool,
}

#[derive(Clone, Default)]
struct Flags {
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    toggle_pause: Arc<AtomicBool>,
    toggle_webcam: Arc<AtomicBool>,
    toggle_large: Arc<AtomicBool>,
}

thread_local! {
    static UI: RefCell<Option<RecordUi>> = const { RefCell::new(None) };
    static OVERLAY: RefCell<Option<OverlayHandle>> = const { RefCell::new(None) };
    static BUBBLE: RefCell<Option<WebcamBubble>> = const { RefCell::new(None) };
}

/// Was nach der Auswahl aufgenommen wird.
#[derive(Debug, Clone, Copy)]
struct Choice {
    target: RecordTarget,
    display_id: u32,
    /// Rahmen um den Bereich (physische Pixel des Monitors).
    rect: Option<(u32, u32, u32, u32)>,
    /// Aufgenommene Fläche (physische Pixel des Monitors) – Bezug für die
    /// Lage der Webcam-Blase.
    area: (u32, u32, u32, u32),
    scale: f32,
}

/// Läuft schon ein Video? Dann beendet das Kürzel es.
pub fn stop_running(app: &AppHandle) -> bool {
    let state = app.state::<VideoState>();
    let guard = state.session.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(flags) => {
            flags.stop.store(true, Ordering::Release);
            true
        }
        None => false,
    }
}

/// Startet eine Video-Aufnahme mit dem Ziel aus den Einstellungen bzw.
/// `target`. Die Bildschirm-Freigabe hat `capture::start` schon geprüft.
pub fn start(app: &AppHandle, target: Option<VideoTarget>) {
    let state = app.state::<VideoState>();
    if state.starting.swap(true, Ordering::AcqRel) {
        return;
    }
    let settings = crate::settings::get(app).video;
    let target = target.unwrap_or(settings.target);
    let app = app.clone();
    std::thread::spawn(move || {
        let microphone = settings.microphone
            && match fmshot_media::microphone_permission() {
                Permission::Granted => true,
                // Systemfenster; ohne Zustimmung ohne Mikrofon weiter.
                Permission::NotDetermined => fmshot_media::request_microphone_permission(),
                Permission::Denied => {
                    done_starting(&app);
                    capture::open_privacy_settings(&app, PrivacyPane::Microphone);
                    return;
                }
            };
        let webcam = settings.webcam
            && !fmshot_media::cameras().is_empty()
            && match fmshot_media::camera_permission() {
                Permission::Granted => true,
                Permission::NotDetermined => fmshot_media::request_camera_permission(),
                Permission::Denied => {
                    done_starting(&app);
                    capture::open_privacy_settings(&app, PrivacyPane::Camera);
                    return;
                }
            };
        select(&app, target, move |app, choice| {
            let app = app.clone();
            std::thread::spawn(move || run(&app, choice, microphone, webcam, settings));
        });
    });
}

fn done_starting(app: &AppHandle) {
    app.state::<VideoState>()
        .starting
        .store(false, Ordering::Release);
}

/// Ziel wählen; `then` läuft im Haupt-Thread, sobald gewählt ist.
fn select(
    app: &AppHandle,
    target: VideoTarget,
    then: impl FnOnce(&AppHandle, Choice) + Send + 'static,
) {
    let state = app.state::<CaptureState>();
    let displays = match state.capturer.displays() {
        Ok(d) => d,
        Err(e) => {
            done_starting(app);
            capture::show_error(app, &e.to_string());
            return;
        }
    };
    let scale_of = {
        let displays = displays.clone();
        move |id: u32| {
            displays
                .iter()
                .find(|d| d.id == id)
                .map(|d| d.scale_factor)
                .unwrap_or(2.0)
        }
    };
    match target {
        VideoTarget::Screen => {
            let cursor = state.capturer.cursor_position();
            let Some(display) = cursor
                .and_then(|c| display_at(&displays, c))
                .or_else(|| displays.iter().find(|d| d.is_primary))
            else {
                done_starting(app);
                return;
            };
            let s = display.scale_factor.max(1.0);
            let choice = Choice {
                target: RecordTarget::Display {
                    display_id: display.id,
                },
                display_id: display.id,
                rect: None,
                area: (
                    0,
                    0,
                    (display.bounds.width * s) as u32,
                    (display.bounds.height * s) as u32,
                ),
                scale: display.scale_factor,
            };
            let a = app.clone();
            let _ = app.run_on_main_thread(move || then(&a, choice));
        }
        VideoTarget::Region => {
            let a = app.clone();
            let _ = app.run_on_main_thread(move || {
                let app_done = a.clone();
                let result = fmshot_overlay::open(
                    OverlayMode::Region,
                    Vec::new(),
                    Box::new(move |result| {
                        OVERLAY.with(|o| o.borrow_mut().take());
                        let OverlayResult::Selected(sel) = result else {
                            done_starting(&app_done);
                            return;
                        };
                        then(
                            &app_done,
                            Choice {
                                target: RecordTarget::Region {
                                    display_id: sel.display_id,
                                    x: sel.x,
                                    y: sel.y,
                                    width: sel.width,
                                    height: sel.height,
                                },
                                display_id: sel.display_id,
                                rect: Some((sel.x, sel.y, sel.width, sel.height)),
                                area: (sel.x, sel.y, sel.width, sel.height),
                                scale: scale_of(sel.display_id),
                            },
                        );
                    }),
                );
                open_result(&a, result);
            });
        }
        VideoTarget::Window => {
            let windows = match state.capturer.windows() {
                Ok(w) => w,
                Err(e) => {
                    done_starting(app);
                    capture::show_error(app, &e.to_string());
                    return;
                }
            };
            let targets: Vec<WindowTarget> = windows
                .iter()
                .map(|w| WindowTarget {
                    id: w.id,
                    bounds: (
                        w.bounds.x as f64,
                        w.bounds.y as f64,
                        w.bounds.width as f64,
                        w.bounds.height as f64,
                    ),
                })
                .collect();
            let a = app.clone();
            let _ = app.run_on_main_thread(move || {
                let app_done = a.clone();
                let result = fmshot_overlay::open(
                    OverlayMode::Window,
                    targets,
                    Box::new(move |result| {
                        OVERLAY.with(|o| o.borrow_mut().take());
                        let OverlayResult::Window(id) = result else {
                            done_starting(&app_done);
                            return;
                        };
                        let Some(w) = windows.iter().find(|w| w.id == id) else {
                            done_starting(&app_done);
                            return;
                        };
                        let b = w.bounds;
                        let center = (b.x + b.width / 2.0, b.y + b.height / 2.0);
                        let display = display_at(&displays, center)
                            .or_else(|| displays.iter().find(|d| d.is_primary));
                        let (display_id, scale, rect) = match display {
                            Some(d) => {
                                let s = d.scale_factor.max(1.0);
                                let x0 = ((b.x - d.bounds.x) * s).max(0.0);
                                let y0 = ((b.y - d.bounds.y) * s).max(0.0);
                                let x1 = ((b.right() - d.bounds.x) * s).min(d.bounds.width * s);
                                let y1 = ((b.bottom() - d.bounds.y) * s).min(d.bounds.height * s);
                                let rect = (
                                    x0 as u32,
                                    y0 as u32,
                                    (x1 - x0).max(0.0) as u32,
                                    (y1 - y0).max(0.0) as u32,
                                );
                                (d.id, d.scale_factor, Some(rect))
                            }
                            None => (0, 2.0, None),
                        };
                        then(
                            &app_done,
                            Choice {
                                target: RecordTarget::Window { window_id: id },
                                display_id,
                                rect,
                                area: rect.unwrap_or((0, 0, 1920, 1080)),
                                scale,
                            },
                        );
                    }),
                );
                open_result(&a, result);
            });
        }
    }
}

fn open_result(app: &AppHandle, result: Result<OverlayHandle, fmshot_overlay::OverlayError>) {
    match result {
        Ok(handle) => OVERLAY.with(|o| *o.borrow_mut() = Some(handle)),
        Err(e) => {
            done_starting(app);
            capture::show_error(app, &e.to_string());
        }
    }
}

fn run(app: &AppHandle, choice: Choice, microphone: bool, webcam: bool, settings: VideoSettings) {
    let library = app.state::<CaptureState>().library.clone();
    let slot = match library.reserve_video(now_ms()) {
        Ok(s) => s,
        Err(e) => {
            done_starting(app);
            capture::show_error(app, &e.to_string());
            return;
        }
    };
    // Mit Kamera am System lässt sich die Webcam jederzeit zuschalten.
    let has_camera = !fmshot_media::cameras().is_empty();
    let files = RecordFiles {
        screen: slot.screen_path(),
        system_audio: settings.system_audio.then(|| slot.system_audio_path()),
        microphone: microphone.then(|| slot.microphone_path()),
        webcam: has_camera.then(|| slot.webcam_path()),
    };
    // Kurz warten, bis das Auswahl-Overlay verschwunden ist.
    std::thread::sleep(Duration::from_millis(80));
    let recording = fmshot_media::start_recording(RecordOptions {
        target: choice.target,
        files: files.clone(),
        fps: settings.fps,
        microphone_id: settings.microphone_id.clone(),
        // FMShot zeichnet den Zeiger selbst aus der Mausspur (Etappe 5).
        show_cursor: false,
        camera_id: settings.camera_id.clone(),
    });
    done_starting(app);
    let recording: Arc<dyn Recording> = match recording {
        Ok(r) => Arc::from(r),
        Err(e) => {
            library.discard_video(&slot);
            let message = match e {
                MediaError::NotImplemented => t("video.unavailable", &[]),
                e => e.to_string(),
            };
            capture::show_error(app, &message);
            return;
        }
    };

    let flags = Flags::default();
    *app.state::<VideoState>()
        .session
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = Some(flags.clone());
    let layout = settings.webcam_layout;
    let mut cam = WebcamControl {
        on: false,
        starting: None,
        large: false,
        log: SectionLog::new(),
        bubble_rect: bubble_rect(&layout, &choice),
        layout,
    };
    open_ui(
        app,
        &choice,
        &flags,
        settings.countdown,
        has_camera.then_some(webcam),
    );
    // Die Kamera braucht bis etwa 1 s – sie läuft parallel zum Countdown an.
    if webcam {
        cam.start(&recording);
    }

    if settings.countdown {
        for n in (1..=3).rev() {
            set_status(app, t("video.countdown", &[("n", &n.to_string())]));
            for _ in 0..5 {
                std::thread::sleep(TICK);
                cam.handle(app, &flags, &recording, &choice, None);
                if flags.cancel.load(Ordering::Acquire) || flags.stop.load(Ordering::Acquire) {
                    break;
                }
            }
        }
    }
    if !flags.cancel.load(Ordering::Acquire) {
        recording.begin();
        cam.log.set(0, cam.mode());
    }
    let mouse = MouseLog::start(app, &recording, &choice);

    let mut failure = None;
    while !flags.stop.load(Ordering::Acquire) && !flags.cancel.load(Ordering::Acquire) {
        if flags.toggle_pause.swap(false, Ordering::AcqRel) {
            let label = if recording.is_paused() {
                recording.resume();
                t("video.pause", &[])
            } else {
                recording.pause();
                t("video.resume", &[])
            };
            set_label(app, RecordAction::Pause, label);
        }
        let at = recording.elapsed().as_millis().min(u32::MAX as u128) as u32;
        cam.handle(app, &flags, &recording, &choice, Some(at));
        if let Some(e) = recording.failure() {
            failure = Some(e);
            break;
        }
        set_status(
            app,
            status_text(recording.as_ref(), microphone, settings.system_audio),
        );
        std::thread::sleep(TICK);
    }

    *app.state::<VideoState>()
        .session
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = None;
    // Lage der Blase = Startlayout der Webcam.
    let bubble = close_bubble(app);

    if flags.cancel.load(Ordering::Acquire) {
        close_ui(app);
        recording.cancel();
        library.discard_video(&slot);
        return;
    }
    let samples = mouse.finish();
    let stopped = recording.stop();
    match stopped {
        Ok(summary) => {
            write_json(&slot.input_path(), &InputTrack { samples });
            if let Some(e) = failure {
                log::warn!("Video endete vorzeitig: {e}");
            }
            let webcam = files
                .webcam
                .as_deref()
                .filter(|p| p.exists() && cam.log.shows_webcam())
                .map(|_| WebcamTrack {
                    layout: webcam_layout(cam.layout, &choice, bubble),
                    sections: cam.log.into_sections(),
                });
            let doc = VideoDoc {
                duration_ms: summary.duration.as_millis().min(u32::MAX as u128) as u32,
                fps: settings.fps,
                webcam,
                ..VideoDoc::default()
            };
            finish(app, &slot, &files, summary, choice.scale, doc);
        }
        Err(MediaError::Empty) => {
            library.discard_video(&slot);
            if let Some(e) = failure {
                capture::show_error(app, &e);
            }
        }
        Err(e) => {
            library.discard_video(&slot);
            capture::show_error(app, &e.to_string());
        }
    }
    close_ui(app);
}

/// Abstand der Proben der Mausspur (~60 je Sekunde).
const MOUSE_INTERVAL: Duration = Duration::from_millis(16);

/// Mausspur während der Aufnahme (Etappe 5): Lage relativ zum
/// aufgenommenen Bereich und ob die Taste gedrückt ist.
struct MouseLog {
    running: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<Vec<InputSample>>>,
}

impl MouseLog {
    fn start(app: &AppHandle, recording: &Arc<dyn Recording>, choice: &Choice) -> Self {
        let running = Arc::new(AtomicBool::new(true));
        let area = area_points(app, choice);
        let (app, recording, flag) = (app.clone(), recording.clone(), running.clone());
        let thread = std::thread::spawn(move || {
            let mut samples = Vec::new();
            let Some((ax, ay, aw, ah)) = area else {
                return samples;
            };
            let capturer = &app.state::<CaptureState>().capturer;
            let mut last: Option<(f32, f32, bool)> = None;
            while flag.load(Ordering::Acquire) {
                if !recording.is_paused()
                    && let Some((x, y)) = capturer.cursor_position()
                {
                    let down = capturer.mouse_button_down();
                    let sample = ((x - ax) / aw.max(1.0), (y - ay) / ah.max(1.0), down);
                    // Unveränderte Lage nicht wiederholen (spart Platz).
                    if last != Some(sample) {
                        last = Some(sample);
                        samples.push(InputSample {
                            t_ms: recording.elapsed().as_millis().min(u32::MAX as u128) as u32,
                            x: sample.0,
                            y: sample.1,
                            down,
                        });
                    }
                }
                std::thread::sleep(MOUSE_INTERVAL);
            }
            samples
        });
        Self {
            running,
            thread: Some(thread),
        }
    }

    fn finish(mut self) -> Vec<InputSample> {
        self.running.store(false, Ordering::Release);
        self.thread
            .take()
            .and_then(|t| t.join().ok())
            .unwrap_or_default()
    }
}

/// Aufgenommene Fläche in globalen Punkten (x, y, Breite, Höhe).
fn area_points(app: &AppHandle, choice: &Choice) -> Option<(f32, f32, f32, f32)> {
    let displays = app.state::<CaptureState>().capturer.displays().ok()?;
    let d = displays.iter().find(|d| d.id == choice.display_id)?;
    let s = d.scale_factor.max(1.0);
    let (x, y, w, h) = choice.area;
    Some((
        d.bounds.x + x as f32 / s,
        d.bounds.y + y as f32 / s,
        w as f32 / s,
        h as f32 / s,
    ))
}

fn write_json<T: serde::Serialize>(path: &Path, value: &T) {
    match serde_json::to_vec(value) {
        Ok(bytes) => {
            if let Err(e) = std::fs::write(path, bytes) {
                log::warn!("{}: {e}", path.display());
            }
        }
        Err(e) => log::warn!("{}: {e}", path.display()),
    }
}

/// Webcam während der Aufnahme: ein/aus, gross/klein, Abschnitte.
struct WebcamControl {
    on: bool,
    /// Kamera läuft gerade an (im Hintergrund).
    starting: Option<std::thread::JoinHandle<fmshot_media::Result<()>>>,
    large: bool,
    log: SectionLog,
    layout: WebcamLayout,
    /// Startlage der Blase (physische Pixel des Monitors).
    bubble_rect: (f64, f64, f64, f64),
}

impl WebcamControl {
    fn mode(&self) -> WebcamMode {
        match (self.on, self.large) {
            (false, _) => WebcamMode::Hidden,
            (true, false) => WebcamMode::Overlay,
            (true, true) => WebcamMode::Large,
        }
    }

    /// Kamera im Hintergrund einschalten; [`Self::handle`] zeigt danach die Blase.
    fn start(&mut self, recording: &Arc<dyn Recording>) {
        let recording = recording.clone();
        self.starting = Some(std::thread::spawn(move || recording.set_webcam(true)));
    }

    /// Knöpfe der Leiste auswerten. `at` = Zeit in der Aufnahme (vor dem
    /// Start `None`: Dann zählt nur der Zustand zu Beginn).
    fn handle(
        &mut self,
        app: &AppHandle,
        flags: &Flags,
        recording: &Arc<dyn Recording>,
        choice: &Choice,
        at: Option<u32>,
    ) {
        if self.starting.as_ref().is_some_and(|h| h.is_finished())
            && let Some(handle) = self.starting.take()
        {
            match handle.join() {
                Ok(Ok(())) => {
                    self.on = true;
                    show_bubble(app, recording, choice, self);
                }
                Ok(Err(e)) => {
                    log::warn!("Webcam: {e}");
                    set_label(app, RecordAction::Webcam, t("video.webcam_on", &[]));
                    capture::show_error(app, &e.to_string());
                }
                Err(_) => log::error!("Webcam: Start abgestürzt"),
            }
        }
        // Während die Kamera anläuft, wartet ein Klick, bis sie läuft.
        if self.starting.is_none() && flags.toggle_webcam.swap(false, Ordering::AcqRel) {
            if self.on {
                if let Err(e) = recording.set_webcam(false) {
                    log::warn!("Webcam: {e}");
                }
                self.on = false;
                hide_bubble(app);
                set_label(app, RecordAction::Webcam, t("video.webcam_on", &[]));
            } else if camera_allowed(app) {
                set_label(app, RecordAction::Webcam, t("video.webcam_off", &[]));
                self.start(recording);
            }
        }
        if flags.toggle_large.swap(false, Ordering::AcqRel) && self.on {
            self.large = !self.large;
            let key = if self.large {
                "video.webcam_small"
            } else {
                "video.webcam_large"
            };
            set_label(app, RecordAction::WebcamLarge, t(key, &[]));
        }
        if let Some(at) = at {
            self.log.set(at, self.mode());
        }
    }
}

/// Kamera-Freigabe vor dem Einschalten; verweigert → Systemeinstellungen.
fn camera_allowed(app: &AppHandle) -> bool {
    match fmshot_media::camera_permission() {
        Permission::Granted => true,
        Permission::NotDetermined => fmshot_media::request_camera_permission(),
        Permission::Denied => {
            capture::open_privacy_settings(app, PrivacyPane::Camera);
            false
        }
    }
}

fn area_rect(choice: &Choice) -> Rect {
    let (x, y, w, h) = choice.area;
    Rect::new(x as f32, y as f32, w.max(1) as f32, h.max(1) as f32)
}

/// Startlage der Blase: wie die Webcam im fertigen Video nach den
/// Einstellungen läge, auf den aufgenommenen Bereich bezogen.
fn bubble_rect(layout: &WebcamLayout, choice: &Choice) -> (f64, f64, f64, f64) {
    let area = area_rect(choice);
    let frame = Size::new(area.width, area.height);
    let camera = Size::new(16.0, 9.0);
    let p = fmshot_video::layout::webcam_overlay(layout, frame, camera);
    (
        (area.x + p.rect.x) as f64,
        (area.y + p.rect.y) as f64,
        p.rect.width as f64,
        p.rect.height as f64,
    )
}

/// Lage der Blase beim Stopp → Lage und Grösse im Dokument.
fn webcam_layout(
    mut layout: WebcamLayout,
    choice: &Choice,
    bubble: Option<(f64, f64, f64, f64)>,
) -> WebcamLayout {
    if let Some((x, y, w, h)) = bubble {
        let rect = Rect::new(x as f32, y as f32, w as f32, h as f32);
        let (position, size) = fmshot_video::layout_from_bubble(area_rect(choice), rect);
        layout.position = position;
        layout.size = size;
    }
    layout
}

fn bubble_style(layout: &WebcamLayout) -> BubbleStyle {
    BubbleStyle {
        corner_radius: match layout.shape {
            WebcamShape::Circle => 0.5,
            _ => layout.corner_radius.clamp(0.0, 0.5) as f64,
        },
        border: layout.border.filter(|b| b.width > 0.0).map(|b| {
            let c = b.color;
            (
                b.width as f64,
                [
                    c.r as f64 / 255.0,
                    c.g as f64 / 255.0,
                    c.b as f64 / 255.0,
                    c.a as f64 / 255.0,
                ],
            )
        }),
        shadow: layout.shadow,
    }
}

/// Zeigt die Blase (beim ersten Mal wird sie angelegt).
fn show_bubble(
    app: &AppHandle,
    recording: &Arc<dyn Recording>,
    choice: &Choice,
    cam: &WebcamControl,
) {
    let (recording, display_id, rect) = (recording.clone(), choice.display_id, cam.bubble_rect);
    let (style, mirror) = (bubble_style(&cam.layout), cam.layout.mirror);
    let _ = app.run_on_main_thread(move || {
        BUBBLE.with(|b| {
            let mut b = b.borrow_mut();
            if let Some(bubble) = b.as_ref() {
                bubble.set_visible(true);
                return;
            }
            let Some(preview) = recording.webcam_preview(mirror) else {
                return;
            };
            // SAFETY: `preview` ist eine gültige Ebene von fmshot-media; die
            // Blase hält sie selbst fest.
            match unsafe {
                fmshot_overlay::open_webcam_bubble(display_id, rect, style, preview.as_ptr())
            } {
                Ok(bubble) => *b = Some(bubble),
                Err(e) => log::error!("Webcam-Blase: {e}"),
            }
        })
    });
}

fn hide_bubble(app: &AppHandle) {
    let _ = app.run_on_main_thread(|| {
        BUBBLE.with(|b| {
            if let Some(bubble) = b.borrow().as_ref() {
                bubble.set_visible(false);
            }
        })
    });
}

/// Schliesst die Blase und gibt ihre letzte Lage zurück.
fn close_bubble(app: &AppHandle) -> Option<(f64, f64, f64, f64)> {
    let (tx, rx) = mpsc::sync_channel(1);
    let _ = app.run_on_main_thread(move || {
        let frame = BUBBLE.with(|b| {
            b.borrow_mut().take().map(|bubble| {
                let frame = bubble.frame();
                bubble.close();
                frame
            })
        });
        let _ = tx.send(frame);
    });
    rx.recv_timeout(Duration::from_secs(2)).ok().flatten()
}

fn close_ui(app: &AppHandle) {
    let _ = app.run_on_main_thread(|| {
        UI.with(|u| {
            if let Some(ui) = u.borrow_mut().take() {
                ui.close();
            }
        })
    });
}

fn set_label(app: &AppHandle, action: RecordAction, text: String) {
    let _ = app.run_on_main_thread(move || {
        UI.with(|u| {
            if let Some(ui) = u.borrow().as_ref() {
                ui.set_label(action, &text);
            }
        })
    });
}

/// `webcam`: `None` = keine Kamera am System (keine Webcam-Knöpfe), sonst
/// ob sie zu Beginn läuft.
fn open_ui(app: &AppHandle, choice: &Choice, flags: &Flags, countdown: bool, webcam: Option<bool>) {
    let status = if countdown {
        t("video.countdown", &[("n", "3")])
    } else {
        t("video.recording", &[("time", "0:00"), ("levels", "")])
    };
    let (pause, stop, cancel) = (
        t("video.pause", &[]),
        t("video.stop", &[]),
        t("video.cancel", &[]),
    );
    let webcam_label = webcam.map(|on| {
        t(
            if on {
                "video.webcam_off"
            } else {
                "video.webcam_on"
            },
            &[],
        )
    });
    let large = webcam.map(|_| t("video.webcam_large", &[]));
    let (display_id, rect, flags) = (choice.display_id, choice.rect, flags.clone());
    let _ = app.run_on_main_thread(move || {
        let ui = fmshot_overlay::open_record_ui(
            display_id,
            rect,
            &RecordLabels {
                status: &status,
                pause: &pause,
                webcam: webcam_label.as_deref(),
                webcam_large: large.as_deref(),
                stop: &stop,
                cancel: &cancel,
            },
            Box::new(move |action| {
                let flag = match action {
                    RecordAction::Pause => &flags.toggle_pause,
                    RecordAction::Webcam => &flags.toggle_webcam,
                    RecordAction::WebcamLarge => &flags.toggle_large,
                    RecordAction::Stop => &flags.stop,
                    RecordAction::Cancel => &flags.cancel,
                };
                flag.store(true, Ordering::Release);
            }),
        );
        match ui {
            Ok(ui) => UI.with(|u| *u.borrow_mut() = Some(ui)),
            Err(e) => log::error!("Video-Leiste: {e}"),
        }
    });
}

fn set_status(app: &AppHandle, text: String) {
    let _ = app.run_on_main_thread(move || {
        UI.with(|u| {
            if let Some(ui) = u.borrow().as_ref() {
                ui.set_status(&text);
            }
        })
    });
}

/// „● 0:12  Mikro ▮▮▮▯▯“
fn status_text(recording: &dyn Recording, microphone: bool, system: bool) -> String {
    let time = format_duration(recording.elapsed());
    let levels = recording.levels();
    let mut meters = Vec::new();
    if microphone {
        meters.push(format!(
            "{} {}",
            t("video.mic", &[]),
            meter(levels.microphone)
        ));
    }
    if system && !microphone {
        meters.push(format!(
            "{} {}",
            t("video.system", &[]),
            meter(levels.system)
        ));
    }
    let key = if recording.is_paused() {
        "video.paused"
    } else {
        "video.recording"
    };
    t(key, &[("time", &time), ("levels", &meters.join("  "))])
}

fn meter(peak: f32) -> String {
    let on = fmshot_media::level_steps(peak, METER_STEPS);
    (0..METER_STEPS)
        .map(|i| if i < on { '▮' } else { '▯' })
        .collect()
}

fn format_duration(d: Duration) -> String {
    let s = d.as_secs();
    if s >= 3600 {
        format!("{}:{:02}:{:02}", s / 3600, (s / 60) % 60, s % 60)
    } else {
        format!("{}:{:02}", s / 60, s % 60)
    }
}

/// Spuren zusammensetzen, Standbild holen, in die Bibliothek legen und wie
/// nach einer Bildaufnahme weitermachen (Vorschau, Editor …).
fn finish(
    app: &AppHandle,
    slot: &VideoSlot,
    files: &RecordFiles,
    summary: fmshot_media::RecordSummary,
    scale: f32,
    doc: VideoDoc,
) {
    let video = slot.video_path();
    let started = std::time::Instant::now();
    let how = match compose(app, files, &doc, scale, &video) {
        Ok(stats) => format!(
            "zusammengesetzt ({} Bilder, {})",
            stats.frames, stats.adapter
        ),
        Err(e) => {
            log::error!("Video zusammensetzen: {e}");
            // Ohne Webcam ist besser als gar nichts.
            if let Err(e) = fmshot_media::export_mp4(files, &video) {
                log::error!("Video zusammenführen: {e}");
                if let Err(e) = std::fs::copy(&files.screen, &video) {
                    log::error!("Video kopieren: {e}");
                }
            }
            "ohne Webcam zusammengeführt".to_string()
        }
    };
    crate::services::timing(
        app,
        format!(
            "Video {}×{}, {:.1} s: {how} in {:.0} ms",
            summary.width,
            summary.height,
            summary.duration.as_secs_f64(),
            started.elapsed().as_secs_f64() * 1000.0
        ),
    );
    let poster = match fmshot_media::frame_at(&video, Duration::ZERO)
        .or_else(|_| fmshot_media::frame_at(&files.screen, Duration::ZERO))
    {
        Ok(p) => p,
        Err(e) => {
            capture::show_error(app, &e.to_string());
            return;
        }
    };
    let library = app.state::<CaptureState>().library.clone();
    let rec = match library.save_video(slot, &poster, scale, doc) {
        Ok(r) => r,
        Err(e) => {
            capture::show_error(app, &e.to_string());
            return;
        }
    };
    if crate::settings::get(app).copy_to_clipboard
        && let Err(e) = fmshot_clipboard::copy_file(&video)
    {
        log::warn!("Video in die Zwischenablage: {e}");
    }
    crate::services::after_capture(app, &rec, &poster, Mode::Video);
}

/// Setzt zusammen; der Fortschritt steht in der Leiste. Direkt nach der
/// Aufnahme gibt es noch keine Ebenen.
fn compose(
    app: &AppHandle,
    files: &RecordFiles,
    doc: &VideoDoc,
    scale: f32,
    out: &Path,
) -> Result<fmshot_video::ExportStats, String> {
    let mut shown = 0;
    let extras = crate::video_edit::extras(files, doc, scale, None);
    crate::video_edit::compose(files, doc, extras, out, &mut |p| {
        let percent = (p * 100.0) as u32;
        if percent >= shown + 5 {
            shown = percent;
            set_status(
                app,
                t("video.exporting", &[("percent", &percent.to_string())]),
            );
        }
        true
    })
}

fn now_ms() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dauer_lesbar() {
        assert_eq!(format_duration(Duration::from_secs(5)), "0:05");
        assert_eq!(format_duration(Duration::from_secs(754)), "12:34");
        assert_eq!(format_duration(Duration::from_secs(3725)), "1:02:05");
    }

    #[test]
    fn pegel_als_balken() {
        assert_eq!(meter(0.0), "▯▯▯▯▯");
        assert_eq!(meter(1.0), "▮▮▮▮▮");
    }
}
