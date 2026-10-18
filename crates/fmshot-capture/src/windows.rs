//! Windows: GDI (`BitBlt` vom Bildschirm, `PrintWindow` für Fenster).
//!
//! GDI liest das fertig zusammengesetzte Bild des Desktop Window Managers
//! und läuft überall – auch in VMs mit einfachem Grafiktreiber, wo Desktop
//! Duplication oft fehlt. Ein ganzer 4K-Monitor dauert rund 20–40 ms, ein
//! Ausschnitt für die Scroll-Aufnahme nur wenige Millisekunden.
//!
//! **Koordinaten:** FMShot arbeitet unter Windows „per monitor aware v2“ in
//! physischen Pixeln des virtuellen Desktops (Ursprung oben links am
//! Hauptmonitor). Mit gemischten Skalierungen gibt es unter Windows kein
//! durchgehendes logisches Koordinatensystem; deshalb ist
//! [`Display::scale_factor`] hier immer 1.0. Die Skalierung des Monitors
//! (150 % → 1.5) steckt in [`Frame::scale_factor`].
//!
//! **Eigene Fenster:** Overlay, Vorschau und Hinweise setzen
//! `WDA_EXCLUDEFROMCAPTURE` und sind nie im Bild. Andere sichtbare Fenster von
//! FMShot (Editor) werden für die Dauer einer Aufnahme genauso ausgeblendet.

use std::ffi::c_void;
use std::mem::size_of;

use fmshot_core::{Rect, RgbaImage};
use windows::Wdk::System::SystemServices::RtlGetVersion;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, POINT, RECT, TRUE};
use windows::Win32::Graphics::Dwm::{
    DWMWA_CLOAKED, DWMWA_EXTENDED_FRAME_BOUNDS, DwmFlush, DwmGetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BitBlt, CAPTUREBLT, CreateCompatibleDC, CreateDIBSection,
    DIB_RGB_COLORS, DISPLAY_DEVICEW, DeleteDC, DeleteObject, EnumDisplayDevicesW,
    EnumDisplayMonitors, GdiFlush, GetDC, GetMonitorInfoW, HBITMAP, HDC, HGDIOBJ, HMONITOR,
    MONITOR_DEFAULTTONEAREST, MONITORINFO, MONITORINFOEXW, MonitorFromWindow, ReleaseDC, SRCCOPY,
    SelectObject,
};
use windows::Win32::Storage::Xps::{PRINT_WINDOW_FLAGS, PrintWindow};
use windows::Win32::System::SystemInformation::OSVERSIONINFOW;
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION,
    QueryFullProcessImageNameW,
};
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor,
    MDT_EFFECTIVE_DPI, SetThreadDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_WHEEL, MOUSEINPUT, SendInput,
    VK_LBUTTON,
};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GWL_EXSTYLE, GWL_STYLE, GetClassNameW, GetCursorPos, GetWindowDisplayAffinity,
    GetWindowLongW, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindow,
    IsWindowVisible, IsZoomed, MONITORINFOF_PRIMARY, SetCursorPos, SetWindowDisplayAffinity,
    WDA_EXCLUDEFROMCAPTURE, WDA_NONE, WS_CAPTION, WS_EX_APPWINDOW, WS_EX_TOOLWINDOW, WS_THICKFRAME,
};
use windows::core::{BOOL, PCWSTR};

use crate::{CaptureError, Display, Frame, PixelRect, Result, ScreenCapturer, WindowInfo};

/// Kleinere Fenster (Symbole, Ränder) sind keine sinnvollen Ziele.
const MIN_WINDOW_SIZE: i32 = 40;
/// `PW_RENDERFULLCONTENT`: auch GPU-gezeichnete Fenster (Browser, Spiele).
const PW_RENDERFULLCONTENT: u32 = 2;
/// Mausrad: eine Raste (120) scrollt in den meisten Programmen rund 100
/// logische Pixel.
const WHEEL_PER_POINT: f32 = 120.0 / 100.0;
/// Fensterklassen der Shell – Desktop und Taskleiste sind keine Fenster.
const SHELL_CLASSES: [&str; 4] = [
    "Progman",
    "WorkerW",
    "Shell_TrayWnd",
    "Shell_SecondaryTrayWnd",
];

