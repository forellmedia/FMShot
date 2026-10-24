//! macOS: ScreenCaptureKit (`SCScreenshotManager`, ab macOS 14).
//!
//! `SCShareableContent` abzufragen kostet spürbar Zeit. Wir halten deshalb
//! einen Schnappschuss im Cache und erneuern ihn nach jeder Aufnahme im
//! Hintergrund. Für Monitore und die eigene App ist ein leicht veralteter
//! Schnappschuss unkritisch; haben sich die Monitore geändert oder schlägt
//! eine Aufnahme damit fehl, wird frisch abgefragt.

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::{Arc, Mutex, mpsc};
use std::time::{Duration, Instant};

use block2::RcBlock;
use nabshot_core::{Rect, RgbaImage};
use objc2::AllocAnyThread;
use objc2::rc::Retained;
use objc2_core_foundation::{
    CFArray, CFDictionary, CFNumber, CFRetained, CFString, CGPoint, CGRect, CGSize,
};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGDisplayBounds, CGDisplayCopyDisplayMode,
    CGDisplayMirrorsDisplay, CGDisplayMode, CGError, CGEvent, CGEventSource, CGEventSourceStateID,
    CGEventTapLocation, CGGetActiveDisplayList, CGImage, CGImageAlphaInfo, CGImageByteOrderInfo,
    CGMainDisplayID, CGMouseButton, CGPreflightPostEventAccess, CGPreflightScreenCaptureAccess,
    CGRectMakeWithDictionaryRepresentation, CGRequestPostEventAccess, CGRequestScreenCaptureAccess,
    CGScrollEventUnit, CGWarpMouseCursorPosition, CGWindowListCopyWindowInfo, CGWindowListCreate,
    CGWindowListOption, kCGColorSpaceSRGB, kCGNullDirectDisplay, kCGNullWindowID, kCGWindowBounds,
    kCGWindowLayer, kCGWindowNumber, kCGWindowOwnerPID,
};
use objc2_foundation::{NSArray, NSError};
use objc2_screen_capture_kit::{
    SCContentFilter, SCDisplay, SCRunningApplication, SCScreenshotManager, SCShareableContent,
    SCStreamConfiguration, SCWindow,
};

use crate::{
    CaptureError, Display, Frame, OwnWindows, PixelRect, Result, ScreenCapturer, WindowInfo,
};

/// Wie lange auf ScreenCaptureKit gewartet wird, bevor wir aufgeben.
const TIMEOUT: Duration = Duration::from_secs(5);
/// Kleinere Fenster (Symbole, Ränder) sind keine sinnvollen Ziele.
const MIN_WINDOW_SIZE: f64 = 40.0;

/// ScreenCaptureKit-Objekte sind unveränderlich und dürfen zwischen Threads
/// wandern; objc2 markiert sie vorsichtshalber nicht als `Send`.
struct SendRetained<T>(Retained<T>);
// SAFETY: siehe oben – SCShareableContent ist ein unveränderlicher Schnappschuss.
unsafe impl<T> Send for SendRetained<T> {}
// SAFETY: wie oben, nur lesender Zugriff.
unsafe impl<T> Sync for SendRetained<T> {}

type Content = Arc<SendRetained<SCShareableContent>>;

pub struct MacCapturer {
    cache: Arc<Mutex<Option<Content>>>,
}

impl MacCapturer {
    pub fn new() -> Self {
        Self {
            cache: Arc::new(Mutex::new(None)),
        }
    }

    /// Schnappschuss aus dem Cache oder frisch.
    ///
    /// Haben sich die Monitore seither geändert (angesteckt, abgesteckt,
    /// umgestellt), ist der Cache unbrauchbar: Ein neuer Monitor fehlte ohne
    /// Fehler, ein abgesteckter stünde noch als Ziel da. Der Vergleich mit
    /// Core Graphics ist synchron und kostet nichts.
    fn content(&self) -> Result<Content> {
        if let Some(c) = self.cache.lock().unwrap_or_else(|e| e.into_inner()).clone() {
            let known = display_layout(&c.0);
            let live = active_display_layout();
            if live.is_empty() || known == live {
                return Ok(c);
            }
            log::info!(
                "Monitore geändert (Schnappschuss {known:?}, jetzt {live:?}) – Bildschirminhalt frisch abfragen"
            );
        }
        self.fresh_content()
    }

