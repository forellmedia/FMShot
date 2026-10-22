//! Video-Aufnahme (ADR 0011): Ziel wählen (Bereich, Fenster, Bildschirm
//! oder nur Webcam), Countdown, Aufnahmeleiste mit Pause/Stopp und Webcam, danach die Spuren
//! zusammensetzen und in die Bibliothek legen. Die Aufnahme macht
//! `nabshot-media`, Layout und Export `nabshot-video`; hier wird nur verdrahtet.

use std::cell::RefCell;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use nabshot_capture::{PrivacyPane, display_at};
use nabshot_core::{
    CameraFormat, CheckpointDrawing, Document, InputSample, InputTrack, LayerTime, Point,
    RecordingCheckpoint, Rect, Size, VideoDoc, WebcamBackground, WebcamLayout, WebcamMode,
    WebcamSection, WebcamShape, WebcamTrack,
};
use nabshot_hotkeys::{Action, Platform};
use nabshot_library::VideoSlot;
use nabshot_media::{
    MediaError, Permission, RecordFiles, RecordOptions, RecordSummary, RecordTarget, Recording,
};
use nabshot_overlay::{
    BubbleStyle, DrawEvent, DrawSurface, DrawTool, OverlayHandle, OverlayMode, OverlayResult,
    OwnClickWatch, RecordAction, RecordLabels, RecordUi, WebcamBubble, WindowTarget,
};
use nabshot_settings::{DrawMode, VideoSettings, VideoTarget};
use nabshot_video::SectionLog;
use nabshot_video::sketch::{Clock, LiveSketch, SKETCH_COLORS, SketchItem, SketchTool};
use tauri::{AppHandle, Emitter, Manager};
use tauri_plugin_dialog::{
    DialogExt, MessageDialogButtons, MessageDialogKind, MessageDialogResult,
};

use crate::capture::{self, CaptureState};
use crate::i18n::t;

/// Wie oft die Leiste (Zeit, Pegel) nachgeführt wird.
const TICK: Duration = Duration::from_millis(200);
/// So oft (in Takten) wird der freie Speicher geprüft (~5 s).
const DISK_TICKS: u32 = 25;
/// Darunter warnt die Leiste.
const DISK_LOW: u64 = 2_000_000_000;
/// Darunter wird die Aufnahme beendet und gesichert, bevor die Platte voll ist.
const DISK_CRITICAL: u64 = 500_000_000;
/// Stufen der Pegelanzeige.
const METER_STEPS: u32 = 5;

/// Laufende Aufnahme – erlaubt „Stopp“ per Tastenkürzel.
#[derive(Default)]
pub struct VideoState {
    session: Mutex<Option<Flags>>,
    /// Zwischen Tastendruck und Aufnahmebeginn (Auswahl, Freigaben).
    starting: AtomicBool,
    /// Von der Aufnahme bis das Video gesichert ist (Beenden wartet darauf).
    active: AtomicBool,
}

#[derive(Clone, Default)]
struct Flags {
    stop: Arc<AtomicBool>,
    cancel: Arc<AtomicBool>,
    toggle_pause: Arc<AtomicBool>,
    toggle_draw: Arc<AtomicBool>,
    toggle_webcam: Arc<AtomicBool>,
    toggle_large: Arc<AtomicBool>,
}

thread_local! {
    static UI: RefCell<Option<RecordUi>> = const { RefCell::new(None) };
    static OVERLAY: RefCell<Option<OverlayHandle>> = const { RefCell::new(None) };
    static BUBBLE: RefCell<Option<WebcamBubble>> = const { RefCell::new(None) };
    static SURFACE: RefCell<Option<DrawSurface>> = const { RefCell::new(None) };
    static OWN_CLICKS: RefCell<Option<OwnClickWatch>> = const { RefCell::new(None) };
    /// Kürzel für Zeichnen ein/aus, steht auf dem Knopf der Leiste.
    static DRAW_SHORTCUT: RefCell<String> = const { RefCell::new(String::new()) };
}

/// Was nach der Auswahl aufgenommen wird.
#[derive(Debug, Clone, Copy)]
struct Choice {
    /// `None` = nur Webcam.
    target: Option<RecordTarget>,
    display_id: u32,
    /// Rahmen um den Bereich (physische Pixel des Monitors).
    rect: Option<(u32, u32, u32, u32)>,
    /// Aufgenommene Fläche (physische Pixel des Monitors) – Bezug für die
    /// Lage der Webcam-Blase.
    area: (u32, u32, u32, u32),
    /// Pixel des Bildschirmpuffers je Punkt (für Rahmen, Blase, Zeichenfläche).
    scale: f32,
    /// Pixel des Videos je Pixel des Puffers (≤ 1, `nabshot_media::record_ratio`).
    ratio: f32,
}

impl Choice {
    /// Pixel des Videos je Punkt.
    fn video_scale(&self) -> f32 {
        self.scale * self.ratio
    }
}

/// Läuft schon ein Video? Dann beendet das Kürzel es.
pub fn stop_running(app: &AppHandle) -> bool {
    let state = app.state::<VideoState>();
    let guard = state.session.lock().unwrap_or_else(|e| e.into_inner());
    match guard.as_ref() {
        Some(flags) => {
            log::info!("Video: Stopp angefordert");
            flags.stop.store(true, Ordering::Release);
            true
        }
        None => false,
    }
}

/// Was eine Tastenkombination während der Aufnahme umschaltet.
#[derive(Debug, Clone, Copy)]
pub enum Toggle {
    Draw,
    Pause,
}

/// Tastenkombination „Zeichnen“ bzw. „Pause“: wirkt nur während einer
/// Aufnahme.
pub fn toggle(app: &AppHandle, what: Toggle) {
    let state = app.state::<VideoState>();
    let guard = state.session.lock().unwrap_or_else(|e| e.into_inner());
    if let Some(flags) = guard.as_ref() {
        log::debug!("Video: Kürzel {what:?}");
        let flag = match what {
            Toggle::Draw => &flags.toggle_draw,
            Toggle::Pause => &flags.toggle_pause,
        };
        flag.store(true, Ordering::Release);
    }
}

/// Startet eine Video-Aufnahme mit dem Ziel aus den Einstellungen bzw.
/// `target`. Die Bildschirm-Freigabe hat `capture::start` schon geprüft.
pub fn start(app: &AppHandle, target: Option<VideoTarget>) {
    let requested = Instant::now();
    let state = app.state::<VideoState>();
    if state.starting.swap(true, Ordering::AcqRel) {
        log::debug!("Video: Start läuft schon – ignoriert");
        return;
    }
    let settings = crate::settings::get(app).video;
    let target = target.unwrap_or(settings.target);
    log_start(&format!("{target:?}"), &settings);
    let app = app.clone();
    std::thread::spawn(move || {
        let microphone = settings.microphone
            && match nabshot_media::microphone_permission() {
                Permission::Granted => true,
                // Systemfenster; ohne Zustimmung ohne Mikrofon weiter.
                Permission::NotDetermined => {
                    asked("Mikrofon", nabshot_media::request_microphone_permission())
                }
                Permission::Denied => {
                    log::warn!("Video: Mikrofon-Freigabe fehlt – Systemeinstellungen");
                    done_starting(&app);
                    capture::open_privacy_settings(&app, PrivacyPane::Microphone);
                    return;
                }
            };
        let webcam = settings.webcam
            && !nabshot_media::cameras().is_empty()
            && match nabshot_media::camera_permission() {
                Permission::Granted => true,
                Permission::NotDetermined => {
                    asked("Kamera", nabshot_media::request_camera_permission())
                }
                Permission::Denied => {
                    log::warn!("Video: Kamera-Freigabe fehlt – Systemeinstellungen");
                    done_starting(&app);
                    capture::open_privacy_settings(&app, PrivacyPane::Camera);
                    return;
                }
            };
        if settings.webcam && !webcam {
            log::info!("Video: ohne Webcam (keine Kamera oder Freigabe)");
        }
        select(&app, target, move |app, choice| {
            let app = app.clone();
            std::thread::spawn(move || run(&app, choice, microphone, webcam, settings, requested));
        });
    });
}

