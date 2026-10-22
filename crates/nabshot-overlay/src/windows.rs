//! Windows-Overlay: pro Monitor ein randloses Popup-Fenster über allem
//! (`WS_EX_TOPMOST`), das nie aktiviert wird (`WS_EX_NOACTIVATE`) – der Fokus
//! der vorherigen App bleibt erhalten. Esc kommt über ein kurzzeitig
//! registriertes Tastenkürzel an, weil ein nicht aktives Fenster keine
//! Tastendrücke bekommt.
//!
//! Gezeichnet wird mit GDI in einen Hintergrundpuffer pro Monitor. Das
//! eingefrorene Bild liegt einmal hell und einmal abgedunkelt vor; bei einer
//! Mausbewegung werden nur die Rechtecke neu zusammengesetzt und auf den
//! Bildschirm kopiert, die sich geändert haben (Fadenkreuz-Streifen, Lupe,
//! Rand der Auswahl). Kein Kopieren des ganzen Monitors pro Mausbewegung
//! (ADR 0003, Nachtrag Windows).
//!
//! Alle Fenster setzen `WDA_EXCLUDEFROMCAPTURE` und sind damit in keiner
//! Aufnahme zu sehen. Koordinaten sind physische Pixel des virtuellen
//! Desktops (wie in `nabshot-capture`).

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::mem::size_of;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use nabshot_core::RgbaImage;
use windows::Win32::Foundation::{
    COLORREF, HINSTANCE, HWND, LPARAM, LRESULT, POINT, RECT, SIZE, WPARAM,
};
use windows::Win32::Graphics::Dwm::{
    DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND, DwmSetWindowAttribute,
};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, AlphaBlend, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    BeginPaint, BitBlt, CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, COLORONCOLOR, CreateCompatibleDC,
    CreateDIBSection, CreateEllipticRgn, CreateFontW, CreatePen, CreateRoundRectRgn,
    CreateSolidBrush, DEFAULT_CHARSET, DIB_RGB_COLORS, DT_CALCRECT, DT_CENTER, DT_END_ELLIPSIS,
    DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, DeleteDC, DeleteObject, DrawTextW, Ellipse,
    EndPaint, EnumDisplayMonitors, ExtSelectClipRgn, FW_NORMAL, FW_SEMIBOLD, FillRect, GdiFlush,
    GetDC, GetMonitorInfoW, GetStockObject, HALFTONE, HBITMAP, HBRUSH, HDC, HFONT, HGDIOBJ,
    HMONITOR, HRGN, IntersectClipRect, InvalidateRect, LineTo, MONITOR_DEFAULTTONEAREST,
    MONITOR_DEFAULTTOPRIMARY, MONITORINFO, MonitorFromPoint, MoveToEx, NULL_BRUSH, NULL_PEN,
    OUT_DEFAULT_PRECIS, PAINTSTRUCT, PATCOPY, PS_SOLID, PatBlt, RGN_AND, ReleaseDC, RestoreDC,
    RoundRect, SRCCOPY, SaveDC, SelectObject, SetBkMode, SetBrushOrgEx, SetStretchBltMode,
    SetTextColor, StretchBlt, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Ole::{
    DROPEFFECT_COPY, DROPEFFECT_NONE, OleInitialize, OleUninitialize,
};
use windows::Win32::System::SystemServices::MK_LBUTTON;
use windows::Win32::UI::HiDpi::{
    DPI_AWARENESS_CONTEXT, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor,
    MDT_EFFECTIVE_DPI, SetThreadDpiAwarenessContext,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    MOD_NOREPEAT, RegisterHotKey, ReleaseCapture, SetCapture, TME_LEAVE, TRACKMOUSEEVENT,
    TrackMouseEvent, UnregisterHotKey, VK_ESCAPE,
};
use windows::Win32::UI::Shell::{
    BHID_DataObject, IShellItem, SHCreateItemFromParsingName, SHDoDragDrop,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CS_HREDRAW, CS_VREDRAW, CreateWindowExW, DefWindowProcW, DestroyWindow, GetSystemMetrics,
    HTCLIENT, HWND_TOPMOST, IDC_ARROW, IDC_CROSS, KillTimer, LWA_ALPHA, LWA_COLORKEY, LoadCursorW,
    MA_NOACTIVATE, RegisterClassExW, SM_CXDRAG, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
    SWP_SHOWWINDOW, SetCursor, SetLayeredWindowAttributes, SetTimer, SetWindowDisplayAffinity,
    SetWindowPos, ShowWindow, ULW_ALPHA, UPDATELAYEREDWINDOWINFO, UpdateLayeredWindowIndirect,
    WDA_EXCLUDEFROMCAPTURE, WINDOW_EX_STYLE, WM_DESTROY, WM_ERASEBKGND, WM_HOTKEY, WM_KEYDOWN,
    WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_MOUSEMOVE, WM_PAINT, WM_RBUTTONDOWN,
    WM_SETCURSOR, WM_TIMER, WNDCLASSEXW, WNDPROC, WS_EX_LAYERED, WS_EX_NOACTIVATE,
    WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};
use windows::core::{BOOL, PCWSTR, w};

use crate::{
    DoneCallback, OverlayError, OverlayMode, OverlayResult, PreviewAction, PreviewPlacement,
    ScrollAction, Selection, WindowTarget, points_to_pixels, rect_from_points, window_at,
};

/// Kleinere Auswahl (logische Pixel) gilt als Klick ohne Ziehen.
const MIN_DRAG: f64 = 3.0;
/// Lupe: Durchmesser in logischen Pixeln, Vergrösserung pro Bildpixel.
const MAGNIFIER_SIZE: f64 = 128.0;
const MAGNIFIER_ZOOM: i32 = 8;
/// Abdunklung ausserhalb der Auswahl (wie macOS: 35 % Schwarz).
const DIM_KEEP: u32 = 166; // 255 × 0.65
const HOTKEY_ESCAPE: i32 = 0x4653; // „FS“
/// Farben der halbdurchsichtigen Ebene im Fenster-Modus (BGRA,
/// vormultipliziert): Abdunklung 35 %, blaue Hervorhebung 25 % darüber,
/// Rahmen, Schatten und Linie des Fadenkreuzes.
const LIVE_DIM: [u8; 4] = [0, 0, 0, 89];
const LIVE_HIGHLIGHT: [u8; 4] = [64, 32, 13, 131];
const LIVE_BORDER: [u8; 4] = [242, 122, 48, 242];
const LIVE_SHADOW: [u8; 4] = [0, 0, 0, 163];
const LIVE_GUIDE: [u8; 4] = [230, 230, 230, 230];
/// Steht in `Win32::UI::Controls`; dafür lohnt das ganze Modul nicht.
const WM_MOUSELEAVE: u32 = 0x02A3;

// --- Overlay ----------------------------------------------------------------------

pub(crate) struct Handle {
    session: Weak<RefCell<Session>>,
}

impl Handle {
    pub fn set_frozen(&self, display_id: u32, image: Arc<RgbaImage>) {
        let Some(session) = self.session.upgrade() else {
            return;
        };
        let Ok(mut s) = session.try_borrow_mut() else {
            return;
        };
        if let Some(st) = s.screens.iter_mut().find(|st| st.display_id == display_id) {
            st.set_frozen(&image);
        }
    }

    pub fn cancel(&self) {
        if let Some(session) = self.session.upgrade() {
            with_session(&session, |s| s.finish(OverlayResult::Cancelled));
        }
    }

    pub fn is_open(&self) -> bool {
        self.session
            .upgrade()
            .is_some_and(|s| s.try_borrow().map(|s| s.on_done.is_some()).unwrap_or(true))
    }
}

thread_local! {
    /// Das offene Overlay – es gibt höchstens eines.
    static SESSION: RefCell<Option<Rc<RefCell<Session>>>> = const { RefCell::new(None) };
}

pub(crate) fn open(
    mode: OverlayMode,
    windows: Vec<WindowTarget>,
    on_done: DoneCallback,
) -> Result<Handle, OverlayError> {
    let _dpi = DpiGuard::new();
    // Ein altes, nie abgeschlossenes Overlay zuerst schliessen.
    if let Some(old) = SESSION.with(|s| s.borrow_mut().take()) {
        with_session(&old, |s| s.finish(OverlayResult::Cancelled));
    }
    register_class(OVERLAY_CLASS, Some(overlay_proc))?;

    let mut screens = Vec::new();
    for m in monitors() {
        match Screen::new(&m, mode) {
            Ok(st) => screens.push(st),
            Err(e) => {
                log_error("Overlay-Fenster", &e);
            }
        }
    }
    if screens.is_empty() {
        return Err(OverlayError::NoScreens);
    }
    let hotkey = screens[0].hwnd;
    // Esc systemweit abfangen, solange das Overlay offen ist.
    let hotkey_ok = unsafe {
        RegisterHotKey(
            Some(hotkey),
            HOTKEY_ESCAPE,
            MOD_NOREPEAT,
            VK_ESCAPE.0 as u32,
        )
    }
    .is_ok();
    let session = Rc::new(RefCell::new(Session {
        mode,
        windows,
        screens,
        on_done: Some(on_done),
        drag: None,
        hotkey: hotkey_ok.then_some(hotkey),
    }));
    SESSION.with(|s| *s.borrow_mut() = Some(session.clone()));

    // Alles zeigen, dann den Zeiger dort einzeichnen, wo er gerade ist.
    let mut cursor = POINT::default();
    let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut cursor) };
    {
        let mut s = session.borrow_mut();
        for st in &s.screens {
            st.show();
        }
        let mode = s.mode;
        let windows = std::mem::take(&mut s.windows);
        for st in s.screens.iter_mut() {
            let local = (cursor.x - st.origin.0, cursor.y - st.origin.1);
            if st.contains(local) {
                st.pointer_moved(mode, &windows, local);
            }
        }
        s.windows = windows;
    }
    if let Ok(c) = unsafe { LoadCursorW(None, IDC_CROSS) } {
        unsafe { SetCursor(Some(c)) };
    }
    Ok(Handle {
        session: Rc::downgrade(&session),
    })
}