pub struct WindowsCapturer;

impl WindowsCapturer {
    pub fn new() -> Self {
        Self
    }
}

impl ScreenCapturer for WindowsCapturer {
    fn displays(&self) -> Result<Vec<Display>> {
        let _dpi = DpiGuard::new();
        Ok(monitors()
            .iter()
            .map(|m| Display {
                id: m.id,
                name: m.name.clone(),
                bounds: rect_to_bounds(&m.rect),
                scale_factor: 1.0,
                is_primary: m.primary,
            })
            .collect())
    }

    fn cursor_position(&self) -> Option<(f32, f32)> {
        let _dpi = DpiGuard::new();
        let mut p = POINT::default();
        unsafe { GetCursorPos(&mut p) }.ok()?;
        Some((p.x as f32, p.y as f32))
    }

    fn mouse_button_down(&self) -> bool {
        // Höchstes Bit = Taste gerade gedrückt.
        let state = unsafe { GetAsyncKeyState(VK_LBUTTON.0 as i32) };
        state < 0
    }

    fn windows(&self) -> Result<Vec<WindowInfo>> {
        let _dpi = DpiGuard::new();
        let own = unsafe { GetCurrentProcessId() };
        let mut result = Vec::new();
        for hwnd in top_level_windows() {
            if let Some(info) = window_info(hwnd, own) {
                result.push(info);
            }
        }
        Ok(result)
    }

    fn capture_window(&self, window_id: u32) -> Result<Frame> {
        let _dpi = DpiGuard::new();
        let hwnd = hwnd_from_id(window_id);
        if !unsafe { IsWindow(Some(hwnd)) }.as_bool() {
            return Err(CaptureError::Backend(format!(
                "Fenster {window_id} nicht gefunden"
            )));
        }
        let monitor = unsafe { MonitorFromWindow(hwnd, MONITOR_DEFAULTTONEAREST) };
        let info = monitor_info(monitor);
        let frame = frame_bounds(hwnd)?;
        let mut image = print_window(hwnd, &frame).or_else(|_| {
            // Manche Programme zeichnen sich nicht für PrintWindow – dann den
            // sichtbaren Bereich vom Bildschirm nehmen.
            let _hidden = HiddenOwnWindows::hide();
            blit_screen(&frame)
        })?;
        if has_rounded_corners(hwnd) {
            let radius = 8.0 * info.as_ref().map_or(1.0, |m| m.scale);
            round_corners(&mut image, radius);
        }
        Ok(Frame {
            display_id: info.as_ref().map_or(0, |m| m.id),
            scale_factor: info.as_ref().map_or(1.0, |m| m.scale),
            image,
        })
    }

    fn capture_region(&self, display_id: u32, rect: PixelRect) -> Result<Frame> {
        let _dpi = DpiGuard::new();
        let m = find_monitor(display_id)?;
        let mw = (m.rect.right - m.rect.left).max(0) as u32;
        let mh = (m.rect.bottom - m.rect.top).max(0) as u32;
        let x = rect.x.min(mw);
        let y = rect.y.min(mh);
        let r = RECT {
            left: m.rect.left + x as i32,
            top: m.rect.top + y as i32,
            right: m.rect.left + (x + rect.width.min(mw - x)) as i32,
            bottom: m.rect.top + (y + rect.height.min(mh - y)) as i32,
        };
        Ok(Frame {
            display_id,
            scale_factor: m.scale,
            image: blit_screen(&r)?,
        })
    }

    fn has_input_permission(&self) -> bool {
        // Windows kennt keine Freigabe für simulierte Eingaben.
        true
    }

    fn request_input_permission(&self) -> bool {
        true
    }

    fn scroll_at(&self, at: (f32, f32), delta: i32) -> Result<()> {
        let _dpi = DpiGuard::new();
        // Das Mausrad wirkt auf das Fenster unter dem Zeiger.
        unsafe { SetCursorPos(at.0.round() as i32, at.1.round() as i32) }
            .map_err(|e| CaptureError::Backend(e.message()))?;
        let wheel = -((delta as f32 * WHEEL_PER_POINT).round() as i32);
        let input = INPUT {
            r#type: INPUT_MOUSE,
            Anonymous: INPUT_0 {
                mi: MOUSEINPUT {
                    // Negativ = nach unten; mouseData ist als DWORD deklariert.
                    mouseData: wheel as u32,
                    dwFlags: MOUSEEVENTF_WHEEL,
                    ..Default::default()
                },
            },
        };
        let sent = unsafe { SendInput(&[input], size_of::<INPUT>() as i32) };
        if sent == 1 {
            Ok(())
        } else {
            Err(CaptureError::Backend(
                "Scroll-Ereignis wurde nicht angenommen".into(),
            ))
        }
    }