    fn fresh_content(&self) -> Result<Content> {
        let content = Arc::new(SendRetained(fetch_shareable_content().inspect_err(
            |e| log::warn!("Bildschirminhalt abfragen fehlgeschlagen: {e}"),
        )?));
        *self.cache.lock().unwrap_or_else(|e| e.into_inner()) = Some(content.clone());
        Ok(content)
    }

    /// Erneuert den Cache im Hintergrund.
    fn refresh_later(&self) {
        let cache = self.cache.clone();
        std::thread::spawn(move || match fetch_shareable_content() {
            Ok(c) => {
                *cache.lock().unwrap_or_else(|e| e.into_inner()) = Some(Arc::new(SendRetained(c)));
            }
            Err(e) => log::debug!("Bildschirminhalt im Hintergrund nicht erneuert: {e}"),
        });
    }

    fn capture_all_with(
        &self,
        content: &SCShareableContent,
        own: OwnWindows,
    ) -> Result<Vec<Frame>> {
        let displays = unsafe { content.displays() };
        let excluded = Excluded::new(content, own);
        // Alle Monitore gleichzeitig anstossen, dann einsammeln.
        let pending = displays
            .iter()
            .map(|d| start_display_capture(&d, &excluded))
            .collect::<Vec<_>>();
        pending.into_iter().map(wait_frame).collect()
    }
}

impl ScreenCapturer for MacCapturer {
    fn has_permission(&self) -> bool {
        CGPreflightScreenCaptureAccess()
    }

    fn request_permission(&self) -> bool {
        let granted = CGRequestScreenCaptureAccess();
        log::info!("Freigabe Bildschirmaufnahme angefragt: erteilt={granted}");
        granted
    }

    fn cursor_position(&self) -> Option<(f32, f32)> {
        // Quartz-Koordinaten: Ursprung oben links am Hauptmonitor.
        let event = CGEvent::new(None)?;
        let p = CGEvent::location(Some(&event));
        Some((p.x as f32, p.y as f32))
    }

    fn mouse_button_down(&self) -> bool {
        // Liest nur den Zustand – braucht keine Freigabe.
        CGEventSource::button_state(
            CGEventSourceStateID::CombinedSessionState,
            CGMouseButton::Left,
        )
    }

    fn warm_up(&self) {
        if self.has_permission() {
            self.refresh_later();
        }
    }

    fn displays(&self) -> Result<Vec<Display>> {
        let content = self.content()?;
        let displays = unsafe { content.0.displays() };
        let displays: Vec<Display> = displays.iter().map(|d| display_info(&d)).collect();
        log::debug!("{} Monitor(e)", displays.len());
        for d in &displays {
            log::debug!(
                "Monitor {}: {}×{} bei ({}, {}), Skalierung {}, Hauptmonitor={}",
                d.id,
                d.bounds.width,
                d.bounds.height,
                d.bounds.x,
                d.bounds.y,
                d.scale_factor,
                d.is_primary
            );
        }
        Ok(displays)
    }

