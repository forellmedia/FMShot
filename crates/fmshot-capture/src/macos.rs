//! macOS: ScreenCaptureKit (`SCScreenshotManager`, ab macOS 14).

use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::mpsc;
use std::time::Duration;

use block2::RcBlock;
use fmshot_core::{Rect, RgbaImage};
use objc2::AllocAnyThread;
use objc2::rc::Retained;
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGDisplayCopyDisplayMode, CGDisplayMode,
    CGImage, CGImageAlphaInfo, CGImageByteOrderInfo, CGMainDisplayID,
    CGPreflightScreenCaptureAccess, CGRequestScreenCaptureAccess, kCGColorSpaceSRGB,
};
use objc2_foundation::{NSArray, NSError};
use objc2_screen_capture_kit::{
    SCContentFilter, SCDisplay, SCScreenshotManager, SCShareableContent, SCStreamConfiguration,
};

use crate::{CaptureError, Display, Frame, Result, ScreenCapturer};

/// Wie lange auf ScreenCaptureKit gewartet wird, bevor wir aufgeben.
const TIMEOUT: Duration = Duration::from_secs(5);

pub struct MacCapturer;

impl MacCapturer {
    pub fn new() -> Self {
        Self
    }
}

/// ScreenCaptureKit-Objekte sind unveränderlich und dürfen zwischen Threads
/// wandern; objc2 markiert sie vorsichtshalber nicht als `Send`.
struct SendRetained<T>(Retained<T>);
// SAFETY: siehe oben – SCShareableContent ist ein unveränderlicher Schnappschuss.
unsafe impl<T> Send for SendRetained<T> {}

impl ScreenCapturer for MacCapturer {
    fn has_permission(&self) -> bool {
        CGPreflightScreenCaptureAccess()
    }

    fn request_permission(&self) -> bool {
        CGRequestScreenCaptureAccess()
    }

    fn displays(&self) -> Result<Vec<Display>> {
        let content = shareable_content()?;
        let displays = unsafe { content.displays() };
        Ok(displays.iter().map(|d| display_info(&d)).collect())
    }

    fn capture_display(&self, display_id: u32) -> Result<Frame> {
        let content = shareable_content()?;
        let display = find_display(&content, display_id)?;
        let rx = start_capture(&display)?;
        wait_frame(rx)
    }

    fn capture_all(&self) -> Result<Vec<Frame>> {
        let content = shareable_content()?;
        let displays = unsafe { content.displays() };
        // Alle Monitore gleichzeitig anstossen, dann einsammeln.
        let pending = displays
            .iter()
            .map(|d| start_capture(&d))
            .collect::<Result<Vec<_>>>()?;
        pending.into_iter().map(wait_frame).collect()
    }
}

fn shareable_content() -> Result<Retained<SCShareableContent>> {
    if !CGPreflightScreenCaptureAccess() {
        return Err(CaptureError::PermissionDenied);
    }
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
        Ok(Ok(content)) => Ok(content.0),
        Ok(Err(e)) => Err(CaptureError::Backend(e)),
        Err(_) => Err(CaptureError::Backend(
            "ScreenCaptureKit antwortet nicht".into(),
        )),
    }
}

fn find_display(content: &SCShareableContent, id: u32) -> Result<Retained<SCDisplay>> {
    unsafe { content.displays() }
        .iter()
        .find(|d| unsafe { d.displayID() } == id)
        .ok_or(CaptureError::UnknownDisplay(id))
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
fn start_capture(display: &SCDisplay) -> Result<PendingFrame> {
    let info = display_info(display);
    let (px_w, px_h) = pixel_size(display);

    let filter = unsafe {
        SCContentFilter::initWithDisplay_excludingWindows(
            SCContentFilter::alloc(),
            display,
            &NSArray::new(),
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
    }

    let (tx, rx) = mpsc::sync_channel(1);
    let block = RcBlock::new(move |image: *mut CGImage, error: *mut NSError| {
        let result = match NonNull::new(image) {
            // SAFETY: gültiges CGImage von ScreenCaptureKit; wir behalten eine
            // eigene Referenz, solange wir die Pixel kopieren.
            Some(image) => {
                let image = unsafe { CFRetained::retain(image) };
                cgimage_to_rgba(&image).map(|image| Frame {
                    display_id: info.id,
                    scale_factor: info.scale_factor,
                    image,
                })
            }
            None => Err(error_text(error)),
        };
        let _ = tx.send(result);
    });
    unsafe {
        SCScreenshotManager::captureImageWithFilter_configuration_completionHandler(
            &filter,
            &config,
            Some(&block),
        );
    }
    Ok(rx)
}

fn wait_frame(rx: PendingFrame) -> Result<Frame> {
    match rx.recv_timeout(TIMEOUT) {
        Ok(Ok(frame)) => Ok(frame),
        Ok(Err(e)) => Err(CaptureError::Backend(e)),
        Err(_) => Err(CaptureError::Backend(
            "Bildschirmaufnahme hat zu lange gedauert".into(),
        )),
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
    // Bildschirme sind deckend – vormultipliziert ist hier gleich gerade.
    RgbaImage::new(width as u32, height as u32, data).map_err(|e| e.to_string())
}

fn error_text(error: *mut NSError) -> String {
    // SAFETY: null oder gültiges NSError.
    match unsafe { Retained::retain(error) } {
        Some(e) => e.localizedDescription().to_string(),
        None => "unbekannter Fehler".into(),
    }
}