    fn capture_display(&self, display_id: u32) -> Result<Frame> {
        let _dpi = DpiGuard::new();
        let m = find_monitor(display_id)?;
        let _hidden = HiddenOwnWindows::hide();
        Ok(Frame {
            display_id,
            scale_factor: m.scale,
            image: blit_screen(&m.rect)?,
        })
    }

    fn capture_all(&self) -> Result<Vec<Frame>> {
        let _dpi = DpiGuard::new();
        let monitors = monitors();
        if monitors.is_empty() {
            return Err(CaptureError::Backend("Keine Monitore gefunden".into()));
        }
        let _hidden = HiddenOwnWindows::hide();
        monitors
            .iter()
            .map(|m| {
                Ok(Frame {
                    display_id: m.id,
                    scale_factor: m.scale,
                    image: blit_screen(&m.rect)?,
                })
            })
            .collect()
    }
}

// --- Monitore ----------------------------------------------------------------

struct Monitor {
    id: u32,
    name: String,
    /// Physische Pixel im virtuellen Desktop.
    rect: RECT,
    scale: f32,
    primary: bool,
}

/// Kennung eines Monitors – dieselbe Rechnung steht in `fmshot-overlay`.
fn monitor_id(h: HMONITOR) -> u32 {
    h.0 as usize as u32
}

fn monitors() -> Vec<Monitor> {
    unsafe extern "system" fn collect(h: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        // SAFETY: `data` zeigt auf den Vec unten, solange EnumDisplayMonitors läuft.
        let list = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
        list.push(h);
        TRUE
    }
    let mut handles: Vec<HMONITOR> = Vec::new();
    let _ = unsafe {
        EnumDisplayMonitors(
            None,
            None,
            Some(collect),
            LPARAM(&mut handles as *mut _ as isize),
        )
    };
    handles.into_iter().filter_map(monitor_info).collect()
}

fn monitor_info(h: HMONITOR) -> Option<Monitor> {
    let mut info = MONITORINFOEXW {
        monitorInfo: MONITORINFO {
            cbSize: size_of::<MONITORINFOEXW>() as u32,
            ..Default::default()
        },
        ..Default::default()
    };
    unsafe { GetMonitorInfoW(h, &mut info.monitorInfo as *mut MONITORINFO) }
        .as_bool()
        .then_some(())?;
    let (mut dx, mut dy) = (96u32, 96u32);
    let _ = unsafe { GetDpiForMonitor(h, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) };
    let device = wide_to_string(&info.szDevice);
    Some(Monitor {
        id: monitor_id(h),
        name: monitor_name(&info.szDevice).unwrap_or(device),
        rect: info.monitorInfo.rcMonitor,
        scale: dx.max(96) as f32 / 96.0,
        primary: info.monitorInfo.dwFlags & MONITORINFOF_PRIMARY != 0,
    })
}

/// Anzeigename des Monitors, z. B. „Generic PnP Monitor“.
fn monitor_name(device: &[u16]) -> Option<String> {
    let mut dd = DISPLAY_DEVICEW {
        cb: size_of::<DISPLAY_DEVICEW>() as u32,
        ..Default::default()
    };
    unsafe { EnumDisplayDevicesW(PCWSTR(device.as_ptr()), 0, &mut dd, 0) }
        .as_bool()
        .then(|| wide_to_string(&dd.DeviceString))
        .filter(|s| !s.is_empty())
}

fn find_monitor(display_id: u32) -> Result<Monitor> {
    monitors()
        .into_iter()
        .find(|m| m.id == display_id)
        .ok_or(CaptureError::UnknownDisplay(display_id))
}

// --- Fenster -----------------------------------------------------------------