/// Start mit den Einstellungen festhalten (ohne Gerätenamen).
fn log_start(target: &str, settings: &VideoSettings) {
    log::info!(
        "Video: Start angefordert – Ziel {target}, {} fps, Mikrofon {}, Systemton {}, \
         Webcam {}, Countdown {}, Zeichnen {:?}, {:?}",
        settings.fps,
        settings.microphone,
        settings.system_audio,
        settings.webcam,
        settings.countdown,
        settings.draw_mode,
        settings.encoding
    );
    log::debug!(
        "Video: Mikrofon-ID {:?}, Kamera-ID {:?}",
        settings.microphone_id,
        settings.camera_id
    );
}

/// Antwort auf die Freigabe-Frage des Systems festhalten.
fn asked(what: &str, granted: bool) -> bool {
    log::info!("Video: {what}-Freigabe erfragt – erteilt: {granted}");
    granted
}

/// Startet eine Aufnahme nur mit der Webcam (Tab *Webcam*). Die Blase
/// erscheint mitten auf dem Bildschirm unter der Maus.
pub fn start_webcam(app: &AppHandle) {
    let requested = Instant::now();
    let state = app.state::<VideoState>();
    if state.starting.swap(true, Ordering::AcqRel) {
        log::debug!("Video: Start läuft schon – ignoriert");
        return;
    }
    let settings = crate::settings::get(app).video;
    log_start(
        &format!("nur Webcam {:?}", settings.camera.format),
        &settings,
    );
    let app = app.clone();
    std::thread::spawn(move || {
        if nabshot_media::cameras().is_empty() {
            log::warn!("Video: keine Kamera gefunden");
            done_starting(&app);
            capture::show_error(&app, &t("video.no_camera", &[]));
            return;
        }
        let camera = match nabshot_media::camera_permission() {
            Permission::Granted => true,
            Permission::NotDetermined => {
                asked("Kamera", nabshot_media::request_camera_permission())
            }
            Permission::Denied => {
                log::warn!("Video: Kamera-Freigabe fehlt – Systemeinstellungen");
                done_starting(&app);
                capture::open_privacy_settings(&app, PrivacyPane::Camera);
                return;
            }
        };
        if !camera {
            log::info!("Video: ohne Kamera-Freigabe – kein Start");
            done_starting(&app);
            return;
        }
        let microphone = settings.microphone
            && match nabshot_media::microphone_permission() {
                Permission::Granted => true,
                Permission::NotDetermined => {
                    asked("Mikrofon", nabshot_media::request_microphone_permission())
                }
                Permission::Denied => {
                    log::warn!("Video: Mikrofon-Freigabe fehlt – Systemeinstellungen");
                    done_starting(&app);
                    capture::open_privacy_settings(&app, PrivacyPane::Microphone);
                    return;
                }
            };
        let Some(choice) = camera_choice(&app, settings.camera.format) else {
            log::error!("Video: kein Monitor für die Webcam-Blase");
            done_starting(&app);
            return;
        };
        run(&app, choice, microphone, true, settings, requested);
    });
}

/// Startlage der Blase bei „nur Webcam“, in Punkten (Breite, Höhe).
fn camera_bubble_points(format: CameraFormat) -> (f32, f32) {
    match format {
        CameraFormat::Landscape => (560.0, 315.0),
        CameraFormat::Portrait => (270.0, 480.0),
        CameraFormat::Square => (400.0, 400.0),
    }
}

/// „Nur Webcam“: Monitor unter der Maus, Blase in seiner Mitte.
fn camera_choice(app: &AppHandle, format: CameraFormat) -> Option<Choice> {
    let state = app.state::<CaptureState>();
    let displays = state
        .capturer
        .displays()
        .map_err(|e| log::error!("Video: Monitore: {e}"))
        .ok()?;
    let display = state
        .capturer
        .cursor_position()
        .and_then(|c| display_at(&displays, c))
        .or_else(|| displays.iter().find(|d| d.is_primary))?;
    let s = display.scale_factor.max(1.0);
    let (w, h) = camera_bubble_points(format);
    let (w, h) = (w * s, h * s);
    let x = (display.bounds.width * s - w) / 2.0;
    let y = (display.bounds.height * s - h) / 2.0;
    Some(Choice {
        target: None,
        display_id: display.id,
        rect: None,
        area: (x.max(0.0) as u32, y.max(0.0) as u32, w as u32, h as u32),
        scale: display.scale_factor,
        ratio: 1.0,
    })
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
            log::error!("Video: Monitore: {e}");
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
                log::error!("Video: kein Monitor gefunden");
                done_starting(app);
                return;
            };
            let s = display.scale_factor.max(1.0);
            let area = (
                0,
                0,
                (display.bounds.width * s) as u32,
                (display.bounds.height * s) as u32,
            );
            let choice = Choice {
                target: Some(RecordTarget::Display {
                    display_id: display.id,
                }),
                display_id: display.id,
                // Rahmen auch beim Vollbild: am Rand des Monitors.
                rect: Some(area),
                area,
                scale: display.scale_factor,
                ratio: nabshot_media::record_ratio(display.id),
            };
            let a = app.clone();
            main_thread(app, "Ziel", move || then(&a, choice));
        }
        VideoTarget::Region => {
            let a = app.clone();
            main_thread(app, "Auswahl", move || {
                let app_done = a.clone();
                log::debug!("Video: Auswahl-Overlay (Bereich) geöffnet");
                let result = nabshot_overlay::open(
                    OverlayMode::Region,
                    Vec::new(),
                    Box::new(move |result| {
                        OVERLAY.with(|o| o.borrow_mut().take());
                        let OverlayResult::Selected(sel) = result else {
                            log::info!("Video: Auswahl abgebrochen");
                            done_starting(&app_done);
                            return;
                        };
                        then(
                            &app_done,
                            Choice {
                                target: Some(RecordTarget::Region {
                                    display_id: sel.display_id,
                                    x: sel.x,
                                    y: sel.y,
                                    width: sel.width,
                                    height: sel.height,
                                }),
                                display_id: sel.display_id,
                                rect: Some((sel.x, sel.y, sel.width, sel.height)),
                                area: (sel.x, sel.y, sel.width, sel.height),
                                scale: scale_of(sel.display_id),
                                ratio: nabshot_media::record_ratio(sel.display_id),
                            },
                        );
                    }),
                );
                open_result(&a, result);
            });
        }
        VideoTarget::Window => {
            let windows = match state.capturer.windows(nabshot_capture::OwnWindows::Hidden) {
                Ok(w) => w,
                Err(e) => {
                    log::error!("Video: Fenster: {e}");
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
            main_thread(app, "Auswahl", move || {
                let app_done = a.clone();
                log::debug!(
                    "Video: Auswahl-Overlay (Fenster, {}) geöffnet",
                    targets.len()
                );
                let result = nabshot_overlay::open(
                    OverlayMode::Window,
                    targets,
                    Box::new(move |result| {
                        OVERLAY.with(|o| o.borrow_mut().take());
                        let OverlayResult::Window(id) = result else {
                            log::info!("Video: Auswahl abgebrochen");
                            done_starting(&app_done);
                            return;
                        };
                        let Some(w) = windows.iter().find(|w| w.id == id) else {
                            log::warn!("Video: gewähltes Fenster {id} nicht mehr in der Liste");
                            done_starting(&app_done);
                            return;
                        };
                        let b = w.bounds;
                        let center = (b.x + b.width / 2.0, b.y + b.height / 2.0);
                        let display = display_at(&displays, center)
                            .or_else(|| displays.iter().find(|d| d.is_primary));
                        let (display_id, scale, rect) = match display {
                            Some(d) => (d.id, d.scale_factor, Some(window_pixels(b, d))),
                            None => {
                                log::warn!("Video: Fenster {id} auf keinem Monitor – ohne Rahmen");
                                (0, 2.0, None)
                            }
                        };
                        then(
                            &app_done,
                            Choice {
                                target: Some(RecordTarget::Window { window_id: id }),
                                display_id,
                                rect,
                                area: rect.unwrap_or((0, 0, 1920, 1080)),
                                scale,
                                ratio: nabshot_media::record_ratio(display_id),
                            },
                        );
                    }),
                );
                open_result(&a, result);
            });
        }
    }
}

/// Fensterlage (globale Punkte) → physische Pixel des Monitors, auf ihn
/// begrenzt.
fn window_pixels(b: Rect, d: &nabshot_capture::Display) -> (u32, u32, u32, u32) {
    let s = d.scale_factor.max(1.0);
    let x0 = ((b.x - d.bounds.x) * s).max(0.0);
    let y0 = ((b.y - d.bounds.y) * s).max(0.0);
    let x1 = ((b.right() - d.bounds.x) * s).min(d.bounds.width * s);
    let y1 = ((b.bottom() - d.bounds.y) * s).min(d.bounds.height * s);
    (
        x0 as u32,
        y0 as u32,
        (x1 - x0).max(0.0) as u32,
        (y1 - y0).max(0.0) as u32,
    )
}