    fn windows(&self, own_windows: OwnWindows) -> Result<Vec<WindowInfo>> {
        let started = Instant::now();
        let content = self.fresh_content()?;
        let own_pid = std::process::id() as i32;
        let sc_windows = unsafe { content.0.windows() };
        // Eigene Fenster nur auf Wunsch – dann auch das schwebende Tray-Fenster.
        let own_app = match own_windows {
            OwnWindows::Hidden => Vec::new(),
            OwnWindows::Shown => own_app_windows(&content.0),
        };
        let mut result = Vec::new();
        for id in window_ids_front_to_back() {
            let Some(w) = sc_windows.iter().find(|w| unsafe { w.windowID() } == id) else {
                continue;
            };
            let frame = unsafe { w.frame() };
            let app = unsafe { w.owningApplication() };
            let is_own = app
                .as_ref()
                .is_some_and(|a| unsafe { a.processID() } == own_pid);
            let normal = if is_own {
                own_app.iter().any(|o| unsafe { o.windowID() } == id)
            } else {
                unsafe { w.windowLayer() == 0 && w.isOnScreen() }
            };
            if !normal || frame.size.width < MIN_WINDOW_SIZE || frame.size.height < MIN_WINDOW_SIZE
            {
                continue;
            }
            result.push(WindowInfo {
                id,
                bounds: Rect::new(
                    frame.origin.x as f32,
                    frame.origin.y as f32,
                    frame.size.width as f32,
                    frame.size.height as f32,
                ),
                app_name: app
                    .map(|a| unsafe { a.applicationName() }.to_string())
                    .unwrap_or_default(),
                title: unsafe { w.title() }
                    .map(|t| t.to_string())
                    .unwrap_or_default(),
            });
        }
        log::debug!(
            "{} Fenster aufgelistet ({own_windows:?}) in {} ms",
            result.len(),
            started.elapsed().as_millis()
        );
        Ok(result)
    }

    fn window_bounds(&self, window_id: u32) -> Option<Rect> {
        // Ohne ScreenCaptureKit: Die Fensterliste von Core Graphics ist
        // synchron und schnell genug für jeden Takt der Aufnahmeleiste.
        let array =
            CGWindowListCopyWindowInfo(CGWindowListOption::OptionIncludingWindow, window_id)?;
        let array: &CFArray = &array;
        if array.count() == 0 {
            return None;
        }
        // SAFETY: Die Einträge sind CFDictionary; „kCGWindowBounds“ ist ein
        // Dictionary mit X, Y, Width, Height.
        unsafe {
            let info = &*(array.value_at_index(0) as *const CFDictionary);
            let bounds = info.value(kCGWindowBounds as *const _ as *const c_void);
            let bounds = (bounds as *const CFDictionary).as_ref()?;
            let mut r = CGRect::default();
            if !CGRectMakeWithDictionaryRepresentation(Some(bounds), &mut r) {
                return None;
            }
            Some(Rect::new(
                r.origin.x as f32,
                r.origin.y as f32,
                r.size.width as f32,
                r.size.height as f32,
            ))
        }
    }

    fn capture_window(&self, window_id: u32) -> Result<Frame> {
        let started = Instant::now();
        let content = self.fresh_content()?;
        let window = unsafe { content.0.windows() }
            .iter()
            .find(|w| unsafe { w.windowID() } == window_id)
            .ok_or_else(|| {
                log::warn!("Fenster {window_id} zum Aufnehmen nicht gefunden");
                CaptureError::Backend(format!("Fenster {window_id} nicht gefunden"))
            })?;
        let rx = start_window_capture(&window, &content.0);
        let frame = wait_frame(rx);
        match &frame {
            Ok(f) => log::info!(
                "Fenster {window_id} aufgenommen: {}×{} px, Skalierung {} in {} ms",
                f.image.width(),
                f.image.height(),
                f.scale_factor,
                started.elapsed().as_millis()
            ),
            Err(e) => log::error!("Fenster {window_id} aufnehmen fehlgeschlagen: {e}"),
        }
        self.refresh_later();
        frame
    }

    fn capture_region(&self, display_id: u32, rect: PixelRect) -> Result<Frame> {
        let content = self.content()?;
        let display = unsafe { content.0.displays() }
            .iter()
            .find(|d| unsafe { d.displayID() } == display_id)
            .ok_or_else(|| {
                log::warn!("Ausschnitt: Monitor {display_id} nicht gefunden");
                CaptureError::UnknownDisplay(display_id)
            })?;
        let info = display_info(&display);
        let excluded = Excluded::new(&content.0, OwnWindows::Hidden);
        let scale = info.scale_factor.max(1.0) as f64;
        let source = CGRect::new(
            CGPoint::new(rect.x as f64 / scale, rect.y as f64 / scale),
            CGSize::new(rect.width as f64 / scale, rect.height as f64 / scale),
        );
        let rx = start_display_capture_rect(
            &display,
            &excluded,
            Some((source, rect.width as usize, rect.height as usize)),
        );
        wait_frame(rx)
    }