/// Sichtbare Fenster oberster Ebene in Z-Reihenfolge (vorne zuerst).
fn top_level_windows() -> Vec<HWND> {
    unsafe extern "system" fn collect(hwnd: HWND, data: LPARAM) -> BOOL {
        // SAFETY: `data` zeigt auf den Vec unten, solange EnumWindows läuft.
        let list = unsafe { &mut *(data.0 as *mut Vec<HWND>) };
        list.push(hwnd);
        TRUE
    }
    let mut list: Vec<HWND> = Vec::new();
    let _ = unsafe { EnumWindows(Some(collect), LPARAM(&mut list as *mut _ as isize)) };
    list
}

fn window_info(hwnd: HWND, own_pid: u32) -> Option<WindowInfo> {
    unsafe {
        if !IsWindowVisible(hwnd).as_bool() || IsIconic(hwnd).as_bool() || is_cloaked(hwnd) {
            return None;
        }
    }
    let ex = unsafe { GetWindowLongW(hwnd, GWL_EXSTYLE) } as u32;
    if ex & WS_EX_TOOLWINDOW.0 != 0 && ex & WS_EX_APPWINDOW.0 == 0 {
        return None;
    }
    let mut pid = 0u32;
    unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
    if pid == own_pid {
        return None;
    }
    let class = class_name(hwnd);
    if SHELL_CLASSES.contains(&class.as_str()) {
        return None;
    }
    let title = window_title(hwnd);
    if title.is_empty() {
        return None;
    }
    let r = frame_bounds(hwnd).ok()?;
    if r.right - r.left < MIN_WINDOW_SIZE || r.bottom - r.top < MIN_WINDOW_SIZE {
        return None;
    }
    Some(WindowInfo {
        id: hwnd_to_id(hwnd),
        bounds: rect_to_bounds(&r),
        app_name: process_name(pid).unwrap_or_default(),
        title,
    })
}

/// Fenster auf anderen virtuellen Desktops oder ausgeblendete UWP-Apps.
fn is_cloaked(hwnd: HWND) -> bool {
    let mut cloaked = 0u32;
    let ok = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_CLOAKED,
            &mut cloaked as *mut u32 as *mut c_void,
            size_of::<u32>() as u32,
        )
    };
    ok.is_ok() && cloaked != 0
}

/// Sichtbarer Fensterrahmen ohne die unsichtbaren Ränder zum Ziehen.
fn frame_bounds(hwnd: HWND) -> Result<RECT> {
    let mut r = RECT::default();
    let dwm = unsafe {
        DwmGetWindowAttribute(
            hwnd,
            DWMWA_EXTENDED_FRAME_BOUNDS,
            &mut r as *mut RECT as *mut c_void,
            size_of::<RECT>() as u32,
        )
    };
    if dwm.is_err() || r.right <= r.left || r.bottom <= r.top {
        unsafe { GetWindowRect(hwnd, &mut r) }.map_err(|e| CaptureError::Backend(e.message()))?;
    }
    Ok(r)
}

fn window_title(hwnd: HWND) -> String {
    let mut buf = [0u16; 512];
    let n = unsafe { GetWindowTextW(hwnd, &mut buf) }.max(0) as usize;
    String::from_utf16_lossy(&buf[..n.min(buf.len())])
}

fn class_name(hwnd: HWND) -> String {
    let mut buf = [0u16; 256];
    let n = unsafe { GetClassNameW(hwnd, &mut buf) }.max(0) as usize;
    String::from_utf16_lossy(&buf[..n.min(buf.len())])
}

/// Programmname ohne Endung, z. B. „chrome“.
fn process_name(pid: u32) -> Option<String> {
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) }.ok()?;
    let mut buf = [0u16; 1024];
    let mut len = buf.len() as u32;
    let ok = unsafe {
        QueryFullProcessImageNameW(
            handle,
            PROCESS_NAME_WIN32,
            windows::core::PWSTR(buf.as_mut_ptr()),
            &mut len,
        )
    };
    let _ = unsafe { CloseHandle(handle) };
    ok.ok()?;
    let path = String::from_utf16_lossy(&buf[..len as usize]);
    std::path::Path::new(&path)
        .file_stem()
        .map(|s| s.to_string_lossy().into_owned())
}