fn open_result(app: &AppHandle, result: Result<OverlayHandle, nabshot_overlay::OverlayError>) {
    match result {
        Ok(handle) => OVERLAY.with(|o| *o.borrow_mut() = Some(handle)),
        Err(e) => {
            log::error!("Video: Auswahl-Overlay: {e}");
            done_starting(app);
            capture::show_error(app, &e.to_string());
        }
    }
}

/// `requested` = Zeitpunkt des Tastendrucks (für die Zeit bis zum Start).
fn run(
    app: &AppHandle,
    choice: Choice,
    microphone: bool,
    webcam: bool,
    settings: VideoSettings,
    requested: Instant,
) {
    let _active = Active::start(app);
    let (x, y, w, h) = choice.area;
    match choice.target {
        Some(target) => log::info!(
            "Video: Ziel {target:?}, Monitor {}, Fläche {w}×{h} bei {x},{y}, Skalierung {}, Verhältnis {:.2}",
            choice.display_id,
            choice.scale,
            choice.ratio
        ),
        None => log::info!(
            "Video: nur Webcam, Monitor {}, Blase {w}×{h}, Skalierung {}",
            choice.display_id,
            choice.scale
        ),
    }
    let library = app.state::<CaptureState>().library.clone();
    let slot = match library.reserve_video(now_ms()) {
        Ok(s) => s,
        Err(e) => {
            log::error!("Video: Platz in der Bibliothek: {e}");
            done_starting(app);
            capture::show_error(app, &e.to_string());
            return;
        }
    };
    let camera_only = choice.target.is_none();
    // Mit Kamera am System lässt sich die Webcam jederzeit zuschalten.
    let has_camera = !nabshot_media::cameras().is_empty();
    let files = RecordFiles {
        screen: (!camera_only).then(|| slot.screen_path()),
        system_audio: (settings.system_audio && !camera_only).then(|| slot.system_audio_path()),
        microphone: microphone.then(|| slot.microphone_path()),
        webcam: has_camera.then(|| slot.webcam_path()),
        faces: has_camera.then(|| slot.faces_path()),
    };
    log::debug!(
        "Video: {} – Spuren: Bildschirm {}, Systemton {}, Mikrofon {}, Webcam {}",
        slot.id,
        files.screen.is_some(),
        files.system_audio.is_some(),
        files.microphone.is_some(),
        files.webcam.is_some()
    );
    // Kurz warten, bis das Auswahl-Overlay verschwunden ist.
    std::thread::sleep(Duration::from_millis(80));
    let opening = Instant::now();
    let recording = nabshot_media::start_recording(RecordOptions {
        target: choice.target,
        files: files.clone(),
        fps: settings.fps,
        microphone_id: settings.microphone_id.clone(),
        // Nabshot zeichnet den Zeiger selbst aus der Mausspur (Etappe 5).
        show_cursor: false,
        camera_id: settings.camera_id.clone(),
    });
    done_starting(app);
    let recording: Arc<dyn Recording> = match recording {
        Ok(r) => {
            log::debug!(
                "Video: Aufnahme bereit in {} ms",
                opening.elapsed().as_millis()
            );
            Arc::from(r)
        }
        Err(e) => {
            log::error!("Video: Aufnahme starten: {e}");
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
    let layout = if camera_only {
        settings.camera.layout()
    } else {
        settings.webcam_layout
    };
    let mut cam = WebcamControl {
        on: false,
        starting: None,
        since: Instant::now(),
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
        // Bei „nur Webcam“ ohne Knöpfe für die Webcam.
        (has_camera && !camera_only).then_some(webcam),
    );
    // Zwischenstand für den Fall eines Absturzes (ADR 0012).
    let mut checkpoint = Checkpoint::new(
        slot.checkpoint_path(),
        RecordingCheckpoint {
            fps: if camera_only { 30 } else { settings.fps },
            scale: if camera_only {
                1.0
            } else {
                choice.video_scale()
            },
            camera_only: camera_only.then_some(settings.camera.format),
            webcam_layout: cam.layout,
            ..RecordingCheckpoint::default()
        },
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
                if flags.cancel.load(Ordering::Acquire)
                    || flags.stop.load(Ordering::Acquire)
                    || recording.failure().is_some()
                {
                    break;
                }
            }
        }
    }
    // Nur Webcam: Ohne laufende Kamera gäbe es kein erstes Bild.
    if camera_only {
        while cam.starting.is_some() && !flags.cancel.load(Ordering::Acquire) {
            cam.handle(app, &flags, &recording, &choice, None);
            std::thread::sleep(Duration::from_millis(20));
        }
        if !cam.on {
            flags.cancel.store(true, Ordering::Release);
        }
    }
    if !flags.cancel.load(Ordering::Acquire) {
        recording.begin();
        cam.log.set(0, cam.mode());
        log::info!(
            "Video: Aufnahme gestartet ({} ms nach dem Kürzel)",
            requested.elapsed().as_millis()
        );
    }
    let mouse =
        (!camera_only).then(|| MouseLog::start(app, &recording, &choice, slot.input_log_path()));
    // Live zeichnen gibt es nur mit Bildschirm.
    let draw_once = settings.draw_mode == DrawMode::Once;
    let sketch = (!camera_only).then(|| Sketch::open(app, &choice, &recording, draw_once));

    // Das aufgenommene Fenster: Der Rahmen folgt ihm.
    let window = match choice.target {
        Some(RecordTarget::Window { window_id }) => app
            .state::<CaptureState>()
            .capturer
            .displays()
            .map_err(|e| log::warn!("Video: Monitore: {e} – Rahmen folgt dem Fenster nicht"))
            .ok()
            .and_then(|ds| ds.into_iter().find(|d| d.id == choice.display_id))
            .map(|d| (window_id, d)),
        _ => None,
    };
    let mut frame = choice.rect;
    let mut failure = recording.failure();
    let mut disk = DiskState::Ok;
    let mut ticks = 0u32;
    let mut moves = 0u32;
    while failure.is_none()
        && !flags.stop.load(Ordering::Acquire)
        && !flags.cancel.load(Ordering::Acquire)
    {
        if flags.toggle_pause.swap(false, Ordering::AcqRel) {
            let at = format_duration(recording.elapsed());
            let label = if recording.is_paused() {
                recording.resume();
                log::info!("Video: fortgesetzt bei {at}");
                t("video.pause", &[])
            } else {
                recording.pause();
                log::info!("Video: Pause bei {at}");
                t("video.resume", &[])
            };
            set_label(app, RecordAction::Pause, label);
        }
        if let Some(sketch) = &sketch {
            if flags.toggle_draw.swap(false, Ordering::AcqRel) {
                main_thread(app, "Zeichnen", || {
                    with_surface(|s| set_drawing(s, !s.is_active()));
                });
            }
            sketch.tick(app);
        }
        let at = recording.elapsed().as_millis().min(u32::MAX as u128) as u32;
        cam.handle(app, &flags, &recording, &choice, Some(at));
        if let Some(e) = recording.failure() {
            failure = Some(e);
            break;
        }
        if ticks.is_multiple_of(DISK_TICKS) {
            let free = nabshot_media::free_space(&slot.dir);
            let was = disk;
            disk = free.map_or(DiskState::Ok, disk_state);
            if disk != was {
                log::warn!(
                    "Video: Speicher {disk:?} ({} MB frei)",
                    free.unwrap_or(0) / 1_000_000
                );
            }
            if disk == DiskState::Critical {
                failure = Some(t("video.disk_full", &[]));
                break;
            }
        }
        ticks = ticks.wrapping_add(1);
        if let Some((id, display)) = &window
            && let Some(bounds) = app.state::<CaptureState>().capturer.window_bounds(*id)
        {
            let rect = window_pixels(bounds, display);
            if frame != Some(rect) {
                frame = Some(rect);
                moves += 1;
                set_rect(app, rect);
            }
        }
        checkpoint.update(|cp| {
            cp.webcam_sections = cam.log.clone().into_sections();
            if let Some(sketch) = &sketch {
                cp.drawings = sketch.done();
            }
        });
        let (time, detail) = status_text(recording.as_ref(), microphone, settings.system_audio);
        let detail = match disk {
            DiskState::Low => format!("{}  {detail}", t("video.disk_low", &[])),
            _ => detail,
        };
        set_time(app, time, detail, !recording.is_paused());
        std::thread::sleep(TICK);
    }
    // Abgebrochen: Bis hierher ist alles gesichert – das zeigt die Leiste,
    // während das Video zusammengesetzt wird.
    let saved_until = format_duration(recording.elapsed());
    let ending = Instant::now();
    if let Some(e) = &failure {
        log::error!("Video: Aufnahme fehlgeschlagen nach {saved_until}: {e}");
        set_failed(app, t("video.failed", &[("time", &saved_until)]));
    } else if flags.cancel.load(Ordering::Acquire) {
        log::info!("Video: abgebrochen nach {saved_until}");
    } else {
        log::info!("Video: gestoppt nach {saved_until}");
    }
    if moves > 0 {
        log::debug!("Video: Rahmen folgte dem Fenster {moves}×");
    }

    *app.state::<VideoState>()
        .session
        .lock()
        .unwrap_or_else(|e| e.into_inner()) = None;
    // Lage der Blase = Startlayout der Webcam.
    let bubble = close_bubble(app);
    let drawings = sketch.map(|s| s.close(app)).unwrap_or_default();

    if flags.cancel.load(Ordering::Acquire) {
        close_ui(app);
        recording.cancel();
        library.discard_video(&slot);
        log::info!("Video: {} verworfen", slot.id);
        return;
    }
    let samples = mouse.map(MouseLog::finish);
    let stopping = Instant::now();
    let stopped = recording.stop();
    let stop_ms = stopping.elapsed().as_millis();
    match stopped {
        Ok(summary) => {
            log::info!(
                "Video: {} aufgenommen, {}×{}, Stopp {stop_ms} ms",
                format_duration(summary.duration),
                summary.width,
                summary.height
            );
            if let Some(samples) = samples {
                log::debug!("Video: Mausspur {} Proben", samples.len());
                write_json(&slot.input_path(), &InputTrack { samples });
            }
            if let Some(e) = &failure {
                log::warn!("Video endete vorzeitig: {e}");
            }
            log::debug!(
                "Video: {} Zeichnungen, Webcam-Abschnitte {}",
                drawings.len(),
                cam.log.clone().into_sections().len()
            );
            let webcam_file = files.webcam.as_deref().filter(|p| p.exists());
            let webcam = if camera_only {
                webcam_file.map(|_| WebcamTrack {
                    layout: cam.layout,
                    sections: vec![WebcamSection {
                        start_ms: 0,
                        mode: WebcamMode::Large,
                    }],
                })
            } else {
                webcam_file
                    .filter(|_| cam.log.shows_webcam())
                    .map(|_| WebcamTrack {
                        layout: webcam_layout(cam.layout, &choice, bubble),
                        sections: cam.log.into_sections(),
                    })
            };
            let doc = VideoDoc {
                duration_ms: summary.duration.as_millis().min(u32::MAX as u128) as u32,
                // Die Kamera liefert 30 Bilder je Sekunde.
                fps: if camera_only { 30 } else { settings.fps },
                webcam,
                camera_only: camera_only.then_some(settings.camera.format),
                ..VideoDoc::default()
            };
            // Das Kamerabild hat keine Bildschirm-Skalierung.
            let scale = if camera_only {
                1.0
            } else {
                choice.video_scale()
            };
            let saved = format_duration(summary.duration);
            let ok = finish(
                app,
                &slot,
                &files,
                summary,
                scale,
                doc,
                drawings,
                Origin::Live,
            );
            if ok {
                log::info!(
                    "Video: fertig {} ms nach dem Stopp",
                    ending.elapsed().as_millis()
                );
            }
            if ok && failure.is_some() {
                let msg = t("video.failed_saved", &[("time", &saved)]);
                main_thread(app, "Hinweis", move || nabshot_overlay::show_toast(&msg));
            }
        }
        Err(MediaError::Empty) => {
            log::warn!("Video: leer – verworfen");
            library.discard_video(&slot);
            if let Some(e) = failure {
                capture::show_error(app, &e);
            }
        }
        Err(e) => {
            log::error!("Video: Aufnahme beenden: {e}");
            library.discard_video(&slot);
            capture::show_error(app, &e.to_string());
        }
    }
    close_ui(app);
}