    fn has_input_permission(&self) -> bool {
        CGPreflightPostEventAccess()
    }

    fn request_input_permission(&self) -> bool {
        CGRequestPostEventAccess()
    }

    fn scroll_at(&self, at: (f32, f32), delta: i32) -> Result<()> {
        let point = CGPoint::new(at.0 as f64, at.1 as f64);
        // Mauszeiger über den Bereich – Scroll-Ereignisse gehen an das Fenster
        // unter dem Zeiger.
        let warped = CGWarpMouseCursorPosition(point);
        if warped != CGError::Success {
            log::debug!("Mauszeiger versetzen fehlgeschlagen: CGError {}", warped.0);
        }
        let event =
            CGEvent::new_scroll_wheel_event2(None, CGScrollEventUnit::Pixel, 1, -delta, 0, 0)
                .ok_or_else(|| {
                    log::warn!("Scroll-Ereignis konnte nicht erzeugt werden");
                    CaptureError::Backend("Scroll-Ereignis".into())
                })?;
        CGEvent::set_location(Some(&event), point);
        CGEvent::post(CGEventTapLocation::HIDEventTap, Some(&event));
        Ok(())
    }

    fn capture_display(&self, display_id: u32) -> Result<Frame> {
        self.capture_all(OwnWindows::Hidden)?
            .into_iter()
            .find(|f| f.display_id == display_id)
            .ok_or(CaptureError::UnknownDisplay(display_id))
    }

    fn capture_all(&self, own: OwnWindows) -> Result<Vec<Frame>> {
        let started = Instant::now();
        let mut content = self.content()?;
        // Ein eigenes Fenster, das erst nach dem Schnappschuss aufging, kennt
        // dieser noch nicht – dann frisch abfragen.
        if own == OwnWindows::Shown {
            let known = own_windows(&content.0);
            if own_normal_window_ids()
                .iter()
                .any(|id| !known.iter().any(|w| unsafe { w.windowID() } == *id))
            {
                log::debug!("Neues eigenes Fenster – Bildschirminhalt frisch abfragen");
                content = self.fresh_content()?;
            }
        }
        let frames = match self.capture_all_with(&content.0, own) {
            Ok(f) => f,
            // Veralteter Schnappschuss (z. B. Monitor abgesteckt) – frisch versuchen.
            Err(e) => {
                log::warn!(
                    "Aufnahme mit zwischengespeichertem Inhalt fehlgeschlagen, neuer Versuch: {e}"
                );
                self.capture_all_with(&self.fresh_content()?.0, own)
                    .inspect_err(|e| log::error!("Monitore aufnehmen fehlgeschlagen: {e}"))?
            }
        };
        let sizes = frames
            .iter()
            .map(|f| {
                format!(
                    "{}: {}×{} @{}",
                    f.display_id,
                    f.image.width(),
                    f.image.height(),
                    f.scale_factor
                )
            })
            .collect::<Vec<_>>()
            .join(", ");
        log::info!(
            "{} Monitor(e) aufgenommen ({own:?}) in {} ms [{sizes}]",
            frames.len(),
            started.elapsed().as_millis()
        );
        self.refresh_later();
        Ok(frames)
    }
}