/// Fenster-Handles passen immer in 32 Bit (auch unter 64-Bit-Windows).
fn hwnd_to_id(hwnd: HWND) -> u32 {
    hwnd.0 as usize as u32
}

fn hwnd_from_id(id: u32) -> HWND {
    // Die oberen 32 Bit sind die Vorzeichenerweiterung der unteren.
    HWND(id as i32 as isize as *mut c_void)
}

/// Windows 11 rundet normale Fenster ab, ausser maximierte.
fn has_rounded_corners(hwnd: HWND) -> bool {
    if windows_build() < 22000 || unsafe { IsZoomed(hwnd) }.as_bool() {
        return false;
    }
    let style = unsafe { GetWindowLongW(hwnd, GWL_STYLE) } as u32;
    style & (WS_CAPTION.0 | WS_THICKFRAME.0) != 0
}

fn windows_build() -> u32 {
    let mut v = OSVERSIONINFOW {
        dwOSVersionInfoSize: size_of::<OSVERSIONINFOW>() as u32,
        ..Default::default()
    };
    // RtlGetVersion meldet die echte Version (GetVersionEx lügt ohne Manifest).
    if unsafe { RtlGetVersion(&mut v) }.is_ok() {
        v.dwBuildNumber
    } else {
        0
    }
}

// --- Eigene Fenster ausblenden ---------------------------------------------------

/// Blendet sichtbare Fenster von FMShot (z. B. den Editor) aus Aufnahmen
/// aus, solange der Wert lebt.
struct HiddenOwnWindows(Vec<HWND>);

impl HiddenOwnWindows {
    fn hide() -> Self {
        let own = unsafe { GetCurrentProcessId() };
        let mut hidden = Vec::new();
        for hwnd in top_level_windows() {
            let mut pid = 0u32;
            unsafe { GetWindowThreadProcessId(hwnd, Some(&mut pid)) };
            if pid != own || !unsafe { IsWindowVisible(hwnd) }.as_bool() {
                continue;
            }
            let mut affinity = 0u32;
            let current = unsafe { GetWindowDisplayAffinity(hwnd, &mut affinity) };
            // Overlay & Co. sind schon ausgeblendet – nicht anfassen.
            if current.is_err() || affinity != WDA_NONE.0 {
                continue;
            }
            if unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) }.is_ok() {
                hidden.push(hwnd);
            }
        }
        if !hidden.is_empty() {
            // Warten, bis der Desktop Window Manager ohne die Fenster gezeichnet hat.
            let _ = unsafe { DwmFlush() };
        }
        Self(hidden)
    }
}

impl Drop for HiddenOwnWindows {
    fn drop(&mut self) {
        for &hwnd in &self.0 {
            let _ = unsafe { SetWindowDisplayAffinity(hwnd, WDA_NONE) };
        }
    }
}

// --- DPI ------------------------------------------------------------------------

/// Rechnet im aktuellen Thread in physischen Pixeln, solange der Wert lebt.
/// Tauri setzt das zwar fürs ganze Programm; so stimmt es auch in Tests
/// und Hintergrund-Threads.
struct DpiGuard(DPI_AWARENESS_CONTEXT);

impl DpiGuard {
    fn new() -> Self {
        Self(unsafe { SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) })
    }
}

impl Drop for DpiGuard {
    fn drop(&mut self) {
        if !self.0.0.is_null() {
            unsafe { SetThreadDpiAwarenessContext(self.0) };
        }
    }
}

// --- GDI -------------------------------------------------------------------------

/// 32-Bit-Bitmap (BGRA, oben links zuerst) in einem eigenen Gerätekontext.
struct Dib {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    width: u32,
    height: u32,
}