struct Session {
    mode: OverlayMode,
    windows: Vec<WindowTarget>,
    screens: Vec<Screen>,
    on_done: Option<DoneCallback>,
    drag: Option<Drag>,
    /// Fenster, auf dem Esc registriert ist.
    hotkey: Option<HWND>,
}

struct Drag {
    screen: usize,
    start: (i32, i32),
}

type Finished = (DoneCallback, OverlayResult, Vec<Screen>);

impl Session {
    /// Beendet das Overlay. Rückruf und Fenster werden erst nach dem Borrow
    /// aufgerufen bzw. zerstört.
    fn finish(&mut self, result: OverlayResult) -> Option<Finished> {
        let on_done = self.on_done.take()?;
        let screens = std::mem::take(&mut self.screens);
        for st in &screens {
            unsafe {
                let _ = ShowWindow(st.hwnd, SW_HIDE);
            }
        }
        if let Some(hwnd) = self.hotkey.take() {
            let _ = unsafe { UnregisterHotKey(Some(hwnd), HOTKEY_ESCAPE) };
        }
        if self.drag.take().is_some() {
            let _ = unsafe { ReleaseCapture() };
        }
        Some((on_done, result, screens))
    }

    fn screen_index(&self, hwnd: HWND) -> Option<usize> {
        self.screens.iter().position(|s| s.hwnd == hwnd)
    }
}

/// Ruft `f` mit der Sitzung auf und danach – ausserhalb des Borrows – einen
/// eventuell fälligen Abschluss.
fn with_session(session: &Rc<RefCell<Session>>, f: impl FnOnce(&mut Session) -> Option<Finished>) {
    let done = match session.try_borrow_mut() {
        Ok(mut s) => f(&mut s),
        Err(_) => return,
    };
    if let Some((on_done, result, screens)) = done {
        SESSION.with(|s| {
            let mut slot = s.borrow_mut();
            if slot.as_ref().is_some_and(|cur| Rc::ptr_eq(cur, session)) {
                *slot = None;
            }
        });
        drop(screens);
        on_done(result);
    }
}

fn current_session() -> Option<Rc<RefCell<Session>>> {
    SESSION.with(|s| s.borrow().clone())
}

/// Was gerade auf einem Monitor zu sehen ist.
#[derive(Debug, Clone, Copy, Default, PartialEq)]
struct View {
    pointer: Option<(i32, i32)>,
    selection: Option<RECT>,
    highlight: Option<RECT>,
}

struct Screen {
    display_id: u32,
    hwnd: HWND,
    /// Linke obere Ecke im virtuellen Desktop (physische Pixel).
    origin: (i32, i32),
    size: (i32, i32),
    scale: f64,
    /// Fenster-Modus: kein eingefrorenes Bild, sondern eine halbdurchsichtige
    /// Ebene über dem laufenden Bildschirm.
    live: bool,
    back: Dib,
    bright: Option<Dib>,
    dim: Option<Dib>,
    view: View,
    /// Ist TrackMouseEvent für WM_MOUSELEAVE angemeldet?
    tracking: bool,
    gdi: Gdi,
}

/// Pinsel, Stifte und Schriften eines Monitors.
struct Gdi {
    white: HBRUSH,
    blue: HBRUSH,
    red: HBRUSH,
    black: HBRUSH,
    white_pen: HGDIOBJ,
    font: HFONT,
    /// 1 × 1 Pixel zum Abdunkeln bzw. Einfärben mit AlphaBlend.
    black_px: Dib,
    blue_px: Dib,
}

impl Gdi {
    fn new(scale: f64) -> Result<Self, String> {
        let black_px = Dib::solid(0, 0, 0)?;
        let blue_px = Dib::solid(51, 128, 255)?;
        unsafe {
            Ok(Self {
                white: CreateSolidBrush(rgb(255, 255, 255)),
                blue: CreateSolidBrush(rgb(51, 128, 255)),
                red: CreateSolidBrush(rgb(255, 64, 51)),
                black: CreateSolidBrush(rgb(0, 0, 0)),
                white_pen: CreatePen(PS_SOLID, px(2.0, scale), rgb(255, 255, 255)).into(),
                font: font(12.0, scale, false),
                black_px,
                blue_px,
            })
        }
    }
}

impl Drop for Gdi {
    fn drop(&mut self) {
        unsafe {
            for b in [self.white, self.blue, self.red, self.black] {
                let _ = DeleteObject(b.into());
            }
            let _ = DeleteObject(self.white_pen);
            let _ = DeleteObject(self.font.into());
        }
    }
}

impl Screen {
    fn new(m: &MonitorInfo, mode: OverlayMode) -> Result<Self, String> {
        let (w, h) = (m.rect.right - m.rect.left, m.rect.bottom - m.rect.top);
        let back = Dib::new(w, h)?;
        let gdi = Gdi::new(m.scale)?;
        let hwnd = create_popup(
            OVERLAY_CLASS,
            WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED,
            m.rect,
        )?;
        let live = mode == OverlayMode::Window;
        if !live {
            // Erst fast unsichtbar (der laufende Bildschirm bleibt zu sehen,
            // Klicks kommen trotzdem an), deckend sobald das eingefrorene Bild
            // da ist.
            let _ = unsafe { SetLayeredWindowAttributes(hwnd, COLORREF(0), 1, LWA_ALPHA) };
        }
        let st = Self {
            display_id: m.id,
            hwnd,
            origin: (m.rect.left, m.rect.top),
            size: (w, h),
            scale: m.scale,
            live,
            back,
            bright: None,
            dim: None,
            view: View::default(),
            tracking: false,
            gdi,
        };
        st.redraw(full_rect(w, h));
        Ok(st)
    }

    /// Setzt `r` neu zusammen und bringt es auf den Bildschirm.
    fn redraw(&self, r: RECT) {
        if self.live {
            self.compose_live(r);
            self.present_live(r);
        } else {
            self.compose(r);
            self.present(r);
        }
    }

    /// Fenster-Modus: halbdurchsichtige Ebene über dem laufenden Bildschirm
    /// (vormultipliziertes BGRA), nur einfarbige Flächen.
    fn compose_live(&self, r: RECT) {
        let (w, h) = self.size;
        let buf = self.back.bytes_mut();
        let pixels = buf.as_chunks_mut::<4>().0;
        let mut fill = |area: RECT, color: [u8; 4]| {
            let Some(a) = intersect(area, r) else { return };
            for y in a.top..a.bottom {
                let row = y as usize * w as usize;
                pixels[row + a.left as usize..row + a.right as usize].fill(color);
            }
        };
        fill(r, LIVE_DIM);
        if let Some(hl) = self.view.highlight {
            let inner = inflate(hl, -1);
            fill(inner, LIVE_HIGHLIGHT);
            let t = px(2.0, self.scale).max(1);
            for edge in [
                RECT {
                    bottom: inner.top + t,
                    ..inner
                },
                RECT {
                    top: inner.bottom - t,
                    ..inner
                },
                RECT {
                    right: inner.left + t,
                    ..inner
                },
                RECT {
                    left: inner.right - t,
                    ..inner
                },
            ] {
                fill(edge, LIVE_BORDER);
            }
        }
        if let Some((x, y)) = self.view.pointer {
            for s in self.guide_rects((x, y)) {
                fill(s, LIVE_SHADOW);
            }
            fill(
                RECT {
                    left: 0,
                    top: y,
                    right: w,
                    bottom: y + 1,
                },
                LIVE_GUIDE,
            );
            fill(
                RECT {
                    left: x,
                    top: 0,
                    right: x + 1,
                    bottom: h,
                },
                LIVE_GUIDE,
            );
        }
    }

    /// Überträgt `r` der halbdurchsichtigen Ebene (nur dieser Ausschnitt wird
    /// kopiert).
    fn present_live(&self, r: RECT) {
        let _ = unsafe { GdiFlush() };
        let pos = POINT {
            x: self.origin.0,
            y: self.origin.1,
        };
        let size = SIZE {
            cx: self.size.0,
            cy: self.size.1,
        };
        let src = POINT::default();
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let info = UPDATELAYEREDWINDOWINFO {
            cbSize: size_of::<UPDATELAYEREDWINDOWINFO>() as u32,
            hdcDst: HDC::default(),
            pptDst: &pos,
            psize: &size,
            hdcSrc: self.back.dc,
            pptSrc: &src,
            crKey: COLORREF(0),
            pblend: &blend,
            dwFlags: ULW_ALPHA,
            prcDirty: &r,
        };
        let _ = unsafe { UpdateLayeredWindowIndirect(self.hwnd, &info) };
    }