fn fetch_shareable_content() -> Result<Retained<SCShareableContent>> {
    if !CGPreflightScreenCaptureAccess() {
        log::warn!("Bildschirmaufnahme nicht freigegeben");
        return Err(CaptureError::PermissionDenied);
    }
    let started = Instant::now();
    let (tx, rx) = mpsc::sync_channel::<std::result::Result<SendRetained<_>, String>>(1);
    let block = RcBlock::new(
        move |content: *mut SCShareableContent, error: *mut NSError| {
            // SAFETY: ScreenCaptureKit übergibt gültige Zeiger oder null.
            let result = match unsafe { Retained::retain(content) } {
                Some(content) => Ok(SendRetained(content)),
                None => Err(error_text(error)),
            };
            let _ = tx.send(result);
        },
    );
    unsafe { SCShareableContent::getShareableContentWithCompletionHandler(&block) };
    match rx.recv_timeout(TIMEOUT) {
        Ok(Ok(content)) => {
            let content = content.0;
            log::debug!(
                "Bildschirminhalt abgefragt in {} ms: {} Monitor(e), {} Fenster, {} Apps",
                started.elapsed().as_millis(),
                unsafe { content.displays() }.count(),
                unsafe { content.windows() }.count(),
                unsafe { content.applications() }.count()
            );
            Ok(content)
        }
        Ok(Err(e)) => Err(CaptureError::Backend(e)),
        Err(_) => Err(CaptureError::Backend(
            "ScreenCaptureKit antwortet nicht".into(),
        )),
    }
}

/// Monitor-ID mit Lage und Grösse in Punkten (globale Koordinaten).
type DisplayLayout = Vec<(u32, [i64; 4])>;

fn rounded(r: CGRect) -> [i64; 4] {
    [r.origin.x, r.origin.y, r.size.width, r.size.height].map(|v| v.round() as i64)
}

/// Monitore im Schnappschuss, sortiert.
fn display_layout(content: &SCShareableContent) -> DisplayLayout {
    let mut layout: DisplayLayout = unsafe { content.displays() }
        .iter()
        .map(|d| unsafe { (d.displayID(), rounded(d.frame())) })
        .collect();
    layout.sort_unstable();
    layout
}

/// Monitore, die jetzt Bild zeigen (Core Graphics, synchron und schnell),
/// sortiert. Gespiegelte Monitore zählen wie bei ScreenCaptureKit nur einmal.
fn active_display_layout() -> DisplayLayout {
    const MAX: usize = 32;
    let mut ids = [0u32; MAX];
    let mut count = 0u32;
    // SAFETY: Puffer für MAX Einträge, Core Graphics schreibt höchstens so viele.
    let err = unsafe { CGGetActiveDisplayList(MAX as u32, ids.as_mut_ptr(), &mut count) };
    if err != CGError::Success {
        log::debug!("Aktive Monitore abfragen fehlgeschlagen: CGError {}", err.0);
        return Vec::new();
    }
    let mut layout: DisplayLayout = ids[..count as usize]
        .iter()
        .copied()
        .filter(|&id| CGDisplayMirrorsDisplay(id) == kCGNullDirectDisplay)
        .map(|id| (id, rounded(CGDisplayBounds(id))))
        .collect();
    layout.sort_unstable();
    layout
}

/// Nabshot selbst – wird aus Monitor-Aufnahmen ausgeblendet (Overlay!).
fn own_app(content: &SCShareableContent) -> Option<Retained<SCRunningApplication>> {
    let pid = std::process::id() as i32;
    unsafe { content.applications() }
        .iter()
        .find(|a| unsafe { a.processID() } == pid)
}

/// Ebene des Overlays (`NSScreenSaverWindowLevel`): Auswahl, Aufnahmeleiste
/// und Hinweise liegen hier und sind nie im Bild.
const OVERLAY_LAYER: isize = 1000;

/// Sichtbare Fenster von Nabshot unter dem Overlay im Schnappschuss –
/// Editor, Einstellungen, Tray-Fenster, aber auch die Vorschaubilder.
fn own_windows(content: &SCShareableContent) -> Vec<Retained<SCWindow>> {
    let pid = std::process::id() as i32;
    unsafe { content.windows() }
        .iter()
        .filter(|w| unsafe {
            w.windowLayer() < OVERLAY_LAYER as _
                && w.isOnScreen()
                && w.owningApplication().is_some_and(|a| a.processID() == pid)
        })
        .collect()
}

/// Fenster der App mit Titel (Editor, Einstellungen, Tray-Fenster) – die
/// Vorschaubilder nach einer Aufnahme haben keinen und bleiben draussen.
fn own_app_windows(content: &SCShareableContent) -> Vec<Retained<SCWindow>> {
    own_windows(content)
        .into_iter()
        .filter(|w| unsafe { w.title() }.is_some_and(|t| t.length() > 0))
        .collect()
}