impl Dib {
    fn new(width: u32, height: u32) -> Result<Self> {
        if width == 0 || height == 0 {
            return Err(CaptureError::Backend("leerer Bereich".into()));
        }
        let dc = unsafe { CreateCompatibleDC(None) };
        if dc.is_invalid() {
            return Err(CaptureError::Backend("CreateCompatibleDC".into()));
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                // Negativ: Zeilen von oben nach unten.
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                ..Default::default()
            },
            ..Default::default()
        };
        let mut bits: *mut c_void = std::ptr::null_mut();
        let bitmap = match unsafe {
            CreateDIBSection(Some(dc), &info, DIB_RGB_COLORS, &mut bits, None, 0)
        } {
            Ok(b) if !bits.is_null() => b,
            _ => {
                let _ = unsafe { DeleteDC(dc) };
                return Err(CaptureError::Backend(format!(
                    "Bitmap {width}×{height} konnte nicht angelegt werden"
                )));
            }
        };
        let old = unsafe { SelectObject(dc, bitmap.into()) };
        Ok(Self {
            dc,
            bitmap,
            old,
            bits: bits as *mut u8,
            width,
            height,
        })
    }

    fn bytes(&self) -> &[u8] {
        let _ = unsafe { GdiFlush() };
        // SAFETY: CreateDIBSection hat width × height × 4 Bytes angelegt.
        unsafe {
            std::slice::from_raw_parts(self.bits, self.width as usize * self.height as usize * 4)
        }
    }

    /// Kopie als RGBA, voll deckend.
    fn to_rgba(&self) -> Result<RgbaImage> {
        let mut data = self.bytes().to_vec();
        bgra_to_rgba_opaque(&mut data);
        RgbaImage::new(self.width, self.height, data)
            .map_err(|e| CaptureError::Backend(e.to_string()))
    }
}

impl Drop for Dib {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.dc, self.old);
            let _ = DeleteObject(self.bitmap.into());
            let _ = DeleteDC(self.dc);
        }
    }
}

/// Bildschirmbereich (physische Pixel im virtuellen Desktop).
fn blit_screen(r: &RECT) -> Result<RgbaImage> {
    let (w, h) = (
        (r.right - r.left).max(0) as u32,
        (r.bottom - r.top).max(0) as u32,
    );
    let dib = Dib::new(w, h)?;
    let screen = unsafe { GetDC(None) };
    if screen.is_invalid() {
        return Err(CaptureError::Backend("Bildschirm nicht lesbar".into()));
    }
    // CAPTUREBLT: auch Fenster mit Transparenz (Menüs, Tooltips).
    let result = unsafe {
        BitBlt(
            dib.dc,
            0,
            0,
            w as i32,
            h as i32,
            Some(screen),
            r.left,
            r.top,
            SRCCOPY | CAPTUREBLT,
        )
    };
    unsafe { ReleaseDC(None, screen) };
    result.map_err(|e| CaptureError::Backend(format!("BitBlt: {}", e.message())))?;
    dib.to_rgba()
}

/// Fensterinhalt über `PrintWindow`, auch wenn das Fenster verdeckt ist.
/// `frame` ist der sichtbare Rahmen (ohne unsichtbare Ränder).
fn print_window(hwnd: HWND, frame: &RECT) -> Result<RgbaImage> {
    let mut wr = RECT::default();
    unsafe { GetWindowRect(hwnd, &mut wr) }.map_err(|e| CaptureError::Backend(e.message()))?;
    let (w, h) = (
        (wr.right - wr.left).max(0) as u32,
        (wr.bottom - wr.top).max(0) as u32,
    );
    let dib = Dib::new(w, h)?;
    if !unsafe { PrintWindow(hwnd, dib.dc, PRINT_WINDOW_FLAGS(PW_RENDERFULLCONTENT)) }.as_bool() {
        return Err(CaptureError::Backend("PrintWindow".into()));
    }
    let full = dib.to_rgba()?;
    if looks_empty(full.data()) {
        return Err(CaptureError::Backend(
            "PrintWindow lieferte ein leeres Bild".into(),
        ));
    }
    let x = (frame.left - wr.left).clamp(0, w as i32) as u32;
    let y = (frame.top - wr.top).clamp(0, h as i32) as u32;
    let fw = ((frame.right - frame.left).max(1) as u32).min(w - x);
    let fh = ((frame.bottom - frame.top).max(1) as u32).min(h - y);
    full.crop(x, y, fw, fh)
        .ok_or_else(|| CaptureError::Backend("Fensterrahmen ausserhalb".into()))
}

// --- Pixel ------------------------------------------------------------------------

/// BGRA (GDI) → RGBA, Alpha auf 255 (GDI lässt Alpha undefiniert).
fn bgra_to_rgba_opaque(data: &mut [u8]) {
    for px in data.as_chunks_mut::<4>().0 {
        let v = u32::from_le_bytes(*px);
        let swapped = (v & 0x0000_FF00) | ((v >> 16) & 0xFF) | ((v & 0xFF) << 16) | 0xFF00_0000;
        *px = swapped.to_le_bytes();
    }
}