    fn show(&self) {
        unsafe {
            let _ = ShowWindow(self.hwnd, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(
                self.hwnd,
                Some(HWND_TOPMOST),
                self.origin.0,
                self.origin.1,
                self.size.0,
                self.size.1,
                SWP_NOACTIVATE | SWP_SHOWWINDOW,
            );
        }
    }

    fn contains(&self, p: (i32, i32)) -> bool {
        p.0 >= 0 && p.1 >= 0 && p.0 < self.size.0 && p.1 < self.size.1
    }

    fn clamp(&self, p: (i32, i32)) -> (i32, i32) {
        (p.0.clamp(0, self.size.0), p.1.clamp(0, self.size.1))
    }

    fn set_frozen(&mut self, image: &RgbaImage) {
        if self.live {
            return;
        }
        let (w, h) = self.size;
        let (Ok(bright), Ok(dim)) = (Dib::new(w, h), Dib::new(w, h)) else {
            return;
        };
        fill_frozen(&bright, &dim, image);
        self.bright = Some(bright);
        self.dim = Some(dim);
        self.compose(full_rect(w, h));
        self.present(full_rect(w, h));
        let _ = unsafe { SetLayeredWindowAttributes(self.hwnd, COLORREF(0), 255, LWA_ALPHA) };
    }

    fn pointer_moved(&mut self, mode: OverlayMode, windows: &[WindowTarget], p: (i32, i32)) {
        let mut view = self.view;
        view.pointer = Some(p);
        if mode == OverlayMode::Window {
            let global = ((self.origin.0 + p.0) as f64, (self.origin.1 + p.1) as f64);
            view.highlight = window_at(windows, global).map(|w| {
                let (x, y, ww, wh) = w.bounds;
                let x = x.round() as i32 - self.origin.0;
                let y = y.round() as i32 - self.origin.1;
                RECT {
                    left: x,
                    top: y,
                    right: x + ww.round() as i32,
                    bottom: y + wh.round() as i32,
                }
            });
        }
        self.update(view);
    }

    fn pointer_left(&mut self) {
        self.update(View {
            pointer: None,
            highlight: None,
            ..self.view
        });
    }

    /// Wechselt zur neuen Ansicht und zeichnet nur, was sich geändert hat.
    fn update(&mut self, view: View) {
        let old = self.view;
        if old == view {
            return;
        }
        self.view = view;
        let mut dirty = Vec::new();
        if old.pointer != view.pointer {
            for p in [old.pointer, view.pointer].into_iter().flatten() {
                dirty.extend(self.guide_rects(p));
                if let Some(m) = self.magnifier_rect(p) {
                    dirty.push(inflate(m, 2));
                }
            }
        }
        if old.selection != view.selection {
            dirty.extend(changed_area(old.selection, view.selection, 2));
            for sel in [old.selection, view.selection].into_iter().flatten() {
                dirty.push(inflate(self.label_rect(sel).0, 1));
            }
        }
        if old.highlight != view.highlight {
            dirty.extend(changed_area(old.highlight, view.highlight, 3));
        }
        let full = full_rect(self.size.0, self.size.1);
        for r in dirty {
            if let Some(r) = intersect(r, full) {
                self.redraw(r);
            }
        }
    }

    /// Waagrechter und senkrechter Streifen des Fadenkreuzes.
    fn guide_rects(&self, (x, y): (i32, i32)) -> [RECT; 2] {
        let (w, h) = self.size;
        [
            RECT {
                left: 0,
                top: y - 1,
                right: w,
                bottom: y + 2,
            },
            RECT {
                left: x - 1,
                top: 0,
                right: x + 2,
                bottom: h,
            },
        ]
    }

    /// Lupe rechts unterhalb des Zeigers, an Rändern auf der anderen Seite.
    fn magnifier_rect(&self, (x, y): (i32, i32)) -> Option<RECT> {
        self.bright.as_ref()?;
        let size = self.magnifier_size();
        let offset = px(24.0, self.scale);
        let mut mx = x + offset;
        let mut my = y + offset;
        if mx + size > self.size.0 {
            mx = x - offset - size;
        }
        if my + size > self.size.1 {
            my = y - offset - size;
        }
        Some(RECT {
            left: mx,
            top: my,
            right: mx + size,
            bottom: my + size,
        })
    }

    /// Durchmesser der Lupe – ein Vielfaches der Vergrösserung, damit die
    /// Pixel gleich gross sind.
    fn magnifier_size(&self) -> i32 {
        let src = (px(MAGNIFIER_SIZE, self.scale) / MAGNIFIER_ZOOM) & !1;
        src.max(8) * MAGNIFIER_ZOOM
    }

    /// Grössenanzeige unter der Auswahl (sonst darüber) und ihr Text.
    fn label_rect(&self, sel: RECT) -> (RECT, String) {
        let pixel = (self.size.0 as u32, self.size.1 as u32);
        let points = (self.size.0 as f64, self.size.1 as f64);
        let rect = (
            sel.left as f64,
            sel.top as f64,
            (sel.right - sel.left) as f64,
            (sel.bottom - sel.top) as f64,
        );
        let (_, _, pw, ph) = points_to_pixels(rect, points, pixel);
        let text = format!("{pw} × {ph}");
        let lw = px(12.0 + 7.0 * text.chars().count() as f64, self.scale);
        let lh = px(20.0, self.scale);
        let gap = px(6.0, self.scale);
        let mut ly = sel.bottom + gap;
        if ly + lh > self.size.1 {
            ly = (sel.top - lh - gap).max(0);
        }
        let lx = (sel.right - lw).clamp(0, (self.size.0 - lw).max(0));
        (
            RECT {
                left: lx,
                top: ly,
                right: lx + lw,
                bottom: ly + lh,
            },
            text,
        )
    }

    /// Setzt den Ausschnitt `r` im Hintergrundpuffer neu zusammen.
    fn compose(&self, r: RECT) {
        let dc = self.back.dc;
        let g = &self.gdi;
        unsafe {
            let saved = SaveDC(dc);
            IntersectClipRect(dc, r.left, r.top, r.right, r.bottom);

            // 1. Abgedunkeltes Bild (vor dem Einfrieren: schwarz, das Fenster
            //    ist dann ohnehin fast durchsichtig).
            match &self.dim {
                Some(dim) => blit(dc, r, dim.dc, (r.left, r.top)),
                None => {
                    FillRect(dc, &r, g.black);
                }
            }
            // 2. Auswahl hell
            if let (Some(sel), Some(bright)) = (self.view.selection, &self.bright)
                && let Some(i) = intersect(sel, r)
            {
                blit(dc, i, bright.dc, (i.left, i.top));
            }
            // 3. Fenster-Hervorhebung (Fenster-Modus)
            if let Some(hl) = self.view.highlight {
                let inner = inflate(hl, -1);
                if let Some(i) = intersect(inner, r) {
                    blend(dc, i, &g.blue_px, 64);
                }
                frame(dc, inner, px(2.0, self.scale), g.blue);
            }
            // 4. Rahmen der Auswahl
            if let Some(sel) = self.view.selection {
                frame(dc, sel, 1, g.white);
            }
            // 5. Fadenkreuz: dunkler Schatten für hellen Grund, helle Linie
            if let Some((x, y)) = self.view.pointer {
                for s in self.guide_rects((x, y)) {
                    if let Some(i) = intersect(s, r) {
                        blend(dc, i, &g.black_px, 115);
                    }
                }
                let (w, h) = self.size;
                fill(
                    dc,
                    RECT {
                        left: 0,
                        top: y,
                        right: w,
                        bottom: y + 1,
                    },
                    g.white,
                );
                fill(
                    dc,
                    RECT {
                        left: x,
                        top: 0,
                        right: x + 1,
                        bottom: h,
                    },
                    g.white,
                );
            }
            // 6. Grössenanzeige
            if let Some(sel) = self.view.selection {
                let (lr, text) = self.label_rect(sel);
                if intersect(lr, r).is_some() {
                    blend(dc, lr, &g.black_px, 191);
                    draw_text(dc, &text, lr, g.font, rgb(255, 255, 255), DT_CENTER);
                }
            }
            // 7. Lupe
            if let (Some(p), Some(bright)) = (self.view.pointer, &self.bright)
                && let Some(m) = self.magnifier_rect(p)
                && intersect(inflate(m, 2), r).is_some()
            {
                self.draw_magnifier(dc, m, p, bright);
            }
            let _ = RestoreDC(dc, saved);
        }
    }

    unsafe fn draw_magnifier(&self, dc: HDC, m: RECT, (x, y): (i32, i32), bright: &Dib) {
        let g = &self.gdi;
        let size = m.right - m.left;
        let src = size / MAGNIFIER_ZOOM;
        unsafe {
            let saved = SaveDC(dc);
            let rgn: HRGN = CreateEllipticRgn(m.left, m.top, m.right + 1, m.bottom + 1);
            ExtSelectClipRgn(dc, Some(rgn), RGN_AND);
            FillRect(dc, &m, g.black);
            // Quellbereich um den Zeiger, am Bildrand beschnitten.
            let sx0 = x - src / 2;
            let sy0 = y - src / 2;
            let cx0 = sx0.max(0);
            let cy0 = sy0.max(0);
            let cx1 = (sx0 + src).min(self.size.0);
            let cy1 = (sy0 + src).min(self.size.1);
            if cx1 > cx0 && cy1 > cy0 {
                SetStretchBltMode(dc, COLORONCOLOR);
                let _ = StretchBlt(
                    dc,
                    m.left + (cx0 - sx0) * MAGNIFIER_ZOOM,
                    m.top + (cy0 - sy0) * MAGNIFIER_ZOOM,
                    (cx1 - cx0) * MAGNIFIER_ZOOM,
                    (cy1 - cy0) * MAGNIFIER_ZOOM,
                    Some(bright.dc),
                    cx0,
                    cy0,
                    cx1 - cx0,
                    cy1 - cy0,
                    SRCCOPY,
                );
            }
            // Das Pixel unter dem Zeiger markieren.
            let c = m.left + (src / 2) * MAGNIFIER_ZOOM;
            let cy = m.top + (src / 2) * MAGNIFIER_ZOOM;
            frame(
                dc,
                RECT {
                    left: c,
                    top: cy,
                    right: c + MAGNIFIER_ZOOM,
                    bottom: cy + MAGNIFIER_ZOOM,
                },
                1,
                g.red,
            );
            let _ = RestoreDC(dc, saved);
            let _ = DeleteObject(rgn.into());
            // Weisser Ring
            let old_pen = SelectObject(dc, g.white_pen);
            let old_brush = SelectObject(dc, GetStockObject(NULL_BRUSH));
            let _ = Ellipse(dc, m.left, m.top, m.right, m.bottom);
            SelectObject(dc, old_brush);
            SelectObject(dc, old_pen);
        }
    }

    /// Kopiert `r` aus dem Hintergrundpuffer auf den Bildschirm.
    fn present(&self, r: RECT) {
        unsafe {
            let dc = GetDC(Some(self.hwnd));
            if !dc.is_invalid() {
                blit(dc, r, self.back.dc, (r.left, r.top));
                ReleaseDC(Some(self.hwnd), dc);
            }
        }
    }

    fn paint(&self) {
        unsafe {
            let mut ps = PAINTSTRUCT::default();
            let dc = BeginPaint(self.hwnd, &mut ps);
            if !dc.is_invalid() {
                let r = ps.rcPaint;
                blit(dc, r, self.back.dc, (r.left, r.top));
            }
            let _ = EndPaint(self.hwnd, &ps);
        }
    }
}

impl Drop for Screen {
    fn drop(&mut self) {
        let _ = unsafe { DestroyWindow(self.hwnd) };
    }
}

/// Hell und abgedunkelt (BGRA) aus dem RGBA-Bild, in einem Durchgang.
/// Passt die Grösse nicht (sollte nicht vorkommen), wird nächster Nachbar
/// genommen.
fn fill_frozen(bright: &Dib, dim: &Dib, image: &RgbaImage) {
    let (w, h) = (bright.width as usize, bright.height as usize);
    let (iw, ih) = (image.width() as usize, image.height() as usize);
    if iw == 0 || ih == 0 {
        return;
    }
    let src = image.data();
    let b = bright.bytes_mut();
    let d = dim.bytes_mut();
    let dim_of = |c: u8| (c as u32 * DIM_KEEP / 255) as u8;
    if iw == w && ih == h {
        let pixels = src.as_chunks::<4>().0.iter();
        for ((s, b), d) in pixels
            .zip(b.as_chunks_mut::<4>().0)
            .zip(d.as_chunks_mut::<4>().0)
        {
            *b = [s[2], s[1], s[0], 255];
            *d = [dim_of(s[2]), dim_of(s[1]), dim_of(s[0]), 255];
        }
        return;
    }
    for y in 0..h {
        let sy = if ih == h { y } else { y * ih / h };
        for x in 0..w {
            let sx = if iw == w { x } else { x * iw / w };
            let s = (sy * iw + sx) * 4;
            let o = (y * w + x) * 4;
            let (r, g, bl) = (src[s] as u32, src[s + 1] as u32, src[s + 2] as u32);
            b[o..o + 4].copy_from_slice(&[bl as u8, g as u8, r as u8, 255]);
            d[o..o + 4].copy_from_slice(&[
                (bl * DIM_KEEP / 255) as u8,
                (g * DIM_KEEP / 255) as u8,
                (r * DIM_KEEP / 255) as u8,
                255,
            ]);
        }
    }
}

const OVERLAY_CLASS: PCWSTR = w!("NabshotOverlay");

unsafe extern "system" fn overlay_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => return LRESULT(1),
        WM_SETCURSOR if (lp.0 & 0xFFFF) as u32 == HTCLIENT => {
            if let Ok(c) = unsafe { LoadCursorW(None, IDC_CROSS) } {
                unsafe { SetCursor(Some(c)) };
            }
            return LRESULT(1);
        }
        _ => {}
    }
    if let Some(session) = current_session()
        && let Some(handled) = overlay_message(&session, hwnd, msg, wp, lp)
    {
        return handled;
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

/// Behandelt eine Nachricht des Overlays; `None` = Standardverhalten.
fn overlay_message(
    session: &Rc<RefCell<Session>>,
    hwnd: HWND,
    msg: u32,
    wp: WPARAM,
    lp: LPARAM,
) -> Option<LRESULT> {
    match msg {
        WM_PAINT => {
            let s = session.try_borrow().ok()?;
            let st = &s.screens[s.screen_index(hwnd)?];
            st.paint();
            Some(LRESULT(0))
        }
        WM_MOUSEMOVE => {
            let mut s = session.try_borrow_mut().ok()?;
            let index = s.screen_index(hwnd)?;
            let p = mouse_point(lp);
            let (mode, windows) = (s.mode, std::mem::take(&mut s.windows));
            let drag = s.drag.as_ref().map(|d| (d.screen, d.start));
            let st = &mut s.screens[index];
            if !st.tracking {
                track_leave(hwnd);
                st.tracking = true;
            }
            match drag {
                // Ziehen bleibt auf dem Monitor, auf dem es begonnen hat.
                Some((screen, start)) if screen == index && mode == OverlayMode::Region => {
                    let p = st.clamp(p);
                    let (x, y, w, h) = rect_from_points(
                        (start.0 as f64, start.1 as f64),
                        (p.0 as f64, p.1 as f64),
                    );
                    let sel = RECT {
                        left: x as i32,
                        top: y as i32,
                        right: (x + w) as i32,
                        bottom: (y + h) as i32,
                    };
                    st.update(View {
                        pointer: Some(p),
                        selection: Some(sel),
                        highlight: None,
                    });
                }
                Some(_) if mode == OverlayMode::Region => {}
                _ => st.pointer_moved(mode, &windows, p),
            }
            s.windows = windows;
            Some(LRESULT(0))
        }
        WM_MOUSELEAVE => {
            let mut s = session.try_borrow_mut().ok()?;
            let index = s.screen_index(hwnd)?;
            let dragging_here = s.drag.as_ref().is_some_and(|d| d.screen == index);
            let st = &mut s.screens[index];
            st.tracking = false;
            if !dragging_here {
                st.pointer_left();
            }
            Some(LRESULT(0))
        }
        WM_LBUTTONDOWN => {
            let mut s = session.try_borrow_mut().ok()?;
            let index = s.screen_index(hwnd)?;
            s.drag = Some(Drag {
                screen: index,
                start: mouse_point(lp),
            });
            unsafe { SetCapture(hwnd) };
            Some(LRESULT(0))
        }
        WM_LBUTTONUP => {
            let p = mouse_point(lp);
            with_session(session, |s| {
                let drag = s.drag.take()?;
                let _ = unsafe { ReleaseCapture() };
                if s.mode == OverlayMode::Window {
                    let origin = s.screens.get(drag.screen)?.origin;
                    let global = ((origin.0 + p.0) as f64, (origin.1 + p.1) as f64);
                    let id = window_at(&s.windows, global)?.id;
                    return s.finish(OverlayResult::Window(id));
                }
                let st = s.screens.get_mut(drag.screen)?;
                let p = st.clamp(p);
                let rect = rect_from_points(
                    (drag.start.0 as f64, drag.start.1 as f64),
                    (p.0 as f64, p.1 as f64),
                );
                let min = MIN_DRAG * st.scale;
                if rect.2 < min || rect.3 < min {
                    // Nur geklickt – Overlay bleibt offen.
                    st.update(View {
                        selection: None,
                        ..st.view
                    });
                    return None;
                }
                let size = (st.size.0 as f64, st.size.1 as f64);
                let pixels = (st.size.0 as u32, st.size.1 as u32);
                let (x, y, width, height) = points_to_pixels(rect, size, pixels);
                let selection = Selection {
                    display_id: st.display_id,
                    x,
                    y,
                    width,
                    height,
                };
                s.finish(OverlayResult::Selected(selection))
            });
            Some(LRESULT(0))
        }
        WM_RBUTTONDOWN => {
            with_session(session, |s| s.finish(OverlayResult::Cancelled));
            Some(LRESULT(0))
        }
        WM_HOTKEY if wp.0 as i32 == HOTKEY_ESCAPE => {
            with_session(session, |s| s.finish(OverlayResult::Cancelled));
            Some(LRESULT(0))
        }
        WM_KEYDOWN if wp.0 as u16 == VK_ESCAPE.0 => {
            with_session(session, |s| s.finish(OverlayResult::Cancelled));
            Some(LRESULT(0))
        }
        _ => None,
    }
}

// --- Scroll-Leiste ------------------------------------------------------------------

const BAR_WIDTH: f64 = 340.0;
const BAR_HEIGHT: f64 = 44.0;
/// x-Bereiche der Knöpfe in der Leiste (logische Pixel).
const FINISH_X: (f64, f64) = (190.0, 262.0);
const CANCEL_X: (f64, f64) = (268.0, 334.0);
/// Farbe, die im Rahmenfenster durchsichtig ist.
const KEY_COLOR: COLORREF = COLORREF(0x00FF_00FF);

pub(crate) struct ScrollUi {
    border: HWND,
    bar: HWND,
    state: Rc<BarState>,
}

struct BarState {
    on_action: Box<dyn Fn(ScrollAction)>,
    status: RefCell<String>,
    finish: String,
    cancel: String,
    scale: f64,
    font: HFONT,
    closed: Cell<bool>,
}

impl Drop for BarState {
    fn drop(&mut self) {
        let _ = unsafe { DeleteObject(self.font.into()) };
    }
}

thread_local! {
    static BARS: RefCell<Vec<(HWND, Rc<BarState>)>> = const { RefCell::new(Vec::new()) };
    /// Rahmenfenster und ihre Skalierung.
    static BORDERS: RefCell<Vec<(HWND, f64)>> = const { RefCell::new(Vec::new()) };
}

impl ScrollUi {
    pub fn set_status(&self, text: &str) {
        *self.state.status.borrow_mut() = text.to_string();
        let _ = unsafe { InvalidateRect(Some(self.bar), None, false) };
    }

    pub fn close(&self) {
        if self.state.closed.replace(true) {
            return;
        }
        BARS.with(|b| b.borrow_mut().retain(|(h, _)| *h != self.bar));
        BORDERS.with(|b| b.borrow_mut().retain(|(h, _)| *h != self.border));
        unsafe {
            let _ = DestroyWindow(self.border);
            let _ = DestroyWindow(self.bar);
        }
    }
}

impl Drop for ScrollUi {
    fn drop(&mut self) {
        self.close();
    }
}

pub(crate) fn open_scroll_ui(
    display_id: u32,
    rect: (u32, u32, u32, u32),
    labels: (&str, &str, &str),
    on_action: Box<dyn Fn(ScrollAction)>,
) -> Result<ScrollUi, OverlayError> {
    let _dpi = DpiGuard::new();
    let m = monitors()
        .into_iter()
        .find(|m| m.id == display_id)
        .ok_or(OverlayError::NoScreens)?;
    register_class(BORDER_CLASS, Some(border_proc))?;
    register_class(BAR_CLASS, Some(bar_proc))?;
    let s = m.scale;
    let area = RECT {
        left: m.rect.left + rect.0 as i32,
        top: m.rect.top + rect.1 as i32,
        right: m.rect.left + (rect.0 + rect.2) as i32,
        bottom: m.rect.top + (rect.1 + rect.3) as i32,
    };

    // Rahmen: durchlässig für Maus und Mausrad, Inneres durchsichtig.
    let margin = px(3.0, s);
    let border = create_popup(
        BORDER_CLASS,
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TRANSPARENT,
        inflate(area, margin),
    )
    .map_err(|e| log_error("Scroll-Rahmen", &e))?;
    let _ = unsafe { SetLayeredWindowAttributes(border, KEY_COLOR, 255, LWA_COLORKEY) };
    BORDERS.with(|b| b.borrow_mut().push((border, s)));

    // Leiste unter dem Bereich, sonst darüber, sonst innen unten.
    let (bw, bh) = (px(BAR_WIDTH, s), px(BAR_HEIGHT, s));
    let work = m.work;
    let gap = margin + px(10.0, s);
    let bx = (area.left + (area.right - area.left - bw) / 2).clamp(
        work.left + px(4.0, s),
        (work.right - bw - px(4.0, s)).max(work.left),
    );
    let by = if area.bottom + gap + bh <= work.bottom {
        area.bottom + gap
    } else if area.top - gap - bh >= work.top {
        area.top - gap - bh
    } else {
        area.bottom - bh - px(12.0, s)
    };
    let bar = match create_popup(
        BAR_CLASS,
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        RECT {
            left: bx,
            top: by,
            right: bx + bw,
            bottom: by + bh,
        },
    ) {
        Ok(h) => h,
        Err(e) => {
            let _ = unsafe { DestroyWindow(border) };
            return Err(log_error("Scroll-Leiste", &e));
        }
    };
    round_window(bar);
    let state = Rc::new(BarState {
        on_action,
        status: RefCell::new(labels.0.to_string()),
        finish: labels.1.to_string(),
        cancel: labels.2.to_string(),
        scale: s,
        font: font(13.0, s, false),
        closed: Cell::new(false),
    });
    BARS.with(|b| b.borrow_mut().push((bar, state.clone())));
    for h in [border, bar] {
        unsafe {
            let _ = ShowWindow(h, SW_SHOWNOACTIVATE);
            let _ = SetWindowPos(h, Some(HWND_TOPMOST), 0, 0, 0, 0, swp_keep());
        }
    }
    Ok(ScrollUi { border, bar, state })
}

const BORDER_CLASS: PCWSTR = w!("NabshotScrollBorder");
const BAR_CLASS: PCWSTR = w!("NabshotScrollBar");

unsafe extern "system" fn border_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if msg == WM_PAINT {
        let scale = BORDERS.with(|b| b.borrow().iter().find(|(h, _)| *h == hwnd).map(|(_, s)| *s));
        paint_with(hwnd, |dc, size| unsafe {
            let key = CreateSolidBrush(KEY_COLOR);
            let red = CreateSolidBrush(rgb(230, 56, 54));
            FillRect(dc, &full_rect(size.0, size.1), key);
            frame(
                dc,
                full_rect(size.0, size.1),
                px(2.0, scale.unwrap_or(1.0)),
                red,
            );
            let _ = DeleteObject(key.into());
            let _ = DeleteObject(red.into());
        });
        return LRESULT(0);
    }
    if msg == WM_ERASEBKGND {
        return LRESULT(1);
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

fn bar_state(hwnd: HWND) -> Option<Rc<BarState>> {
    BARS.with(|b| {
        b.borrow()
            .iter()
            .find(|(h, _)| *h == hwnd)
            .map(|(_, s)| s.clone())
    })
}

unsafe extern "system" fn bar_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => return LRESULT(1),
        WM_SETCURSOR => {
            set_arrow_cursor();
            return LRESULT(1);
        }
        WM_PAINT => {
            if let Some(st) = bar_state(hwnd) {
                paint_with(hwnd, |dc, size| unsafe { paint_bar(dc, size, &st) });
                return LRESULT(0);
            }
        }
        WM_LBUTTONDOWN => {
            if let Some(st) = bar_state(hwnd) {
                let x = mouse_point(lp).0 as f64 / st.scale;
                let action = if x >= FINISH_X.0 && x <= FINISH_X.1 {
                    Some(ScrollAction::Finish)
                } else if x >= CANCEL_X.0 && x <= CANCEL_X.1 {
                    Some(ScrollAction::Cancel)
                } else {
                    None
                };
                if let Some(a) = action {
                    (st.on_action)(a);
                }
                return LRESULT(0);
            }
        }
        _ => {}
    }
    unsafe { DefWindowProcW(hwnd, msg, wp, lp) }
}