/// Wie [`own_windows`], aber jetzt (Core Graphics, synchron und schnell).
fn own_normal_window_ids() -> Vec<u32> {
    let options =
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
    let Some(array) = CGWindowListCopyWindowInfo(options, kCGNullWindowID) else {
        return Vec::new();
    };
    let array: &CFArray = &array;
    let pid = std::process::id() as i64;
    let number = |info: &CFDictionary, key: &CFString| -> Option<i64> {
        // SAFETY: Die Einträge zu diesen Schlüsseln sind CFNumber.
        unsafe {
            let value = info.value(key as *const _ as *const c_void);
            (value as *const CFNumber).as_ref()?.as_i64()
        }
    };
    (0..array.count())
        .filter_map(|i| {
            // SAFETY: Die Einträge sind CFDictionary.
            let info = unsafe { &*(array.value_at_index(i) as *const CFDictionary) };
            let own = number(info, unsafe { kCGWindowOwnerPID }) == Some(pid);
            let normal =
                number(info, unsafe { kCGWindowLayer }).is_some_and(|l| l < OVERLAY_LAYER as i64);
            (own && normal)
                .then(|| number(info, unsafe { kCGWindowNumber }))
                .flatten()
                .map(|id| id as u32)
        })
        .collect()
}

/// Was in einer Monitor-Aufnahme fehlen soll: Nabshot selbst, ausser
/// den normalen Fenstern bei [`OwnWindows::Shown`].
struct Excluded {
    app: Option<Retained<SCRunningApplication>>,
    except: Vec<Retained<SCWindow>>,
}

impl Excluded {
    fn new(content: &SCShareableContent, own: OwnWindows) -> Self {
        Self {
            app: own_app(content),
            except: match own {
                OwnWindows::Hidden => Vec::new(),
                // Nur Fenster, die jetzt zu sehen sind. Der Schnappschuss kann
                // älter sein: Ein inzwischen verstecktes Fenster (⌘H, Editor
                // schliessen) stünde dort noch als sichtbar, und
                // ScreenCaptureKit zeichnete es trotzdem – mit altem Inhalt
                // obenauf oder, wenn macOS den schon verworfen hat, als graue
                // Fläche.
                OwnWindows::Shown => {
                    let visible = own_normal_window_ids();
                    let known = own_app_windows(content);
                    let total = known.len();
                    let shown: Vec<_> = known
                        .into_iter()
                        .filter(|w| visible.contains(&unsafe { w.windowID() }))
                        .collect();
                    log::debug!(
                        "Eigene Fenster im Bild: {} (im Schnappschuss {total})",
                        shown.len()
                    );
                    shown
                }
            },
        }
    }
}

/// Fenster-IDs in Bildschirm-Reihenfolge (vorne zuerst).
fn window_ids_front_to_back() -> Vec<u32> {
    let options =
        CGWindowListOption::OptionOnScreenOnly | CGWindowListOption::ExcludeDesktopElements;
    let Some(array) = CGWindowListCreate(options, kCGNullWindowID) else {
        return Vec::new();
    };
    let array: &CFArray = &array;
    (0..array.count())
        // SAFETY: CGWindowListCreate liefert CGWindowIDs als Zeigerwerte.
        .map(|i| unsafe { array.value_at_index(i) } as usize as u32)
        .collect()
}

fn display_info(d: &SCDisplay) -> Display {
    let id = unsafe { d.displayID() };
    let frame = unsafe { d.frame() };
    let (px_w, _) = pixel_size(d);
    Display {
        id,
        name: format!("Display {id}"),
        bounds: Rect::new(
            frame.origin.x as f32,
            frame.origin.y as f32,
            frame.size.width as f32,
            frame.size.height as f32,
        ),
        scale_factor: px_w as f32 / frame.size.width.max(1.0) as f32,
        is_primary: id == CGMainDisplayID(),
    }
}

