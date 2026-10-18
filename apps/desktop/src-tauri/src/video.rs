//! Video-Aufnahme (ADR 0011): Ziel wählen (Bereich, Fenster, Bildschirm),
//! Countdown, Aufnahmeleiste mit Pause/Stopp, danach Spuren zusammenführen
//! und in die Bibliothek legen. Die Aufnahme selbst macht `fmshot-media`.

use std::cell::RefCell;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use fmshot_capture::{PrivacyPane, display_at};
use fmshot_library::VideoSlot;
use fmshot_media::{MediaError, Permission, RecordFiles, RecordOptions, RecordTarget, Recording};
use fmshot_overlay::{
    OverlayHandle, OverlayMode, OverlayResult, RecordAction, RecordLabels, RecordUi, WindowTarget,
};
use fmshot_settings::{VideoSettings, VideoTarget};
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
}

thread_local! {
    static UI: RefCell<Option<RecordUi>> = const { RefCell::new(None) };
    static OVERLAY: RefCell<Option<OverlayHandle>> = const { RefCell::new(None) };
}

/// Was nach der Auswahl aufgenommen wird.
#[derive(Debug, Clone, Copy)]
struct Choice {
    target: RecordTarget,
    display_id: u32,
    /// Rahmen um den Bereich (physische Pixel des Monitors).
    rect: Option<(u32, u32, u32, u32)>,
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
        select(&app, target, move |app, choice| {
            let app = app.clone();
            std::thread::spawn(move || run(&app, choice, microphone, settings));
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
            let choice = Choice {
                target: RecordTarget::Display {
                    display_id: display.id,
                },
                display_id: display.id,
                rect: None,
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

fn run(app: &AppHandle, choice: Choice, microphone: bool, settings: VideoSettings) {
    let library = app.state::<CaptureState>().library.clone();
    let slot = match library.reserve_video(now_ms()) {
        Ok(s) => s,
        Err(e) => {
            done_starting(app);
            capture::show_error(app, &e.to_string());
            return;
        }
    };
    let files = RecordFiles {
        screen: slot.screen_path(),
        system_audio: settings.system_audio.then(|| slot.system_audio_path()),
        microphone: microphone.then(|| slot.microphone_path()),
    };
    // Kurz warten, bis das Auswahl-Overlay verschwunden ist.
    std::thread::sleep(Duration::from_millis(80));
    let recording = fmshot_media::start_recording(RecordOptions {
        target: choice.target,
        files: files.clone(),
        fps: settings.fps,
        microphone_id: settings.microphone_id.clone(),
        // Bis FMShot den Zeiger selbst zeichnet (Etappe 5).
        show_cursor: true,
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
    open_ui(app, &choice, &flags, settings.countdown);

    if settings.countdown {
        for n in (1..=3).rev() {
            set_status(app, t("video.countdown", &[("n", &n.to_string())]));
            for _ in 0..5 {
                std::thread::sleep(TICK);
                if flags.cancel.load(Ordering::Acquire) || flags.stop.load(Ordering::Acquire) {
                    break;
                }
            }
        }
    }
    if !flags.cancel.load(Ordering::Acquire) {
        recording.begin();
    }

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
            let _ = app.run_on_main_thread(move || {
                UI.with(|u| {
                    if let Some(ui) = u.borrow().as_ref() {
                        ui.set_pause_label(&label);
                    }
                })
            });
        }
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
    let _ = app.run_on_main_thread(|| {
        UI.with(|u| {
            if let Some(ui) = u.borrow_mut().take() {
                ui.close();
            }
        })
    });

    if flags.cancel.load(Ordering::Acquire) {
        recording.cancel();
        library.discard_video(&slot);
        return;
    }
    match recording.stop() {
        Ok(summary) => {
            if let Some(e) = failure {
                log::warn!("Video endete vorzeitig: {e}");
            }
            finish(app, &slot, &files, summary, choice.scale);
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
}

fn open_ui(app: &AppHandle, choice: &Choice, flags: &Flags, countdown: bool) {
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
    let (display_id, rect, flags) = (choice.display_id, choice.rect, flags.clone());
    let _ = app.run_on_main_thread(move || {
        let ui = fmshot_overlay::open_record_ui(
            display_id,
            rect,
            &RecordLabels {
                status: &status,
                pause: &pause,
                stop: &stop,
                cancel: &cancel,
            },
            Box::new(move |action| {
                let flag = match action {
                    RecordAction::Pause => &flags.toggle_pause,
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

/// Spuren zusammenführen, Standbild holen, in die Bibliothek legen und wie
/// nach einer Bildaufnahme weitermachen (Vorschau, Editor …).
fn finish(
    app: &AppHandle,
    slot: &VideoSlot,
    files: &RecordFiles,
    summary: fmshot_media::RecordSummary,
    scale: f32,
) {
    let video = slot.video_path();
    let started = std::time::Instant::now();
    if let Err(e) = fmshot_media::export_mp4(files, &video) {
        // Ohne Ton ist besser als gar nichts.
        log::error!("Video zusammenführen: {e}");
        if let Err(e) = std::fs::copy(&files.screen, &video) {
            log::error!("Video kopieren: {e}");
        }
    }
    crate::services::timing(
        app,
        format!(
            "Video {}×{}, {:.1} s: zusammengeführt in {:.0} ms",
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
    let rec = match library.save_video(slot, &poster, scale, summary.duration.as_millis() as i64) {
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