/// Komplett schwarz? (Manche Programme zeichnen sich nicht für PrintWindow.)
fn looks_empty(rgba: &[u8]) -> bool {
    rgba.as_chunks::<4>()
        .0
        .iter()
        .step_by(97)
        .all(|px| px[0] == 0 && px[1] == 0 && px[2] == 0)
}

/// Macht die vier Ecken mit Radius `radius` (Pixel) weich durchsichtig – so
/// wie Windows 11 die Fenster zeichnet.
fn round_corners(image: &mut RgbaImage, radius: f32) {
    let (w, h) = (image.width() as usize, image.height() as usize);
    let r = radius.min(w as f32 / 2.0).min(h as f32 / 2.0);
    let n = r.ceil() as usize;
    if n == 0 {
        return;
    }
    let mut data = std::mem::replace(image, RgbaImage::filled(1, 1, [0; 4])).into_data();
    for dy in 0..n {
        for dx in 0..n {
            // Abstand der Pixelmitte vom Kreismittelpunkt der Ecke.
            let fx = r - (dx as f32 + 0.5);
            let fy = r - (dy as f32 + 0.5);
            if fx <= 0.0 || fy <= 0.0 {
                continue;
            }
            let coverage = (r - (fx * fx + fy * fy).sqrt() + 0.5).clamp(0.0, 1.0);
            if coverage >= 1.0 {
                continue;
            }
            for (x, y) in [
                (dx, dy),
                (w - 1 - dx, dy),
                (dx, h - 1 - dy),
                (w - 1 - dx, h - 1 - dy),
            ] {
                let a = &mut data[(y * w + x) * 4 + 3];
                *a = (*a as f32 * coverage).round() as u8;
            }
        }
    }
    if let Ok(img) = RgbaImage::new(w as u32, h as u32, data) {
        *image = img;
    }
}

fn rect_to_bounds(r: &RECT) -> Rect {
    Rect::new(
        r.left as f32,
        r.top as f32,
        (r.right - r.left) as f32,
        (r.bottom - r.top) as f32,
    )
}

fn wide_to_string(s: &[u16]) -> String {
    let n = s.iter().position(|&c| c == 0).unwrap_or(s.len());
    String::from_utf16_lossy(&s[..n])
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bgra_wird_rgba_und_deckend() {
        let mut px = [1, 2, 3, 0, 10, 20, 30, 7];
        bgra_to_rgba_opaque(&mut px);
        assert_eq!(px, [3, 2, 1, 255, 30, 20, 10, 255]);
    }

    #[test]
    fn ecken_werden_durchsichtig_mitte_nicht() {
        let mut img = RgbaImage::filled(40, 30, [200, 200, 200, 255]);
        round_corners(&mut img, 8.0);
        assert_eq!(img.pixel(0, 0).map(|p| p[3]), Some(0));
        assert_eq!(img.pixel(39, 29).map(|p| p[3]), Some(0));
        assert_eq!(img.pixel(20, 15).map(|p| p[3]), Some(255));
        assert_eq!(img.pixel(20, 0).map(|p| p[3]), Some(255));
    }

    #[test]
    fn fenster_id_hin_und_zurueck() {
        let hwnd = HWND(0x0012_34AB_usize as *mut c_void);
        assert_eq!(hwnd_from_id(hwnd_to_id(hwnd)), hwnd);
    }

    #[test]
    fn schwarzes_bild_gilt_als_leer() {
        assert!(looks_empty(&[0, 0, 0, 255, 0, 0, 0, 255]));
        assert!(!looks_empty(&[0, 0, 9, 255]));
    }

    /// Braucht einen echten Desktop – in der CI vorhanden, aber ohne Gewähr.
    #[test]
    #[ignore]
    fn monitor_aufnehmen() {
        let c = WindowsCapturer::new();
        let d = c.displays().unwrap();
        assert!(!d.is_empty());
        let f = c.capture_display(d[0].id).unwrap();
        assert_eq!(f.image.width() as f32, d[0].bounds.width);
    }
}