/// Im Haupt-Thread ausführen; klappt das nicht (App endet), festhalten.
fn main_thread(app: &AppHandle, what: &str, f: impl FnOnce() + Send + 'static) {
    if let Err(e) = app.run_on_main_thread(f) {
        log::warn!("Video: {what}: {e}");
    }
}

/// Live zeichnen (ADR 0011): Die Maus kommt im Haupt-Thread an, das
/// Verschwinden prüft der Aufnahme-Thread – beide teilen sich den Zustand.
struct Sketch {
    state: Mutex<LiveSketch>,
    recording: Arc<dyn Recording>,
    started: Instant,
    /// Ausgabepixel je Pixel der Aufnahme fürs Bild auf dem Bildschirm
    /// (Punkte reichen, das fertige Video zeichnet in voller Grösse).
    render_scale: f32,
    /// Pixel des Videos je Pixel der Zeichenfläche ([`Choice::ratio`]).
    ratio: f32,
    /// Nach jeder Markierung zurück (Einstellung [`DrawMode::Once`]).
    once: bool,
}

/// SF Symbol je Werkzeug.
fn tool_symbol(tool: SketchTool) -> &'static str {
    match tool {
        SketchTool::Arrow => "arrow.up.right",
        SketchTool::Line => "line.diagonal",
        SketchTool::Rectangle => "rectangle",
        SketchTool::Ellipse => "circle",
        SketchTool::Pen => "scribble",
        SketchTool::Step => "1.circle",
        SketchTool::Highlight => "highlighter",
    }
}

impl Sketch {
    fn open(
        app: &AppHandle,
        choice: &Choice,
        recording: &Arc<dyn Recording>,
        once: bool,
    ) -> Arc<Self> {
        // Gezeichnet wird in Pixeln des Videos; auf dem Bildschirm in Punkten.
        let (_, _, w, h) = choice.area;
        let size = (
            (w as f32 * choice.ratio).round() as u32,
            (h as f32 * choice.ratio).round() as u32,
        );
        let sketch = Arc::new(Self {
            state: Mutex::new(LiveSketch::new(size, choice.video_scale())),
            recording: recording.clone(),
            started: Instant::now(),
            render_scale: 1.0 / choice.video_scale().max(1.0),
            ratio: choice.ratio,
            once,
        });
        let (s, display_id, area) = (sketch.clone(), choice.display_id, choice.area);
        main_thread(app, "Zeichenfläche", move || {
            let tools: Vec<DrawTool<'_>> = SketchTool::ALL
                .iter()
                .map(|t| DrawTool {
                    symbol: tool_symbol(*t),
                    key: t.key(),
                })
                .collect();
            let colors: Vec<[f64; 4]> = SKETCH_COLORS
                .iter()
                .map(|c| {
                    [
                        c.r as f64 / 255.0,
                        c.g as f64 / 255.0,
                        c.b as f64 / 255.0,
                        1.0,
                    ]
                })
                .collect();
            let handler = s.clone();
            match nabshot_overlay::open_draw_surface(
                display_id,
                area,
                &tools,
                &colors,
                Box::new(move |e| handler.on_event(e)),
            ) {
                Ok(surface) => {
                    log::debug!("Video: Zeichenfläche geöffnet ({}×{})", area.2, area.3);
                    surface.set_selected(0, 0);
                    SURFACE.with(|x| *x.borrow_mut() = Some(surface));
                }
                Err(e) => log::error!("Zeichenfläche: {e}"),
            }
        });
        sketch
    }