/// Physische Pixelgrösse des Monitors (bei Retina doppelt so viele wie Punkte).
fn pixel_size(d: &SCDisplay) -> (usize, usize) {
    let id = unsafe { d.displayID() };
    let mode = CGDisplayCopyDisplayMode(id);
    let w = CGDisplayMode::pixel_width(mode.as_deref());
    let h = CGDisplayMode::pixel_height(mode.as_deref());
    if w > 0 && h > 0 {
        (w, h)
    } else {
        (unsafe { d.width() } as usize, unsafe { d.height() }
            as usize)
    }
}

type PendingFrame = mpsc::Receiver<std::result::Result<Frame, String>>;

/// Startet die Aufnahme eines Monitors; das Ergebnis kommt über den Kanal.
fn start_display_capture(display: &SCDisplay, excluded: &Excluded) -> PendingFrame {
    start_display_capture_rect(display, excluded, None)
}

/// Wie oben, optional nur ein Ausschnitt (Punkte im Monitor + Pixelgrösse).
fn start_display_capture_rect(
    display: &SCDisplay,
    excluded: &Excluded,
    source: Option<(CGRect, usize, usize)>,
) -> PendingFrame {
    let info = display_info(display);
    let (px_w, px_h) = match source {
        Some((_, w, h)) => (w, h),
        None => pixel_size(display),
    };
    let apps = match &excluded.app {
        Some(app) => NSArray::from_slice(&[&**app]),
        None => NSArray::new(),
    };
    let except: Vec<&SCWindow> = excluded.except.iter().map(|w| &**w).collect();
    let filter = unsafe {
        SCContentFilter::initWithDisplay_excludingApplications_exceptingWindows(
            SCContentFilter::alloc(),
            display,
            &apps,
            &NSArray::from_slice(&except),
        )
    };
    let config = unsafe { SCStreamConfiguration::new() };
    unsafe {
        config.setWidth(px_w);
        config.setHeight(px_h);
        config.setShowsCursor(false);
        // In sRGB umrechnen lassen, damit Farben ohne eingebettetes Profil
        // (PNG, Zwischenablage) überall gleich aussehen.
        config.setColorSpaceName(kCGColorSpaceSRGB);
        if let Some((rect, _, _)) = source {
            config.setSourceRect(rect);
        }
    }
    screenshot(&filter, &config, info.id, info.scale_factor)
}

fn start_window_capture(window: &SCWindow, content: &SCShareableContent) -> PendingFrame {
    let frame = unsafe { window.frame() };
    // Skalierung des Monitors, auf dem die Fenstermitte liegt.
    let center = (
        frame.origin.x + frame.size.width / 2.0,
        frame.origin.y + frame.size.height / 2.0,
    );
    let display = unsafe { content.displays() }.iter().find(|d| {
        let f = unsafe { d.frame() };
        center.0 >= f.origin.x
            && center.1 >= f.origin.y
            && center.0 < f.origin.x + f.size.width
            && center.1 < f.origin.y + f.size.height
    });
    let (display_id, scale) = display
        .map(|d| {
            let info = display_info(&d);
            (info.id, info.scale_factor)
        })
        .unwrap_or_else(|| {
            log::debug!("Fenstermitte auf keinem Monitor – nehme Hauptmonitor, Skalierung 2");
            (CGMainDisplayID(), 2.0)
        });

    let filter = unsafe {
        SCContentFilter::initWithDesktopIndependentWindow(SCContentFilter::alloc(), window)
    };
    let config = unsafe { SCStreamConfiguration::new() };
    unsafe {
        config.setWidth((frame.size.width * scale as f64).round() as usize);
        config.setHeight((frame.size.height * scale as f64).round() as usize);
        config.setShowsCursor(false);
        config.setColorSpaceName(kCGColorSpaceSRGB);
        // Runde Ecken bleiben durchsichtig.
        config.setShouldBeOpaque(false);
        config.setIgnoreShadowsSingleWindow(true);
    }
    screenshot(&filter, &config, display_id, scale)
}

