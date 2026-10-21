//! Zwischenablage.
//!
//! - macOS: PNG mit Auflösungsangabe als `NSImage` – Retina-Aufnahmen werden
//!   beim Einfügen in Punktgrösse dargestellt, nicht doppelt so gross.
//! - Windows: PNG (mit Auflösung, für Office & Co.) und `CF_DIBV5` (mit
//!   Alpha und DPI, für alle anderen) – Aufnahmen mit 150 % Skalierung landen
//!   in Word in richtiger Grösse.
//! - Linux: `arboard` (Rohpixel).

use fmshot_core::RgbaImage;

#[derive(Debug, thiserror::Error)]
pub enum ClipboardError {
    #[error("Bild konnte nicht kodiert werden: {0}")]
    Encode(String),
    #[error("Zwischenablage nicht verfügbar: {0}")]
    Unavailable(String),
}

/// Legt ein Bild in die Zwischenablage. `scale_factor` = physische Pixel pro
/// Punkt (2.0 bei Retina).
pub fn copy_image(image: &RgbaImage, scale_factor: f32) -> Result<(), ClipboardError> {
    #[cfg(target_os = "macos")]
    return macos::copy_image(image, scale_factor);
    #[cfg(target_os = "windows")]
    return windows::copy_image(image, scale_factor);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    {
        let _ = scale_factor;
        let data = arboard::ImageData {
            width: image.width() as usize,
            height: image.height() as usize,
            bytes: std::borrow::Cow::Borrowed(image.data()),
        };
        arboard::Clipboard::new()
            .and_then(|mut c| c.set_image(data))
            .map_err(|e| ClipboardError::Unavailable(e.to_string()))
    }
}

/// Legt Text in die Zwischenablage (z. B. OCR-Ergebnis).
pub fn copy_text(text: &str) -> Result<(), ClipboardError> {
    #[cfg(target_os = "macos")]
    return macos::copy_text(text);
    #[cfg(target_os = "windows")]
    return windows::copy_text(text);
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    arboard::Clipboard::new()
        .and_then(|mut c| c.set_text(text.to_owned()))
        .map_err(|e| ClipboardError::Unavailable(e.to_string()))
}

/// Legt eine Datei in die Zwischenablage (z. B. ein Video) – Einfügen in
/// Finder, Mail oder Chat-Apps fügt die Datei ein.
pub fn copy_file(path: &std::path::Path) -> Result<(), ClipboardError> {
    #[cfg(target_os = "macos")]
    return macos::copy_file(path);
    #[cfg(not(target_os = "macos"))]
    {
        let _ = path;
        Err(ClipboardError::Unavailable(
            "Dateien kopieren folgt auf dieser Plattform".into(),
        ))
    }
}

/// Zähler, der bei jeder Änderung der Zwischenablage weiterzählt – auch
/// durch andere Programme. So lässt sich prüfen, ob noch die eigene Kopie
/// drinliegt. `None`, wo es keinen gibt (Linux).
pub fn change_count() -> Option<u32> {
    #[cfg(target_os = "macos")]
    return Some(macos::change_count());
    #[cfg(target_os = "windows")]
    return Some(windows::change_count());
    #[cfg(not(any(target_os = "macos", target_os = "windows")))]
    None
}

#[cfg(target_os = "windows")]
mod windows;

#[cfg(target_os = "macos")]
mod macos {
    use objc2::AllocAnyThread;
    use objc2::runtime::ProtocolObject;
    use objc2_app_kit::{NSImage, NSPasteboard, NSPasteboardTypeString};
    use objc2_foundation::{NSArray, NSData, NSString, NSURL};

    use super::*;

    pub fn copy_file(path: &std::path::Path) -> Result<(), ClipboardError> {
        let url = NSURL::fileURLWithPath(&NSString::from_str(&path.to_string_lossy()));
        let pb = NSPasteboard::generalPasteboard();
        pb.clearContents();
        let objects = NSArray::from_slice(&[ProtocolObject::from_ref(&*url)]);
        if pb.writeObjects(&objects) {
            Ok(())
        } else {
            Err(ClipboardError::Unavailable("writeObjects".into()))
        }
    }

    pub fn copy_image(image: &RgbaImage, scale_factor: f32) -> Result<(), ClipboardError> {
        let png = fmshot_library::encode_png(image, scale_factor)
            .map_err(|e| ClipboardError::Encode(e.to_string()))?;
        let data = NSData::with_bytes(&png);
        // NSImage liest die Auflösung aus dem PNG und setzt die Grösse in Punkten.
        let ns_image = NSImage::initWithData(NSImage::alloc(), &data)
            .ok_or_else(|| ClipboardError::Encode("NSImage aus PNG".into()))?;
        let pb = NSPasteboard::generalPasteboard();
        pb.clearContents();
        let objects = NSArray::from_slice(&[ProtocolObject::from_ref(&*ns_image)]);
        if pb.writeObjects(&objects) {
            Ok(())
        } else {
            Err(ClipboardError::Unavailable("writeObjects".into()))
        }
    }

    pub fn change_count() -> u32 {
        // Nur Gleichheit zählt – Abschneiden auf 32 Bit schadet nicht.
        NSPasteboard::generalPasteboard().changeCount() as u32
    }

    pub fn copy_text(text: &str) -> Result<(), ClipboardError> {
        let pb = NSPasteboard::generalPasteboard();
        pb.clearContents();
        let s = NSString::from_str(text);
        if pb.setString_forType(&s, unsafe { NSPasteboardTypeString }) {
            Ok(())
        } else {
            Err(ClipboardError::Unavailable("setString".into()))
        }
    }
}