unsafe fn paint_bar(dc: HDC, size: (i32, i32), st: &BarState) {
    let s = st.scale;
    unsafe {
        let bg = CreateSolidBrush(rgb(31, 31, 33));
        let red = CreateSolidBrush(rgb(230, 56, 54));
        FillRect(dc, &full_rect(size.0, size.1), bg);
        let status = st.status.borrow().clone();
        draw_text(
            dc,
            &status,
            RECT {
                left: px(14.0, s),
                top: 0,
                right: px(FINISH_X.0 - 6.0, s),
                bottom: size.1,
            },
            st.font,
            rgb(255, 255, 255),
            DT_LEFT,
        );
        let finish = RECT {
            left: px(FINISH_X.0, s),
            top: px(8.0, s),
            right: px(FINISH_X.1, s),
            bottom: px(36.0, s),
        };
        round_fill(dc, finish, px(6.0, s), red);
        draw_text(
            dc,
            &st.finish,
            finish,
            st.font,
            rgb(255, 255, 255),
            DT_CENTER,
        );
        let cancel = RECT {
            left: px(CANCEL_X.0, s),
            top: 0,
            right: px(CANCEL_X.1, s),
            bottom: size.1,
        };
        draw_text(
            dc,
            &st.cancel,
            cancel,
            st.font,
            rgb(204, 204, 204),
            DT_CENTER,
        );
        let _ = DeleteObject(bg.into());
        let _ = DeleteObject(red.into());
    }
}