    fn clock(&self) -> Clock {
        Clock {
            rec_ms: self.recording.elapsed().as_millis().min(u32::MAX as u128) as u32,
            wall_ms: self.started.elapsed().as_millis() as u64,
        }
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, LiveSketch> {
        self.state.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Maus und Tasten der Zeichenfläche (Haupt-Thread).
    fn on_event(&self, event: DrawEvent) {
        let clock = self.clock();
        let at = |x: f32, y: f32| Point::new(x * self.ratio, y * self.ratio);
        let mut s = self.lock();
        match event {
            DrawEvent::Tool(i) => {
                if let Some(tool) = SketchTool::ALL.get(i) {
                    s.tool = *tool;
                }
            }
            DrawEvent::Color(i) => {
                if let Some(color) = SKETCH_COLORS.get(i) {
                    s.color = *color;
                }
            }
            DrawEvent::Press { x, y } => s.press(at(x, y), clock),
            DrawEvent::Drag { x, y, shift } => s.drag(at(x, y), shift),
            DrawEvent::Release { x, y, shift } => {
                if s.release(at(x, y), shift, clock) && self.once {
                    with_surface(|surface| set_drawing(surface, false));
                }
            }
            // Esc entfernt die Zeichnungen, ein zweites Esc beendet das Zeichnen.
            DrawEvent::Escape => {
                if !s.clear(clock) {
                    with_surface(|surface| set_drawing(surface, false));
                }
            }
            DrawEvent::Exit => with_surface(|surface| set_drawing(surface, false)),
        }
        let tool = SketchTool::ALL
            .iter()
            .position(|t| *t == s.tool)
            .unwrap_or(0);
        let color = SKETCH_COLORS
            .iter()
            .position(|c| *c == s.color)
            .unwrap_or(0);
        let image = s.render(self.render_scale).map(Arc::new);
        drop(s);
        with_surface(|surface| {
            surface.set_selected(tool, color);
            surface.set_image(image);
        });
    }

    /// Abgelaufene Zeichnungen entfernen (Aufnahme-Thread).
    fn tick(&self, app: &AppHandle) {
        let changed = self.lock().tick(self.clock());
        if changed {
            main_thread(app, "Zeichenfläche", || {
                with_surface(|s| s.set_image(None))
            });
        }
    }

    /// Fertige Zeichnungen bis jetzt (Zwischenstand).
    fn done(&self) -> Vec<CheckpointDrawing> {
        self.lock()
            .done()
            .iter()
            .map(|d| CheckpointDrawing {
                kind: d.kind.clone(),
                opacity: d.opacity,
                start_ms: d.start_ms,
                end_ms: d.end_ms,
            })
            .collect()
    }

    /// Fläche schliessen und die Zeichnungen als Ebenen zurückgeben.
    fn close(&self, app: &AppHandle) -> Vec<SketchItem> {
        let (tx, rx) = mpsc::sync_channel(1);
        main_thread(app, "Zeichenfläche", move || {
            SURFACE.with(|s| {
                if let Some(surface) = s.borrow_mut().take() {
                    surface.close();
                    log::debug!("Video: Zeichenfläche geschlossen");
                }
            });
            // Wartet keiner mehr (Zeitüberschreitung), ist das egal.
            let _ = tx.send(());
        });
        if let Err(e) = rx.recv_timeout(Duration::from_secs(2)) {
            log::warn!("Video: Zeichenfläche schliessen: {e}");
        }
        let end = self.clock().rec_ms;
        self.lock().finish(end)
    }
}

fn with_surface(f: impl FnOnce(&DrawSurface)) {
    SURFACE.with(|s| {
        if let Some(surface) = s.borrow().as_ref() {
            f(surface);
        }
    });
}

/// Zeichnen ein/aus und den Knopf in der Leiste nachführen (Haupt-Thread).
fn set_drawing(surface: &DrawSurface, on: bool) {
    if surface.is_active() != on {
        log::info!("Video: Zeichnen {}", if on { "an" } else { "aus" });
    }
    surface.set_active(on);
    let label = DRAW_SHORTCUT.with(|s| draw_label(on, &s.borrow()));
    UI.with(|u| {
        if let Some(ui) = u.borrow().as_ref() {
            ui.set_label(RecordAction::Draw, &label);
        }
    });
}

/// Abstand der Proben der Mausspur (~60 je Sekunde).
const MOUSE_INTERVAL: Duration = Duration::from_millis(16);

/// Mausspur während der Aufnahme (Etappe 5): Lage relativ zum
/// aufgenommenen Bereich und ob die Taste gedrückt ist. Klicks in Nabshot
/// selbst (Leisten, Zeichnen) fallen beim Abschluss heraus.
struct MouseLog {
    app: AppHandle,
    running: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<Vec<InputSample>>>,
    /// Zeitpunkte der Klicks in Nabshot selbst (Zeit im Video).
    own_clicks: Arc<Mutex<Vec<u32>>>,
}

impl MouseLog {
    /// `log` = Datei, an die jede Sekunde die neuen Proben angehängt werden
    /// (JSON-Zeilen) – nach einem Absturz bleibt die Spur bis dahin erhalten.
    fn start(
        app: &AppHandle,
        recording: &Arc<dyn Recording>,
        choice: &Choice,
        log: std::path::PathBuf,
    ) -> Self {
        let own_clicks = Arc::new(Mutex::new(Vec::new()));
        let (rec, own) = (recording.clone(), own_clicks.clone());
        main_thread(app, "Klick-Wächter", move || {
            let watch = nabshot_overlay::watch_own_clicks(Box::new(move || {
                let at = rec.elapsed().as_millis().min(u32::MAX as u128) as u32;
                own.lock().unwrap_or_else(|e| e.into_inner()).push(at);
            }));
            OWN_CLICKS.with(|w| *w.borrow_mut() = Some(watch));
        });
        let running = Arc::new(AtomicBool::new(true));
        let handle = app.clone();
        let area = area_points(app, choice);
        let (app, recording, flag) = (app.clone(), recording.clone(), running.clone());
        let thread = std::thread::spawn(move || {
            let mut samples = Vec::new();
            let Some((ax, ay, aw, ah)) = area else {
                log::warn!("Video: Fläche unbekannt – keine Mausspur");
                return samples;
            };
            let capturer = &app.state::<CaptureState>().capturer;
            let mut last: Option<(f32, f32, bool)> = None;
            let mut writer = InputLog::open(log);
            let mut flushed = Instant::now();
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
                if flushed.elapsed() >= Duration::from_secs(1) {
                    flushed = Instant::now();
                    writer.append(&samples);
                }
                std::thread::sleep(MOUSE_INTERVAL);
            }
            samples
        });
        Self {
            app: handle,
            running,
            thread: Some(thread),
            own_clicks,
        }
    }

    fn finish(mut self) -> Vec<InputSample> {
        self.stop();
        let mut samples = match self.thread.take().map(|t| t.join()) {
            Some(Ok(samples)) => samples,
            Some(Err(_)) => {
                log::error!("Video: Mausspur abgestürzt");
                Vec::new()
            }
            None => Vec::new(),
        };
        let own = self.own_clicks.lock().unwrap_or_else(|e| e.into_inner());
        let before = samples.len();
        nabshot_video::cursor::remove_own_clicks(&mut samples, &own);
        log::debug!(
            "Video: {} Klicks in Nabshot, {} Proben entfernt",
            own.len(),
            before - samples.len()
        );
        samples
    }

    fn stop(&self) {
        self.running.store(false, Ordering::Release);
        main_thread(&self.app, "Klick-Wächter", || {
            OWN_CLICKS.with(|w| w.borrow_mut().take());
        });
    }
}

/// Mausspur zeilenweise auf der Platte (`<id>_input.jsonl`).
struct InputLog {
    file: Option<std::io::BufWriter<std::fs::File>>,
    /// So viele Proben stehen schon in der Datei.
    written: usize,
    /// Schreibfehler nur einmal festhalten.
    failed: bool,
}

impl InputLog {
    fn open(path: std::path::PathBuf) -> Self {
        let file = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&path)
            .map_err(|e| log::warn!("{}: {e}", path.display()))
            .ok()
            .map(std::io::BufWriter::new);
        Self {
            file,
            written: 0,
            failed: false,
        }
    }

    fn append(&mut self, samples: &[InputSample]) {
        use std::io::Write;
        let Some(file) = self.file.as_mut() else {
            return;
        };
        let mut result = Ok(());
        for sample in &samples[self.written.min(samples.len())..] {
            if let Ok(line) = serde_json::to_string(sample) {
                result = result.and(writeln!(file, "{line}"));
            }
        }
        self.written = samples.len();
        if let Err(e) = result.and(file.flush())
            && !self.failed
        {
            self.failed = true;
            log::warn!("Video: Mausspur sichern: {e}");
        }
    }
}

