//! Windows-Zwischenablage über die Win32-API.
//!
//! Bilder liegen zweifach drin: als registriertes Format „PNG“ (Office,
//! Browser und Chat-Apps nehmen es bevorzugt, inklusive Auflösung) und als
//! `CF_DIBV5` mit Alpha, sRGB und DPI. `CF_DIB` / `CF_BITMAP` erzeugt Windows
//! daraus selbst.

use std::time::Duration;

use fmshot_core::RgbaImage;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HGLOBAL};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, GetClipboardSequenceNumber, OpenClipboard,
    RegisterClipboardFormatW, SetClipboardData,
};
use windows::Win32::System::Memory::{GMEM_MOVEABLE, GlobalAlloc, GlobalLock, GlobalUnlock};
use windows::Win32::System::Ole::{CF_DIBV5, CF_UNICODETEXT};
use windows::core::w;

use crate::ClipboardError;

/// Windows rechnet in 96 dpi pro logischem Pixel (macOS: 72 dpi pro Punkt).
const BASE_DPI: f32 = 96.0;
/// Die PNG-Kodierung der Bibliothek rechnet mit 72 dpi als Basis.
const PNG_BASE_DPI: f32 = 72.0;

pub fn copy_image(image: &RgbaImage, scale_factor: f32) -> Result<(), ClipboardError> {
    let scale = scale_factor.max(1.0);
    let png = fmshot_library::encode_png(image, scale * BASE_DPI / PNG_BASE_DPI)
        .map_err(|e| ClipboardError::Encode(e.to_string()))?;
    let dib = dibv5(image, scale);
    let png_format = unsafe { RegisterClipboardFormatW(w!("PNG")) };
    with_clipboard(|| {
        if png_format != 0 {
            set_data(png_format, &png)?;
        }
        set_data(CF_DIBV5.0 as u32, &dib)
    })
}

pub fn change_count() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

pub fn copy_text(text: &str) -> Result<(), ClipboardError> {
    // Windows-Programme erwarten CRLF und eine abschliessende Null.
    let utf16: Vec<u16> = text
        .replace("\r\n", "\n")
        .replace('\n', "\r\n")
        .encode_utf16()
        .chain(std::iter::once(0))
        .collect();
    let bytes: Vec<u8> = utf16.iter().flat_map(|c| c.to_le_bytes()).collect();
    with_clipboard(|| set_data(CF_UNICODETEXT.0 as u32, &bytes))
}

/// Öffnet die Zwischenablage (andere Programme halten sie manchmal kurz
/// offen – dann einige Male nachfassen), leert sie und führt `f` aus.
fn with_clipboard(f: impl FnOnce() -> Result<(), ClipboardError>) -> Result<(), ClipboardError> {
    let mut opened = false;
    for _ in 0..20 {
        if unsafe { OpenClipboard(None) }.is_ok() {
            opened = true;
            break;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    if !opened {
        return Err(ClipboardError::Unavailable(
            "Zwischenablage ist von einem anderen Programm belegt".into(),
        ));
    }
    let result = unsafe { EmptyClipboard() }
        .map_err(|e| ClipboardError::Unavailable(e.message()))
        .and_then(|()| f());
    let _ = unsafe { CloseClipboard() };
    result
}

/// Kopiert `bytes` in globalen Speicher und übergibt ihn der Zwischenablage.
fn set_data(format: u32, bytes: &[u8]) -> Result<(), ClipboardError> {
    let unavailable = |e: windows::core::Error| ClipboardError::Unavailable(e.message());
    let mem: HGLOBAL = unsafe { GlobalAlloc(GMEM_MOVEABLE, bytes.len()) }.map_err(unavailable)?;
    let ptr = unsafe { GlobalLock(mem) } as *mut u8;
    if ptr.is_null() {
        let _ = unsafe { GlobalFree(Some(mem)) };
        return Err(ClipboardError::Unavailable("GlobalLock".into()));
    }
    // SAFETY: `ptr` zeigt auf mindestens `bytes.len()` frisch belegte Bytes.
    unsafe { std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len()) };
    let _ = unsafe { GlobalUnlock(mem) };
    match unsafe { SetClipboardData(format, Some(HANDLE(mem.0))) } {
        // Der Speicher gehört jetzt der Zwischenablage.
        Ok(_) => Ok(()),
        Err(e) => {
            let _ = unsafe { GlobalFree(Some(mem)) };
            Err(unavailable(e))
        }
    }
}

/// `CF_DIBV5`: 124 Byte Kopf, danach BGRA-Zeilen von unten nach oben.
fn dibv5(image: &RgbaImage, scale: f32) -> Vec<u8> {
    const HEADER: u32 = 124;
    const BI_BITFIELDS: u32 = 3;
    const LCS_SRGB: u32 = 0x7352_4742; // 'sRGB'
    const LCS_GM_IMAGES: u32 = 4;
    let (w, h) = (image.width(), image.height());
    let size = w as usize * h as usize * 4;
    let ppm = (BASE_DPI * scale / 0.0254).round() as u32;

    let mut out = Vec::with_capacity(HEADER as usize + size);
    let mut put = |v: u32| out.extend_from_slice(&v.to_le_bytes());
    put(HEADER);
    put(w); // bV5Width
    put(h); // bV5Height > 0: von unten nach oben (verträgt jedes Programm)
    put(1 | (32 << 16)); // bV5Planes = 1, bV5BitCount = 32
    put(BI_BITFIELDS);
    put(size as u32);
    put(ppm); // bV5XPelsPerMeter
    put(ppm); // bV5YPelsPerMeter
    put(0); // bV5ClrUsed
    put(0); // bV5ClrImportant
    put(0x00FF_0000); // Rot
    put(0x0000_FF00); // Grün
    put(0x0000_00FF); // Blau
    put(0xFF00_0000); // Alpha
    put(LCS_SRGB);
    for _ in 0..9 {
        put(0); // bV5Endpoints (bei sRGB unbenutzt)
    }
    for _ in 0..3 {
        put(0); // Gamma Rot/Grün/Blau
    }
    put(LCS_GM_IMAGES);
    put(0); // bV5ProfileData
    put(0); // bV5ProfileSize
    put(0); // bV5Reserved
    debug_assert_eq!(out.len(), HEADER as usize);

    let row = w as usize * 4;
    for line in image.data().chunks_exact(row.max(1)).rev() {
        for px in line.as_chunks::<4>().0 {
            out.extend_from_slice(&[px[2], px[1], px[0], px[3]]);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn dib_kopf_und_pixel() {
        // 1 × 2: oben rot, unten halbdurchsichtig blau
        let img = RgbaImage::new(1, 2, vec![255, 0, 0, 255, 0, 0, 255, 128]).unwrap();
        let dib = dibv5(&img, 1.5);
        assert_eq!(dib.len(), 124 + 8);
        assert_eq!(u32::from_le_bytes(dib[0..4].try_into().unwrap()), 124);
        // 144 dpi ≈ 5669 Pixel pro Meter
        assert_eq!(u32::from_le_bytes(dib[24..28].try_into().unwrap()), 5669);
        // Erste Zeile im Speicher = unterste Bildzeile, BGRA
        assert_eq!(&dib[124..128], &[255, 0, 0, 128]);
        assert_eq!(&dib[128..132], &[0, 0, 255, 255]);
    }
}