// --- Schnellzugriff-Vorschau -----------------------------------------------------------

const PREVIEW_W: f64 = 252.0;
const PREVIEW_IMG_MAX_H: f64 = 170.0;
const PREVIEW_PAD: f64 = 8.0;
const PREVIEW_BAR_H: f64 = 34.0;
/// Ohne Maus darüber verschwindet die Vorschau nach dieser Zeit.
const PREVIEW_MS: u32 = 6000;
const PREVIEW_TIMER: usize = 1;

struct PreviewState {
    hwnd: Cell<HWND>,
    on_action: Box<dyn Fn(PreviewAction)>,
    file: String,
    labels: [String; 4],
    scale: f64,
    /// Bild und Bereich in der Vorschau (logische Pixel, oben links).
    image: Dib,
    image_rect: LRect,
    height: i32,
    font: HFONT,
    hovered: Cell<bool>,
    tracking: Cell<bool>,
    down: Cell<Option<(i32, i32)>>,
    dragging: Cell<bool>,
    closed: Cell<bool>,
}

impl Drop for PreviewState {
    fn drop(&mut self) {
        let _ = unsafe { DeleteObject(self.font.into()) };
    }
}

thread_local! {
    /// Offene Vorschauen (zum Stapeln in der Ecke), älteste zuerst.
    static PREVIEWS: RefCell<Vec<Rc<PreviewState>>> = const { RefCell::new(Vec::new()) };
}

/// Rechteck in logischen Pixeln.
#[derive(Debug, Clone, Copy, PartialEq)]
struct LRect {
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl LRect {
    fn contains(&self, p: (f64, f64)) -> bool {
        p.0 >= self.x && p.1 >= self.y && p.0 < self.x + self.w && p.1 < self.y + self.h
    }

    fn to_px(self, scale: f64) -> RECT {
        RECT {
            left: px(self.x, scale),
            top: px(self.y, scale),
            right: px(self.x + self.w, scale),
            bottom: px(self.y + self.h, scale),
        }
    }
}

/// Knopf ✕ oben rechts auf dem Bild.
fn close_rect(image: LRect) -> LRect {
    LRect {
        x: image.x + image.w - 26.0,
        y: image.y + 4.0,
        w: 22.0,
        h: 22.0,
    }
}

/// Knopf unter dem Punkt (logische Pixel, oben links) – wie unter macOS.
fn preview_hit(image: LRect, p: (f64, f64)) -> Option<PreviewAction> {
    if close_rect(image).contains(p) {
        return Some(PreviewAction::Close);
    }
    if image.contains(p) {
        return Some(PreviewAction::Edit);
    }
    let bar_y = image.y + image.h + 4.0;
    if p.1 < bar_y || p.1 > bar_y + PREVIEW_BAR_H {
        return None;
    }
    let w = (PREVIEW_W - 2.0 * PREVIEW_PAD) / 4.0;
    let i = ((p.0 - PREVIEW_PAD) / w).floor();
    if i < 0.0 {
        return None;
    }
    [
        PreviewAction::Copy,
        PreviewAction::Save,
        PreviewAction::Edit,
        PreviewAction::Text,
    ]
    .get(i as usize)
    .copied()
}

pub(crate) fn show_preview(
    image: Arc<RgbaImage>,
    file: &str,
    placement: PreviewPlacement,
    labels: [&str; 4],
    on_action: Box<dyn Fn(PreviewAction)>,
) -> Result<(), OverlayError> {
    let _dpi = DpiGuard::new();
    register_class(PREVIEW_CLASS, Some(preview_proc))?;
    let (anchor, monitor) = match placement {
        PreviewPlacement::At(x, y) => {
            let p = POINT {
                x: x.round() as i32,
                y: y.round() as i32,
            };
            (Some(p), unsafe {
                MonitorFromPoint(p, MONITOR_DEFAULTTONEAREST)
            })
        }
        PreviewPlacement::Corner => (None, unsafe {
            MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY)
        }),
    };
    let m = monitor_info(monitor).ok_or(OverlayError::NoScreens)?;
    let s = m.scale;

    let (iw, ih) = (image.width() as f64, image.height() as f64);
    let max_w = PREVIEW_W - 2.0 * PREVIEW_PAD;
    // Das Vorschaubild ist in Bildpixeln; anzeigen in logischer Grösse.
    let fit = (max_w / iw.max(1.0))
        .min(PREVIEW_IMG_MAX_H / ih.max(1.0))
        .min(1.0);
    let (dw, dh) = ((iw * fit).max(40.0), (ih * fit).max(30.0));
    let height = PREVIEW_PAD + dh + 4.0 + PREVIEW_BAR_H + PREVIEW_PAD;
    let image_rect = LRect {
        x: (PREVIEW_W - dw) / 2.0,
        y: PREVIEW_PAD,
        w: dw,
        h: dh,
    };
    let (w_px, h_px) = (px(PREVIEW_W, s), px(height, s));
    let work = m.work;
    let margin = px(8.0, s);
    let (x, y) = match anchor {
        // Rechts unterhalb des Punkts, ganz auf dem Monitor.
        Some(p) => (
            (p.x + px(16.0, s)).clamp(
                work.left + margin,
                (work.right - w_px - margin).max(work.left),
            ),
            (p.y + px(16.0, s)).clamp(
                work.top + margin,
                (work.bottom - h_px - margin).max(work.top),
            ),
        ),
        None => {
            let stacked: i32 = PREVIEWS.with(|p| {
                p.borrow()
                    .iter()
                    .filter(|st| !st.closed.get())
                    .map(|st| st.height + margin)
                    .sum()
            });
            (
                work.right - w_px - px(16.0, s),
                work.bottom - h_px - px(16.0, s) - stacked,
            )
        }
    };