/// Mausspur aus `<id>_input.jsonl` (nach einem Absturz). Kaputte letzte
/// Zeile wird übersprungen.
fn read_input_log(path: &Path) -> Vec<InputSample> {
    match std::fs::read_to_string(path) {
        Ok(text) => {
            let lines = text.lines().count();
            let samples: Vec<InputSample> = text
                .lines()
                .filter_map(|l| serde_json::from_str(l).ok())
                .collect();
            log::debug!(
                "Video: Mausspur {} Proben, {} Zeilen unlesbar",
                samples.len(),
                lines - samples.len()
            );
            samples
        }
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Vec::new(),
        Err(e) => {
            log::warn!("{}: {e}", path.display());
            Vec::new()
        }
    }
}

/// Zwischenstand der Aufnahme (`<id>_recording.json`), nur bei Änderung
/// neu geschrieben.
struct Checkpoint {
    path: std::path::PathBuf,
    state: RecordingCheckpoint,
    written: Option<String>,
}

impl Checkpoint {
    fn new(path: std::path::PathBuf, state: RecordingCheckpoint) -> Self {
        let mut c = Self {
            path,
            state,
            written: None,
        };
        c.update(|_| {});
        c
    }

    fn update(&mut self, change: impl FnOnce(&mut RecordingCheckpoint)) {
        change(&mut self.state);
        let json = match serde_json::to_string(&self.state) {
            Ok(json) => json,
            Err(e) => {
                log::warn!("Video: Zwischenstand: {e}");
                return;
            }
        };
        if self.written.as_deref() == Some(json.as_str()) {
            return;
        }
        // Erst daneben schreiben, dann umbenennen: nie eine halbe Datei.
        let temp = self.path.with_extension("json.tmp");
        match std::fs::write(&temp, &json).and_then(|()| std::fs::rename(&temp, &self.path)) {
            Ok(()) => {
                log::debug!(
                    "Video: Zwischenstand gesichert ({} Webcam-Abschnitte, {} Zeichnungen)",
                    self.state.webcam_sections.len(),
                    self.state.drawings.len()
                );
                self.written = Some(json);
            }
            Err(e) => log::warn!("{}: {e}", self.path.display()),
        }
    }
}

/// Freier Speicher während der Aufnahme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum DiskState {
    Ok,
    /// Warnung in der Leiste.
    Low,
    /// Aufnahme beenden und sichern.
    Critical,
}

fn disk_state(free: u64) -> DiskState {
    if free < DISK_CRITICAL {
        DiskState::Critical
    } else if free < DISK_LOW {
        DiskState::Low
    } else {
        DiskState::Ok
    }
}

/// Wird gerade aufgenommen oder ein Video gesichert?
pub fn is_active(app: &AppHandle) -> bool {
    let state = app.state::<VideoState>();
    state.active.load(Ordering::Acquire) || state.starting.load(Ordering::Acquire)
}

/// Läuft eine Aufnahme (bis das Video gesichert ist)? Setzt beim Ende zurück.
struct Active(AppHandle);

impl Active {
    fn start(app: &AppHandle) -> Self {
        app.state::<VideoState>()
            .active
            .store(true, Ordering::Release);
        Self(app.clone())
    }
}

impl Drop for Active {
    fn drop(&mut self) {
        self.0
            .state::<VideoState>()
            .active
            .store(false, Ordering::Release);
    }
}

/// Beenden (Menü, Tray): Eine laufende Aufnahme erst regulär stoppen und
/// sichern, dann beenden.
pub fn quit(app: &AppHandle) {
    let state = app.state::<VideoState>();
    if !state.active.load(Ordering::Acquire) {
        app.exit(0);
        return;
    }
    log::info!("Beenden: wartet, bis das Video gesichert ist");
    stop_running(app);
    let app = app.clone();
    std::thread::spawn(move || {
        let started = Instant::now();
        let deadline = started + Duration::from_secs(600);
        while app.state::<VideoState>().active.load(Ordering::Acquire) && Instant::now() < deadline
        {
            std::thread::sleep(Duration::from_millis(100));
        }
        if app.state::<VideoState>().active.load(Ordering::Acquire) {
            log::warn!("Beenden: Video nach 10 min nicht gesichert – beendet trotzdem");
        } else {
            log::info!(
                "Beenden: Video gesichert nach {} ms",
                started.elapsed().as_millis()
            );
        }
        app.exit(0);
    });
}

/// Auch bei Abbruch: Abtastung und Wächter beenden.
impl Drop for MouseLog {
    fn drop(&mut self) {
        self.stop();
    }
}