fn screenshot(
    filter: &SCContentFilter,
    config: &SCStreamConfiguration,
    display_id: u32,
    scale_factor: f32,
) -> PendingFrame {
    let (tx, rx) = mpsc::sync_channel(1);
    let block = RcBlock::new(move |image: *mut CGImage, error: *mut NSError| {
        let result = match NonNull::new(image) {
            // SAFETY: gültiges CGImage von ScreenCaptureKit; wir behalten eine
            // eigene Referenz, solange wir die Pixel kopieren.
            Some(image) => {
                let image = unsafe { CFRetained::retain(image) };
                cgimage_to_rgba(&image).map(|image| Frame {
                    display_id,
                    scale_factor,
                    image,
                })
            }
            None => Err(error_text(error)),
        };
        if tx.send(result).is_err() {
            log::debug!("Bildschirmaufnahme kam erst nach dem Zeitlimit");
        }
    });
    unsafe {
        SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(
            filter,
            config,
            Some(&block),
        );
    }
    rx
}

fn wait_frame(rx: PendingFrame) -> Result<Frame> {
    match rx.recv_timeout(TIMEOUT) {
        Ok(Ok(frame)) => Ok(frame),
        Ok(Err(e)) => Err(CaptureError::Backend(e)),
        Err(_) => {
            log::warn!(
                "Bildschirmaufnahme: keine Antwort nach {} s",
                TIMEOUT.as_secs()
            );
            Err(CaptureError::Backend(
                "Bildschirmaufnahme hat zu lange gedauert".into(),
            ))
        }
    }
}

/// Zeichnet das CGImage in einen RGBA-Puffer. Das funktioniert für jedes
/// Quellformat und rechnet nötigenfalls Farben um.
fn cgimage_to_rgba(image: &CGImage) -> std::result::Result<RgbaImage, String> {
    let width = CGImage::width(Some(image));
    let height = CGImage::height(Some(image));
    let bytes_per_row = width * 4;
    let mut data = vec![0u8; bytes_per_row * height];

    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))
        .ok_or("sRGB-Farbraum nicht verfügbar")?;
    // RGBA in Speicherreihenfolge: Alpha zuletzt, Byte-Reihenfolge „Big“.
    let info = CGImageAlphaInfo::PremultipliedLast.0 | CGImageByteOrderInfo::Order32Big.0;
    // SAFETY: `data` lebt länger als der Kontext und ist gross genug.
    let ctx = unsafe {
        CGBitmapContextCreate(
            data.as_mut_ptr() as *mut c_void,
            width,
            height,
            8,
            bytes_per_row,
            Some(&space),
            info,
        )
    }
    .ok_or("Bitmap-Kontext konnte nicht angelegt werden")?;
    CGContext::draw_image(
        Some(&ctx),
        CGRect::new(
            CGPoint::new(0.0, 0.0),
            CGSize::new(width as f64, height as f64),
        ),
        Some(image),
    );
    drop(ctx);
    unpremultiply(&mut data);
    RgbaImage::new(width as u32, height as u32, data).map_err(|e| e.to_string())
}

/// Vormultiplizierte Farben → gerade (nur bei halbdurchsichtigen Pixeln nötig,
/// z. B. runde Fensterecken).
fn unpremultiply(data: &mut [u8]) {
    for px in data.as_chunks_mut::<4>().0 {
        let a = px[3] as u32;
        if a != 0 && a != 255 {
            for c in &mut px[..3] {
                *c = ((*c as u32 * 255 + a / 2) / a).min(255) as u8;
            }
        }
    }
}

fn error_text(error: *mut NSError) -> String {
    // SAFETY: null oder gültiges NSError.
    match unsafe { Retained::retain(error) } {
        Some(e) => e.localizedDescription().to_string(),
        None => "unbekannter Fehler".into(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Braucht Bildschirm und Freigabe: `cargo test -p nabshot-capture -- --ignored`.
    /// Melden ScreenCaptureKit und Core Graphics die Monitore verschieden,
    /// würde [`MacCapturer::content`] bei jeder Aufnahme frisch abfragen.
    #[test]
    #[ignore]
    fn snapshot_layout_matches_core_graphics() {
        let content = fetch_shareable_content().expect("Bildschirminhalt");
        assert_eq!(display_layout(&content), active_display_layout());
    }
}