    let dib = Dib::new(image.width() as i32, image.height() as i32)
        .map_err(|e| log_error("Vorschaubild", &e))?;
    fill_bgra(&dib, &image);
    let state = Rc::new(PreviewState {
        hwnd: Cell::new(HWND::default()),
        on_action,
        file: file.to_string(),
        labels: labels.map(|l| l.to_string()),
        scale: s,
        image: dib,
        image_rect,
        height: h_px,
        font: font(11.5, s, true),
        hovered: Cell::new(false),
        tracking: Cell::new(false),
        down: Cell::new(None),
        dragging: Cell::new(false),
        closed: Cell::new(false),
    });
    // Erst eintragen, dann erzeugen: WM_PAINT kann schon beim Anzeigen kommen.
    PREVIEWS.with(|p| p.borrow_mut().push(state.clone()));
    let hwnd = match create_popup(
        PREVIEW_CLASS,
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
        RECT {
            left: x,
            top: y,
            right: x + w_px,
            bottom: y + h_px,
        },
    ) {
        Ok(h) => h,
        Err(e) => {
            PREVIEWS.with(|p| p.borrow_mut().retain(|st| !Rc::ptr_eq(st, &state)));
            return Err(log_error("Vorschau", &e));
        }
    };
    state.hwnd.set(hwnd);
    round_window(hwnd);
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, swp_keep());
        SetTimer(Some(hwnd), PREVIEW_TIMER, PREVIEW_MS, None);
    }
    Ok(())
}

const PREVIEW_CLASS: PCWSTR = w!("NabshotPreview");

fn preview_state(hwnd: HWND) -> Option<Rc<PreviewState>> {
    PREVIEWS.with(|p| p.borrow().iter().find(|st| st.hwnd.get() == hwnd).cloned())
}

fn close_preview(st: &PreviewState) {
    if st.closed.replace(true) {
        return;
    }
    let hwnd = st.hwnd.get();
    PREVIEWS.with(|p| p.borrow_mut().retain(|s| s.hwnd.get() != hwnd));
    unsafe {
        let _ = KillTimer(Some(hwnd), PREVIEW_TIMER);
        let _ = DestroyWindow(hwnd);
    }
}

unsafe extern "system" fn preview_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_MOUSEACTIVATE => return LRESULT(MA_NOACTIVATE as isize),
        WM_ERASEBKGND => return LRESULT(1),
        WM_SETCURSOR => {
            set_arrow_cursor();
            return LRESULT(1);
        }
        WM_DESTROY => return LRESULT(0),
        _ => {}
    }
    let Some(st) = preview_state(hwnd) else {
        return unsafe { DefWindowProcW(hwnd, msg, wp, lp) };
    };
    let logical = |lp: LPARAM| {
        let (x, y) = mouse_point(lp);
        (x as f64 / st.scale, y as f64 / st.scale)
    };
    match msg {
        WM_PAINT => {
            paint_with(hwnd, |dc, size| unsafe { paint_preview(dc, size, &st) });
        }
        WM_TIMER if wp.0 == PREVIEW_TIMER => {
            // Solange die Maus darüber ist, bleibt die Vorschau offen.
            if !st.hovered.get() && !st.dragging.get() {
                close_preview(&st);
            }
        }
        WM_MOUSEMOVE => {
            if !st.tracking.get() {
                track_leave(hwnd);
                st.tracking.set(true);
                st.hovered.set(true);
            }
            let p = mouse_point(lp);
            let pressed = wp.0 as u32 & MK_LBUTTON.0 != 0;
            if let Some(start) = st.down.get()
                && pressed
                && !st.dragging.get()
                && st
                    .image_rect
                    .contains((start.0 as f64 / st.scale, start.1 as f64 / st.scale))
            {
                let threshold = unsafe { GetSystemMetrics(SM_CXDRAG) }.max(4);
                if (p.0 - start.0).abs() > threshold || (p.1 - start.1).abs() > threshold {
                    st.dragging.set(true);
                    let dropped = drag_file(hwnd, &st.file);
                    st.dragging.set(false);
                    st.down.set(None);
                    if dropped {
                        (st.on_action)(PreviewAction::Dragged);
                        close_preview(&st);
                    }
                }
            }
        }
        WM_MOUSELEAVE => {
            st.tracking.set(false);
            st.hovered.set(false);
        }
        WM_LBUTTONDOWN => st.down.set(Some(mouse_point(lp))),
        WM_LBUTTONUP => {
            let Some(start) = st.down.take() else {
                return LRESULT(0);
            };
            let p = mouse_point(lp);
            if st.dragging.get() || (p.0 - start.0).abs() > 4 || (p.1 - start.1).abs() > 4 {
                return LRESULT(0);
            }
            if let Some(action) = preview_hit(st.image_rect, logical(lp)) {
                let closes = !matches!(action, PreviewAction::Copy | PreviewAction::Text);
                (st.on_action)(action);
                if closes {
                    close_preview(&st);
                }
            }
        }
        _ => return unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
    LRESULT(0)
}

unsafe fn paint_preview(dc: HDC, size: (i32, i32), st: &PreviewState) {
    let s = st.scale;
    unsafe {
        let bg = CreateSolidBrush(rgb(31, 31, 33));
        let button = CreateSolidBrush(rgb(53, 53, 55));
        let close_bg = CreateSolidBrush(rgb(12, 12, 13));
        FillRect(dc, &full_rect(size.0, size.1), bg);

        // Bild mit runden Ecken
        let ir = st.image_rect.to_px(s);
        let saved = SaveDC(dc);
        let rgn = CreateRoundRectRgn(
            ir.left,
            ir.top,
            ir.right + 1,
            ir.bottom + 1,
            px(12.0, s),
            px(12.0, s),
        );
        ExtSelectClipRgn(dc, Some(rgn), RGN_AND);
        SetStretchBltMode(dc, HALFTONE);
        let _ = SetBrushOrgEx(dc, 0, 0, None);
        let _ = StretchBlt(
            dc,
            ir.left,
            ir.top,
            ir.right - ir.left,
            ir.bottom - ir.top,
            Some(st.image.dc),
            0,
            0,
            st.image.width,
            st.image.height,
            SRCCOPY,
        );
        let _ = RestoreDC(dc, saved);
        let _ = DeleteObject(rgn.into());

        // ✕
        let cr = close_rect(st.image_rect).to_px(s);
        let old_pen = SelectObject(dc, GetStockObject(NULL_PEN));
        let old_brush = SelectObject(dc, close_bg.into());
        let _ = Ellipse(dc, cr.left, cr.top, cr.right, cr.bottom);
        SelectObject(dc, old_brush);
        // Das Kreuz als zwei Linien (Symbolschriften fehlen GDI manchmal).
        let pen = CreatePen(PS_SOLID, px(1.5, s).max(1), rgb(255, 255, 255));
        SelectObject(dc, pen.into());
        let inset = px(7.0, s);
        let (l, t, r, b) = (
            cr.left + inset,
            cr.top + inset,
            cr.right - inset,
            cr.bottom - inset,
        );
        let _ = MoveToEx(dc, l, t, None);
        let _ = LineTo(dc, r, b);
        let _ = MoveToEx(dc, r, t, None);
        let _ = LineTo(dc, l, b);
        SelectObject(dc, old_pen);
        let _ = DeleteObject(pen.into());

        // Knopfleiste
        let bar_y = st.image_rect.y + st.image_rect.h + 4.0;
        let w = (PREVIEW_W - 2.0 * PREVIEW_PAD) / 4.0;
        for (i, label) in st.labels.iter().enumerate() {
            let x = PREVIEW_PAD + i as f64 * w;
            let r = LRect {
                x: x + 2.0,
                y: bar_y + 3.0,
                w: w - 4.0,
                h: PREVIEW_BAR_H - 6.0,
            }
            .to_px(s);
            round_fill(dc, r, px(6.0, s), button);
            draw_text(dc, label, r, st.font, rgb(255, 255, 255), DT_CENTER);
        }
        for b in [bg, button, close_bg] {
            let _ = DeleteObject(b.into());
        }
    }
}

/// Zieht die Datei per Drag & Drop in eine andere App (nur Kopieren – die
/// Bibliothek behält ihre Datei). Blockiert, bis losgelassen wird.
fn drag_file(hwnd: HWND, file: &str) -> bool {
    let path: Vec<u16> = file.encode_utf16().chain(std::iter::once(0)).collect();
    // OLE wird pro Aufruf gezählt; Tauri hat es im Haupt-Thread meist schon an.
    let ole = unsafe { OleInitialize(None) };
    let result = (|| -> windows::core::Result<bool> {
        let item: IShellItem = unsafe { SHCreateItemFromParsingName(PCWSTR(path.as_ptr()), None) }?;
        let data: windows::Win32::System::Com::IDataObject =
            unsafe { item.BindToHandler(None, &BHID_DataObject) }?;
        // Ohne eigene Quelle: die Shell nimmt ihre Standardquelle und zeigt
        // das Vorschaubild der Datei beim Ziehen an.
        let effect = unsafe {
            SHDoDragDrop(
                Some(hwnd),
                &data,
                None::<&windows::Win32::System::Ole::IDropSource>,
                DROPEFFECT_COPY,
            )
        }?;
        Ok(effect != DROPEFFECT_NONE)
    })();
    if ole.is_ok() {
        unsafe { OleUninitialize() };
    }
    match result {
        Ok(dropped) => dropped,
        Err(e) => {
            log_error("Ziehen", &e.message());
            false
        }
    }
}