/// Aufgenommene Fläche in globalen Punkten (x, y, Breite, Höhe).
fn area_points(app: &AppHandle, choice: &Choice) -> Option<(f32, f32, f32, f32)> {
    let displays = app
        .state::<CaptureState>()
        .capturer
        .displays()
        .map_err(|e| log::warn!("Video: Monitore: {e}"))
        .ok()?;
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
    starting: Option<std::thread::JoinHandle<nabshot_media::Result<()>>>,
    /// Seit wann die Kamera anläuft.
    since: Instant,
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
        log::debug!("Webcam: startet");
        self.since = Instant::now();
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
                    log::info!("Webcam: an ({} ms)", self.since.elapsed().as_millis());
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
                log::info!("Webcam: aus");
                if let Err(e) = recording.set_webcam(false) {
                    log::warn!("Webcam: {e}");
                }
                self.on = false;
                hide_bubble(app);
                set_label(app, RecordAction::Webcam, t("video.webcam_on", &[]));
            } else if camera_allowed(app) {
                log::info!("Webcam: einschalten");
                set_label(app, RecordAction::Webcam, t("video.webcam_off", &[]));
                self.start(recording);
            } else {
                log::info!("Webcam: keine Freigabe");
            }
        }
        if flags.toggle_large.swap(false, Ordering::AcqRel) && self.on {
            self.large = !self.large;
            log::info!("Webcam: {}", if self.large { "gross" } else { "klein" });
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
    match nabshot_media::camera_permission() {
        Permission::Granted => true,
        Permission::NotDetermined => asked("Kamera", nabshot_media::request_camera_permission()),
        Permission::Denied => {
            log::warn!("Webcam: Kamera-Freigabe fehlt – Systemeinstellungen");
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
    // Nur Webcam: Die Blase ist die ganze Fläche.
    if choice.target.is_none() {
        return (
            area.x as f64,
            area.y as f64,
            area.width as f64,
            area.height as f64,
        );
    }
    let frame = Size::new(area.width, area.height);
    let camera = Size::new(16.0, 9.0);
    let p = nabshot_video::layout::webcam_overlay(layout, frame, camera);
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
        let (position, size) = nabshot_video::layout_from_bubble(area_rect(choice), rect);
        layout.position = position;
        layout.size = size;
    }
    layout
}

fn bubble_style(layout: &WebcamLayout, camera_only: bool) -> BubbleStyle {
    // Wie im Export: Ohne Hintergrund schwebt nur die Person.
    let removed = layout.background == WebcamBackground::Remove;
    if camera_only {
        return BubbleStyle {
            corner_radius: 0.04,
            border: None,
            shadow: !removed,
            transparent: removed,
        };
    }
    BubbleStyle {
        corner_radius: match layout.shape {
            WebcamShape::Circle => 0.5,
            _ => layout.corner_radius.clamp(0.0, 0.5) as f64,
        },
        border: layout
            .border
            .filter(|b| b.width > 0.0 && !removed)
            .map(|b| {
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
        shadow: layout.shadow && !removed,
        transparent: removed,
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
    let style = bubble_style(&cam.layout, choice.target.is_none());
    let layout = cam.layout;
    main_thread(app, "Webcam-Blase", move || {
        BUBBLE.with(|b| {
            let mut b = b.borrow_mut();
            if let Some(bubble) = b.as_ref() {
                bubble.set_visible(true);
                return;
            }
            let Some(preview) = recording.webcam_preview(&layout) else {
                log::warn!("Webcam-Blase: keine Vorschau");
                return;
            };
            // SAFETY: `preview` ist eine gültige Ebene von nabshot-media; die
            // Blase hält sie selbst fest.
            match unsafe {
                nabshot_overlay::open_webcam_bubble(display_id, rect, style, preview.as_ptr())
            } {
                Ok(bubble) => {
                    log::debug!("Webcam-Blase geöffnet ({:.0}×{:.0})", rect.2, rect.3);
                    *b = Some(bubble);
                }
                Err(e) => log::error!("Webcam-Blase: {e}"),
            }
        })
    });
}

fn hide_bubble(app: &AppHandle) {
    main_thread(app, "Webcam-Blase", || {
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
    main_thread(app, "Webcam-Blase", move || {
        let frame = BUBBLE.with(|b| {
            b.borrow_mut().take().map(|bubble| {
                let frame = bubble.frame();
                bubble.close();
                log::debug!("Webcam-Blase geschlossen");
                frame
            })
        });
        // Wartet keiner mehr (Zeitüberschreitung), ist das egal.
        let _ = tx.send(frame);
    });
    rx.recv_timeout(Duration::from_secs(2))
        .map_err(|e| log::warn!("Webcam-Blase schliessen: {e}"))
        .ok()
        .flatten()
}

fn close_ui(app: &AppHandle) {
    main_thread(app, "Video-Leiste", || {
        UI.with(|u| {
            if let Some(ui) = u.borrow_mut().take() {
                ui.close();
                log::debug!("Video-Leiste geschlossen");
            }
        })
    });
}

/// Beschriftung des Zeichnen-Knopfs, mit Kürzel (z. B. „Zeichnen  ⇧⌘8“).
fn draw_label(on: bool, shortcut: &str) -> String {
    let label = t(if on { "video.draw_done" } else { "video.draw" }, &[]);
    if shortcut.is_empty() {
        label
    } else {
        format!("{label}  {shortcut}")
    }
}

/// Aktuelles Kürzel für Zeichnen ein/aus (leer, wenn keins belegt ist).
fn draw_shortcut(app: &AppHandle) -> String {
    let platform = Platform::current();
    crate::settings::get(app)
        .effective_hotkeys(platform)
        .into_iter()
        .find(|(a, _)| *a == Action::RecordDraw)
        .map(|(_, s)| crate::i18n::shortcut_label(&s))
        .unwrap_or_default()
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
    // Zeichnen nur mit Bildschirm.
    let shortcut = draw_shortcut(app);
    let draw = choice.target.map(|_| draw_label(false, &shortcut));
    let (display_id, rect, flags) = (choice.display_id, choice.rect, flags.clone());
    main_thread(app, "Video-Leiste", move || {
        DRAW_SHORTCUT.with(|s| *s.borrow_mut() = shortcut);
        let ui = nabshot_overlay::open_record_ui(
            display_id,
            rect,
            &RecordLabels {
                status: &status,
                pause: &pause,
                draw: draw.as_deref(),
                webcam: webcam_label.as_deref(),
                webcam_large: large.as_deref(),
                stop: &stop,
                cancel: &cancel,
            },
            Box::new(move |action| {
                log::debug!("Video-Leiste: {action:?}");
                let flag = match action {
                    RecordAction::Pause => &flags.toggle_pause,
                    RecordAction::Draw => &flags.toggle_draw,
                    RecordAction::Webcam => &flags.toggle_webcam,
                    RecordAction::WebcamLarge => &flags.toggle_large,
                    RecordAction::Stop => &flags.stop,
                    RecordAction::Cancel => &flags.cancel,
                };
                flag.store(true, Ordering::Release);
            }),
        );
        match ui {
            Ok(ui) => {
                log::debug!("Video-Leiste geöffnet");
                UI.with(|u| *u.borrow_mut() = Some(ui));
            }
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

fn set_time(app: &AppHandle, time: String, detail: String, live: bool) {
    let _ = app.run_on_main_thread(move || {
        UI.with(|u| {
            if let Some(ui) = u.borrow().as_ref() {
                ui.set_time(&time, &detail, live);
            }
        })
    });
}

fn set_failed(app: &AppHandle, text: String) {
    let _ = app.run_on_main_thread(move || {
        UI.with(|u| {
            if let Some(ui) = u.borrow().as_ref() {
                ui.set_failed(&text);
            }
        })
    });
}

fn set_rect(app: &AppHandle, rect: (u32, u32, u32, u32)) {
    let _ = app.run_on_main_thread(move || {
        UI.with(|u| {
            if let Some(ui) = u.borrow().as_ref() {
                ui.set_rect(rect);
            }
        })
    });
}

/// Zeit „0:12“ und daneben „Mikro ▮▮▮▯▯“ bzw. „Pause“.
fn status_text(recording: &dyn Recording, microphone: bool, system: bool) -> (String, String) {
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
    let detail = if recording.is_paused() {
        t("video.pause", &[])
    } else {
        meters.join("  ")
    };
    (time, detail)
}

fn meter(peak: f32) -> String {
    let on = nabshot_media::level_steps(peak, METER_STEPS);
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

/// Woher ein Video kommt, das gesichert wird.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Origin {
    /// Gerade aufgenommen: wie nach einer Bildaufnahme weitermachen.
    Live,
    /// Nach einem Absturz wiederhergestellt: nur in die Galerie.
    Recovered,
}

/// Spuren zusammensetzen, Standbild holen, in die Bibliothek legen und wie
/// nach einer Bildaufnahme weitermachen (Vorschau, Editor …). `false`, wenn
/// es nicht geklappt hat – dann bleiben die Spuren für einen neuen Versuch
/// beim nächsten Start liegen.
#[allow(clippy::too_many_arguments)]
fn finish(
    app: &AppHandle,
    slot: &VideoSlot,
    files: &RecordFiles,
    summary: RecordSummary,
    scale: f32,
    video_doc: VideoDoc,
    drawings: Vec<SketchItem>,
    origin: Origin,
) -> bool {
    // Das Dokument bezieht sich auf die volle Grösse, auch wenn das fertige
    // Video kleiner ist (Einstellung *Auflösung*). Live Gezeichnetes wird zu
    // Ebenen mit Zeitbereich.
    let started = Instant::now();
    let frame = crate::video_edit::frame_size(files, &video_doc).unwrap_or_else(|e| {
        log::warn!("Video: Grösse aus den Spuren: {e} – nehme Aufnahmegrösse");
        (summary.width.max(2), summary.height.max(2))
    });
    let drawing_count = drawings.len();
    let mut doc = Document::from_capture_scaled(frame.0, frame.1, scale);
    let mut video_doc = video_doc;
    for item in drawings {
        let id = doc.push_layer(item.kind);
        if let Some(layer) = doc.layers.iter_mut().find(|l| l.id == id) {
            layer.opacity = item.opacity;
        }
        video_doc.layer_times.push(LayerTime {
            layer: id.0,
            start_ms: item.start_ms,
            end_ms: item.end_ms,
        });
    }
    doc.video = Some(video_doc);
    // Das fertige Video entsteht erst auf Wunsch (Kopieren, Sichern …) – der
    // Editor spielt die Spuren direkt ab. Standbild aus der Hauptspur.
    let Some(main) = files.screen.as_ref().or(files.webcam.as_ref()) else {
        log::error!("Video {}: keine Spur zum Sichern", slot.id);
        return false;
    };
    let poster = match nabshot_media::frame_at(main, Duration::ZERO) {
        Ok(p) => p,
        Err(e) => {
            log::error!("Video {}: Standbild: {e}", slot.id);
            capture::show_error(app, &e.to_string());
            return false;
        }
    };
    let poster_ms = started.elapsed().as_millis();
    let library = app.state::<CaptureState>().library.clone();
    let rec = match library.save_video(slot, &poster, scale, doc) {
        Ok(r) => r,
        Err(e) => {
            log::error!("Video {}: sichern: {e}", slot.id);
            capture::show_error(app, &e.to_string());
            return false;
        }
    };
    // Gesichert: Zwischenstände werden nicht mehr gebraucht.
    for path in [slot.checkpoint_path(), slot.input_log_path()] {
        if let Err(e) = std::fs::remove_file(&path)
            && e.kind() != std::io::ErrorKind::NotFound
        {
            log::warn!("{}: {e}", path.display());
        }
    }
    log::info!(
        "Video: gesichert als {} ({}×{}, {} Zeichnungen, {origin:?}) in {} ms (Standbild {poster_ms} ms)",
        rec.id,
        frame.0,
        frame.1,
        drawing_count,
        started.elapsed().as_millis()
    );
    crate::services::index_capture(app, &rec);
    if origin == Origin::Live {
        open_in_editor(app, &rec.id, crate::settings::get(app).copy_to_clipboard);
    }
    true
}

/// Gerade aufgenommenes Video im Editor öffnen. `copy` = danach das fertige
/// Video in die Zwischenablage (der Editor zeigt dabei den Fortschritt).
fn open_in_editor(app: &AppHandle, id: &str, copy: bool) {
    #[derive(Clone, serde::Serialize)]
    struct OpenVideo<'a> {
        id: &'a str,
        copy: bool,
    }
    log::debug!("Video: {id} im Editor öffnen (kopieren: {copy})");
    if let Err(e) = app.emit("open-video", OpenVideo { id, copy }) {
        log::warn!("open-video: {e}");
    }
    let a = app.clone();
    main_thread(app, "Hauptfenster", move || {
        crate::tray::show_main_window(&a)
    });
}

/// Nach dem Start: Aufnahmen, die nie fertig wurden (Absturz), anbieten
/// (ADR 0012). Läuft im Hintergrund.
pub fn offer_recovery(app: &AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        let library = app.state::<CaptureState>().library.clone();
        let slots: Vec<(VideoSlot, Duration)> = library
            .unfinished_videos()
            .into_iter()
            .filter_map(|slot| {
                let Some(main) = [slot.screen_path(), slot.webcam_path()]
                    .into_iter()
                    .find(|p| p.exists())
                else {
                    log::debug!(
                        "Abgebrochene Aufnahme {}: ohne Spur – übersprungen",
                        slot.id
                    );
                    return None;
                };
                match nabshot_media::duration(&main) {
                    Ok(d) => Some((slot, d)),
                    Err(e) => {
                        // Nicht einmal ein Fragment lesbar: nichts zu retten.
                        log::warn!("Abgebrochene Aufnahme {}: {e}", slot.id);
                        library.discard_video(&slot);
                        None
                    }
                }
            })
            .collect();
        if slots.is_empty() {
            return;
        }
        log::info!(
            "Wiederherstellung: {} abgebrochene Aufnahme(n) angeboten",
            slots.len()
        );
        let durations: Vec<String> = slots.iter().map(|(_, d)| format_duration(*d)).collect();
        let message = if slots.len() == 1 {
            t("video.recover_one", &[("duration", &durations[0])])
        } else {
            t(
                "video.recover_many",
                &[
                    ("count", &slots.len().to_string()),
                    ("durations", &durations.join(", ")),
                ],
            )
        };
        let (recover, discard) = (t("video.recover", &[]), t("video.discard", &[]));
        let answer = app
            .dialog()
            .message(message)
            .title(t("video.recover_title", &[]))
            .kind(MessageDialogKind::Warning)
            .buttons(MessageDialogButtons::YesNoCancelCustom(
                recover.clone(),
                discard.clone(),
                t("video.later", &[]),
            ))
            .blocking_show_with_result();
        let choice = match answer {
            MessageDialogResult::Yes => Some(true),
            MessageDialogResult::No => Some(false),
            MessageDialogResult::Custom(label) if label == recover => Some(true),
            MessageDialogResult::Custom(label) if label == discard => Some(false),
            // Später (oder Fenster geschlossen): beim nächsten Start wieder fragen.
            _ => None,
        };
        log::info!(
            "Wiederherstellung: {}",
            match choice {
                Some(true) => "angenommen",
                Some(false) => "verworfen",
                None => "später",
            }
        );
        for (slot, duration) in slots {
            match choice {
                Some(true) => {
                    let started = Instant::now();
                    if recover_slot(&app, &slot, duration) {
                        log::info!(
                            "Wiederherstellung: {} ({}) gesichert in {} ms",
                            slot.id,
                            format_duration(duration),
                            started.elapsed().as_millis()
                        );
                        let msg = t(
                            "video.recovered",
                            &[("duration", &format_duration(duration))],
                        );
                        main_thread(&app, "Hinweis", move || nabshot_overlay::show_toast(&msg));
                    } else {
                        log::error!("Wiederherstellung: {} fehlgeschlagen", slot.id);
                    }
                }
                Some(false) => {
                    log::info!("Wiederherstellung: {} verworfen", slot.id);
                    library.discard_video(&slot);
                }
                None => {}
            }
        }
    });
}

/// Eine abgebrochene Aufnahme aus Spuren und Zwischenstand sichern.
fn recover_slot(app: &AppHandle, slot: &VideoSlot, duration: Duration) -> bool {
    let existing = |p: std::path::PathBuf| p.exists().then_some(p);
    let files = RecordFiles {
        screen: existing(slot.screen_path()),
        system_audio: existing(slot.system_audio_path()),
        microphone: existing(slot.microphone_path()),
        webcam: existing(slot.webcam_path()),
        faces: existing(slot.faces_path()),
    };
    // Mausspur: Stand bis zum Absturz.
    if !slot.input_path().exists() {
        let samples = read_input_log(&slot.input_log_path());
        if !samples.is_empty() {
            write_json(&slot.input_path(), &InputTrack { samples });
        }
    }
    let checkpoint = match std::fs::read(slot.checkpoint_path()) {
        Ok(b) => serde_json::from_slice::<RecordingCheckpoint>(&b)
            .map_err(|e| log::warn!("Wiederherstellung {}: Zwischenstand: {e}", slot.id))
            .ok(),
        Err(e) => {
            log::warn!("Wiederherstellung {}: ohne Zwischenstand ({e})", slot.id);
            None
        }
    };
    log::debug!(
        "Wiederherstellung {}: Bildschirm {}, Webcam {}, Zwischenstand {}",
        slot.id,
        files.screen.is_some(),
        files.webcam.is_some(),
        checkpoint.is_some()
    );
    let settings = crate::settings::get(app).video;
    let camera_only = files.screen.is_none();
    let scale = app
        .state::<CaptureState>()
        .capturer
        .displays()
        .map_err(|e| log::warn!("Wiederherstellung: Monitore: {e}"))
        .ok()
        .and_then(|ds| ds.into_iter().find(|d| d.is_primary))
        .map_or(2.0, |d| d.scale_factor * nabshot_media::record_ratio(d.id));
    let fallback = RecordingCheckpoint {
        fps: settings.fps,
        scale,
        camera_only: camera_only.then_some(settings.camera.format),
        webcam_layout: if camera_only {
            settings.camera.layout()
        } else {
            settings.webcam_layout
        },
        ..RecordingCheckpoint::default()
    };
    let r = nabshot_video::recovery::recovered(
        checkpoint,
        fallback,
        duration.as_millis().min(u32::MAX as u128) as u32,
        !camera_only,
        files.webcam.is_some(),
    );
    let drawings = r
        .drawings
        .into_iter()
        .map(|d| SketchItem {
            kind: d.kind,
            opacity: d.opacity,
            start_ms: d.start_ms,
            end_ms: d.end_ms,
        })
        .collect();
    let main = files.screen.as_ref().or(files.webcam.as_ref());
    let (width, height) = main
        .and_then(|p| {
            nabshot_media::open_video(p)
                .map_err(|e| log::warn!("Wiederherstellung {}: {e}", slot.id))
                .ok()
        })
        .map_or((0, 0), |source| source.size());
    let summary = RecordSummary {
        duration,
        width,
        height,
    };
    finish(
        app,
        slot,
        &files,
        summary,
        r.scale,
        r.video,
        drawings,
        Origin::Recovered,
    )
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
    fn speicher_warnt_und_stoppt_rechtzeitig() {
        assert_eq!(disk_state(50_000_000_000), DiskState::Ok);
        assert_eq!(disk_state(1_500_000_000), DiskState::Low);
        assert_eq!(disk_state(400_000_000), DiskState::Critical);
    }

    #[test]
    fn mausspur_zeilen_ueberstehen_einen_abbruch() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("x_input.jsonl");
        let sample = |t_ms| InputSample {
            t_ms,
            x: 0.5,
            y: 0.25,
            down: false,
        };
        let mut log = InputLog::open(path.clone());
        log.append(&[sample(0), sample(16)]);
        log.append(&[sample(0), sample(16), sample(32)]);
        // Absturz mitten in einer Zeile.
        std::fs::OpenOptions::new()
            .append(true)
            .open(&path)
            .and_then(|mut f| std::io::Write::write_all(&mut f, b"{\"t_ms\":4"))
            .unwrap();
        let back = read_input_log(&path);
        assert_eq!(back.iter().map(|s| s.t_ms).collect::<Vec<_>>(), [0, 16, 32]);
    }

    #[test]
    fn pegel_als_balken() {
        assert_eq!(meter(0.0), "▯▯▯▯▯");
        assert_eq!(meter(1.0), "▮▮▮▮▮");
    }
}