// --- Hinweis --------------------------------------------------------------------

/// Wie lange der Hinweis sichtbar bleibt.
const TOAST_MS: u32 = 1600;
const TOAST_TIMER: usize = 1;

struct Toast {
    hwnd: HWND,
    text: String,
    font: HFONT,
}

impl Drop for Toast {
    fn drop(&mut self) {
        let _ = unsafe { DeleteObject(self.font.into()) };
    }
}

thread_local! {
    static TOASTS: RefCell<Vec<Toast>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn show_toast(text: &str) {
    let _dpi = DpiGuard::new();
    if register_class(TOAST_CLASS, Some(toast_proc)).is_err() {
        return;
    }
    // Auf dem Monitor, auf dem die Maus ist.
    let mut cursor = POINT::default();
    let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::GetCursorPos(&mut cursor) };
    let Some(m) = monitor_info(unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTOPRIMARY) })
    else {
        return;
    };
    let s = m.scale;
    let font = font(15.0, s, false);
    let text_w = measure_text(text, font).0;
    let width = (text_w + px(40.0, s)).min(m.rect.right - m.rect.left - px(80.0, s));
    let height = px(40.0, s);
    let work = m.work;
    let x = work.left + (work.right - work.left - width) / 2;
    let y = work.bottom - px(90.0, s) - height;
    let hwnd = match create_popup(
        TOAST_CLASS,
        WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_LAYERED | WS_EX_TRANSPARENT,
        RECT {
            left: x,
            top: y,
            right: x + width,
            bottom: y + height,
        },
    ) {
        Ok(h) => h,
        Err(e) => {
            log_error("Hinweis", &e);
            let _ = unsafe { DeleteObject(font.into()) };
            return;
        }
    };
    TOASTS.with(|t| {
        t.borrow_mut().push(Toast {
            hwnd,
            text: text.to_string(),
            font,
        })
    });
    // Leicht durchscheinend; Klicks gehen durch.
    let _ = unsafe { SetLayeredWindowAttributes(hwnd, COLORREF(0), 230, LWA_ALPHA) };
    round_window(hwnd);
    unsafe {
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, swp_keep());
        SetTimer(Some(hwnd), TOAST_TIMER, TOAST_MS, None);
    }
}

const TOAST_CLASS: PCWSTR = w!("NabshotToast");

unsafe extern "system" fn toast_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_ERASEBKGND => LRESULT(1),
        WM_PAINT => {
            let content = TOASTS.with(|t| {
                t.borrow()
                    .iter()
                    .find(|t| t.hwnd == hwnd)
                    .map(|t| (t.text.clone(), t.font))
            });
            paint_with(hwnd, |dc, size| unsafe {
                let bg = CreateSolidBrush(rgb(26, 26, 26));
                FillRect(dc, &full_rect(size.0, size.1), bg);
                let _ = DeleteObject(bg.into());
                if let Some((text, font)) = &content {
                    draw_text(
                        dc,
                        text,
                        full_rect(size.0, size.1),
                        *font,
                        rgb(255, 255, 255),
                        DT_CENTER,
                    );
                }
            });
            LRESULT(0)
        }
        WM_TIMER if wp.0 == TOAST_TIMER => {
            unsafe {
                let _ = KillTimer(Some(hwnd), TOAST_TIMER);
                let _ = DestroyWindow(hwnd);
            }
            TOASTS.with(|t| t.borrow_mut().retain(|t| t.hwnd != hwnd));
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

// --- Monitore --------------------------------------------------------------------

struct MonitorInfo {
    /// Wie `nabshot-capture`: der Wert des `HMONITOR`.
    id: u32,
    rect: RECT,
    /// Ohne Taskleiste.
    work: RECT,
    scale: f64,
}

fn monitors() -> Vec<MonitorInfo> {
    unsafe extern "system" fn collect(h: HMONITOR, _: HDC, _: *mut RECT, data: LPARAM) -> BOOL {
        // SAFETY: `data` zeigt auf den Vec unten, solange EnumDisplayMonitors läuft.
        let list = unsafe { &mut *(data.0 as *mut Vec<HMONITOR>) };
        list.push(h);
        BOOL(1)
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

fn monitor_info(h: HMONITOR) -> Option<MonitorInfo> {
    let mut info = MONITORINFO {
        cbSize: size_of::<MONITORINFO>() as u32,
        ..Default::default()
    };
    if !unsafe { GetMonitorInfoW(h, &mut info) }.as_bool() {
        return None;
    }
    let (mut dx, mut dy) = (96u32, 96u32);
    let _ = unsafe { GetDpiForMonitor(h, MDT_EFFECTIVE_DPI, &mut dx, &mut dy) };
    Some(MonitorInfo {
        id: h.0 as usize as u32,
        rect: info.rcMonitor,
        work: info.rcWork,
        scale: dx.max(96) as f64 / 96.0,
    })
}

// --- Fenster ---------------------------------------------------------------------

fn register_class(name: PCWSTR, proc: WNDPROC) -> Result<(), OverlayError> {
    let instance = instance();
    let wc = WNDCLASSEXW {
        cbSize: size_of::<WNDCLASSEXW>() as u32,
        style: CS_HREDRAW | CS_VREDRAW,
        lpfnWndProc: proc,
        hInstance: instance,
        hCursor: unsafe { LoadCursorW(None, IDC_ARROW) }.unwrap_or_default(),
        lpszClassName: name,
        ..Default::default()
    };
    // Schlägt fehl, wenn die Klasse schon registriert ist – das ist in Ordnung.
    let _ = unsafe { RegisterClassExW(&wc) };
    Ok(())
}

fn instance() -> HINSTANCE {
    unsafe { GetModuleHandleW(None) }
        .map(|m| HINSTANCE(m.0))
        .unwrap_or_default()
}

/// Randloses Popup-Fenster (noch unsichtbar), nie in Aufnahmen zu sehen.
fn create_popup(class: PCWSTR, ex: WINDOW_EX_STYLE, r: RECT) -> Result<HWND, String> {
    let hwnd = unsafe {
        CreateWindowExW(
            ex,
            class,
            w!("Nabshot"),
            WS_POPUP,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            None,
            None,
            Some(instance()),
            None,
        )
    }
    .map_err(|e| e.message())?;
    let _ = unsafe { SetWindowDisplayAffinity(hwnd, WDA_EXCLUDEFROMCAPTURE) };
    Ok(hwnd)
}

/// Runde Ecken wie bei normalen Fenstern (nur Windows 11; sonst eckig).
fn round_window(hwnd: HWND) {
    let pref = DWMWCP_ROUND;
    let _ = unsafe {
        DwmSetWindowAttribute(
            hwnd,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &pref as *const _ as *const c_void,
            size_of_val(&pref) as u32,
        )
    };
}

fn swp_keep() -> windows::Win32::UI::WindowsAndMessaging::SET_WINDOW_POS_FLAGS {
    use windows::Win32::UI::WindowsAndMessaging::{SWP_NOMOVE, SWP_NOSIZE};
    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE | SWP_SHOWWINDOW
}

/// Meldet WM_MOUSELEAVE an, sobald die Maus das Fenster verlässt.
fn track_leave(hwnd: HWND) {
    let mut t = TRACKMOUSEEVENT {
        cbSize: size_of::<TRACKMOUSEEVENT>() as u32,
        dwFlags: TME_LEAVE,
        hwndTrack: hwnd,
        dwHoverTime: 0,
    };
    let _ = unsafe { TrackMouseEvent(&mut t) };
}

fn set_arrow_cursor() {
    if let Ok(c) = unsafe { LoadCursorW(None, IDC_ARROW) } {
        unsafe { SetCursor(Some(c)) };
    }
}

/// Mausposition aus `lParam` (vorzeichenbehaftet – ausserhalb des Fensters
/// beim Ziehen auch negativ).
fn mouse_point(lp: LPARAM) -> (i32, i32) {
    let x = (lp.0 & 0xFFFF) as u16 as i16 as i32;
    let y = ((lp.0 >> 16) & 0xFFFF) as u16 as i16 as i32;
    (x, y)
}

/// Zeichnet ein ganzes Fenster in einem Zwischenpuffer (kein Flackern).
fn paint_with(hwnd: HWND, f: impl FnOnce(HDC, (i32, i32))) {
    unsafe {
        let mut ps = PAINTSTRUCT::default();
        let dc = BeginPaint(hwnd, &mut ps);
        if !dc.is_invalid() {
            let mut r = RECT::default();
            let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(hwnd, &mut r);
            let size = (r.right - r.left, r.bottom - r.top);
            if let Ok(buf) = Dib::new(size.0, size.1) {
                f(buf.dc, size);
                blit(dc, full_rect(size.0, size.1), buf.dc, (0, 0));
            }
        }
        let _ = EndPaint(hwnd, &ps);
    }
}

// --- DPI ---------------------------------------------------------------------------

/// Physische Pixel im aktuellen Thread, solange der Wert lebt. Fenster, die
/// in dieser Zeit entstehen, bleiben „per monitor aware v2“.
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

// --- GDI ---------------------------------------------------------------------------

/// 32-Bit-Bitmap (BGRA, oben links zuerst) mit eigenem Gerätekontext.
struct Dib {
    dc: HDC,
    bitmap: HBITMAP,
    old: HGDIOBJ,
    bits: *mut u8,
    width: i32,
    height: i32,
}

impl Dib {
    fn new(width: i32, height: i32) -> Result<Self, String> {
        let (width, height) = (width.max(1), height.max(1));
        let dc = unsafe { CreateCompatibleDC(None) };
        if dc.is_invalid() {
            return Err("CreateCompatibleDC".into());
        }
        let info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width,
                biHeight: -height,
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
                return Err(format!(
                    "Bitmap {width}×{height} konnte nicht angelegt werden"
                ));
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

    /// Ein einzelnes Pixel in dieser Farbe (für AlphaBlend).
    fn solid(r: u8, g: u8, b: u8) -> Result<Self, String> {
        let dib = Self::new(1, 1)?;
        dib.bytes_mut().copy_from_slice(&[b, g, r, 255]);
        Ok(dib)
    }

    #[allow(clippy::mut_from_ref)]
    fn bytes_mut(&self) -> &mut [u8] {
        let _ = unsafe { GdiFlush() };
        // SAFETY: CreateDIBSection hat width × height × 4 Bytes angelegt; nur
        // der Haupt-Thread greift darauf zu, und nie zugleich mit GDI.
        unsafe {
            std::slice::from_raw_parts_mut(
                self.bits,
                self.width as usize * self.height as usize * 4,
            )
        }
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

/// RGBA-Bild in eine gleich grosse Bitmap (BGRA).
fn fill_bgra(dib: &Dib, image: &RgbaImage) {
    let dst = dib.bytes_mut();
    for (o, s) in dst
        .as_chunks_mut::<4>()
        .0
        .iter_mut()
        .zip(image.data().as_chunks::<4>().0)
    {
        // Durchsichtiges (runde Fensterecken) auf den dunklen Grund der Vorschau.
        let a = s[3] as u32;
        let mix = |c: u8| ((c as u32 * a + 31 * (255 - a) + 127) / 255) as u8;
        *o = [mix(s[2]), mix(s[1]), mix(s[0]), 255];
    }
}

fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF(r as u32 | (g as u32) << 8 | (b as u32) << 16)
}

/// Logische Pixel → physische Pixel.
fn px(v: f64, scale: f64) -> i32 {
    (v * scale).round() as i32
}

fn font(points: f64, scale: f64, semibold: bool) -> HFONT {
    unsafe {
        CreateFontW(
            // Negativ: Höhe des Schriftgrads in Pixeln (wie `fontSize` unter macOS).
            -px(points, scale),
            0,
            0,
            0,
            if semibold {
                FW_SEMIBOLD.0 as i32
            } else {
                FW_NORMAL.0 as i32
            },
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            w!("Segoe UI"),
        )
    }
}

/// Breite und Höhe eines Textes in Pixeln.
fn measure_text(text: &str, font: HFONT) -> (i32, i32) {
    unsafe {
        let dc = CreateCompatibleDC(None);
        let old = SelectObject(dc, font.into());
        let mut wide: Vec<u16> = text.encode_utf16().collect();
        let mut r = RECT::default();
        DrawTextW(
            dc,
            &mut wide,
            &mut r,
            DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX,
        );
        SelectObject(dc, old);
        let _ = DeleteDC(dc);
        (r.right - r.left, r.bottom - r.top)
    }
}

unsafe fn draw_text(
    dc: HDC,
    text: &str,
    r: RECT,
    font: HFONT,
    color: COLORREF,
    align: windows::Win32::Graphics::Gdi::DRAW_TEXT_FORMAT,
) {
    let mut wide: Vec<u16> = text.encode_utf16().collect();
    let mut r = r;
    unsafe {
        let old = SelectObject(dc, font.into());
        SetBkMode(dc, TRANSPARENT);
        SetTextColor(dc, color);
        DrawTextW(
            dc,
            &mut wide,
            &mut r,
            align | DT_VCENTER | DT_SINGLELINE | DT_NOPREFIX | DT_END_ELLIPSIS,
        );
        SelectObject(dc, old);
    }
}

unsafe fn blit(dst: HDC, r: RECT, src: HDC, from: (i32, i32)) {
    let _ = unsafe {
        BitBlt(
            dst,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            Some(src),
            from.0,
            from.1,
            SRCCOPY,
        )
    };
}

unsafe fn fill(dc: HDC, r: RECT, brush: HBRUSH) {
    unsafe {
        let old = SelectObject(dc, brush.into());
        let _ = PatBlt(
            dc,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            PATCOPY,
        );
        SelectObject(dc, old);
    }
}

/// Rahmen der Dicke `t` innen an `r`.
unsafe fn frame(dc: HDC, r: RECT, t: i32, brush: HBRUSH) {
    let t = t.max(1);
    unsafe {
        fill(
            dc,
            RECT {
                bottom: r.top + t,
                ..r
            },
            brush,
        );
        fill(
            dc,
            RECT {
                top: r.bottom - t,
                ..r
            },
            brush,
        );
        fill(
            dc,
            RECT {
                right: r.left + t,
                ..r
            },
            brush,
        );
        fill(
            dc,
            RECT {
                left: r.right - t,
                ..r
            },
            brush,
        );
    }
}

unsafe fn round_fill(dc: HDC, r: RECT, radius: i32, brush: HBRUSH) {
    unsafe {
        let old_pen = SelectObject(dc, GetStockObject(NULL_PEN));
        let old_brush = SelectObject(dc, brush.into());
        let _ = RoundRect(
            dc,
            r.left,
            r.top,
            r.right + 1,
            r.bottom + 1,
            radius * 2,
            radius * 2,
        );
        SelectObject(dc, old_brush);
        SelectObject(dc, old_pen);
    }
}

/// Legt die Farbe von `color` (1 × 1 Pixel) mit `alpha` über `r`.
unsafe fn blend(dc: HDC, r: RECT, color: &Dib, alpha: u8) {
    let f = BLENDFUNCTION {
        BlendOp: AC_SRC_OVER as u8,
        BlendFlags: 0,
        SourceConstantAlpha: alpha,
        AlphaFormat: 0,
    };
    let _ = unsafe {
        AlphaBlend(
            dc,
            r.left,
            r.top,
            r.right - r.left,
            r.bottom - r.top,
            color.dc,
            0,
            0,
            1,
            1,
            f,
        )
    };
}

// --- Rechtecke ----------------------------------------------------------------------

fn full_rect(w: i32, h: i32) -> RECT {
    RECT {
        left: 0,
        top: 0,
        right: w,
        bottom: h,
    }
}

fn inflate(r: RECT, d: i32) -> RECT {
    RECT {
        left: r.left - d,
        top: r.top - d,
        right: r.right + d,
        bottom: r.bottom + d,
    }
}

fn intersect(a: RECT, b: RECT) -> Option<RECT> {
    let r = RECT {
        left: a.left.max(b.left),
        top: a.top.max(b.top),
        right: a.right.min(b.right),
        bottom: a.bottom.min(b.bottom),
    };
    (r.left < r.right && r.top < r.bottom).then_some(r)
}

fn union(a: RECT, b: RECT) -> RECT {
    RECT {
        left: a.left.min(b.left),
        top: a.top.min(b.top),
        right: a.right.max(b.right),
        bottom: a.bottom.max(b.bottom),
    }
}

/// Bereiche, die sich ändern, wenn ein Rechteck (mit Rand der Breite `edge`)
/// von `old` nach `new` wechselt: die gemeinsame Innenfläche bleibt gleich.
fn changed_area(old: Option<RECT>, new: Option<RECT>, edge: i32) -> Vec<RECT> {
    match (old, new) {
        (None, None) => Vec::new(),
        (Some(r), None) | (None, Some(r)) => vec![inflate(r, edge)],
        (Some(a), Some(b)) => {
            let outer = union(inflate(a, edge), inflate(b, edge));
            match intersect(inflate(a, -edge), inflate(b, -edge)) {
                Some(inner) => subtract(outer, inner),
                None => vec![outer],
            }
        }
    }
}

/// `outer` ohne `inner` (das ganz in `outer` liegt) als bis zu vier Streifen.
fn subtract(outer: RECT, inner: RECT) -> Vec<RECT> {
    let parts = [
        RECT {
            bottom: inner.top,
            ..outer
        },
        RECT {
            top: inner.bottom,
            ..outer
        },
        RECT {
            top: inner.top,
            bottom: inner.bottom,
            right: inner.left,
            ..outer
        },
        RECT {
            top: inner.top,
            bottom: inner.bottom,
            left: inner.right,
            ..outer
        },
    ];
    parts
        .into_iter()
        .filter(|r| r.left < r.right && r.top < r.bottom)
        .collect()
}

/// Schreibt den Fehler ins Protokoll der App und gibt ihn weiter.
fn log_error(what: &str, e: &str) -> OverlayError {
    log::error!("{what}: {e}");
    OverlayError::Backend(format!("{what}: {e}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn r(l: i32, t: i32, rr: i32, b: i32) -> RECT {
        RECT {
            left: l,
            top: t,
            right: rr,
            bottom: b,
        }
    }

    fn area(rects: &[RECT]) -> i64 {
        rects
            .iter()
            .map(|r| ((r.right - r.left) * (r.bottom - r.top)) as i64)
            .sum()
    }

    #[test]
    fn wachsende_auswahl_zeichnet_nur_den_rand_neu() {
        let dirty = changed_area(Some(r(100, 100, 300, 300)), Some(r(100, 100, 310, 300)), 2);
        // Statt 210 × 200 Pixel nur ein schmaler Streifen plus Ränder.
        assert!(area(&dirty) < 10_000, "{dirty:?}");
        // Der neue Rand liegt im neu gezeichneten Bereich.
        assert!(dirty.iter().any(|d| d.left <= 309 && d.right >= 311));
    }

    #[test]
    fn neue_auswahl_wird_ganz_gezeichnet() {
        assert_eq!(
            changed_area(None, Some(r(0, 0, 10, 10)), 1),
            [r(-1, -1, 11, 11)]
        );
    }

    #[test]
    fn rechteck_ohne_innenflaeche() {
        let parts = subtract(r(0, 0, 10, 10), r(2, 2, 8, 8));
        assert_eq!(area(&parts), 100 - 36);
    }

    #[test]
    fn vorschau_knoepfe() {
        let image = LRect {
            x: 10.0,
            y: 8.0,
            w: 200.0,
            h: 100.0,
        };
        assert_eq!(
            preview_hit(image, (205.0, 12.0)),
            Some(PreviewAction::Close)
        );
        assert_eq!(preview_hit(image, (50.0, 50.0)), Some(PreviewAction::Edit));
        assert_eq!(preview_hit(image, (12.0, 125.0)), Some(PreviewAction::Copy));
        assert_eq!(
            preview_hit(image, (240.0, 125.0)),
            Some(PreviewAction::Text)
        );
        assert_eq!(preview_hit(image, (2.0, 125.0)), None);
    }

    #[test]
    fn mausposition_vorzeichenbehaftet() {
        let lp = LPARAM((((-5i16) as u16 as isize) << 16) | ((-3i16) as u16 as isize));
        assert_eq!(mouse_point(lp), (-3, -5));
    }
}
