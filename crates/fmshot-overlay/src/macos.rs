//! macOS-Overlay: pro Monitor ein randloses `NSPanel` über allem (auch über
//! Menüleiste und Vollbild-Spaces). Das Panel aktiviert die App nicht – der
//! Fokus der vorherigen App bleibt erhalten.
//!
//! Gezeichnet wird ausschliesslich mit Core-Animation-Ebenen: Das eingefrorene
//! Bild liegt einmal als Textur auf der GPU, beim Ziehen ändern sich nur Pfade
//! und Positionen. Kein Neuzeichnen von Pixeln pro Mausbewegung.
//!
//! Das Panel ist zuerst durchsichtig (laufender Bildschirm darunter) und zeigt
//! das eingefrorene Bild, sobald [`Handle::set_frozen`] es liefert.

use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::{Rc, Weak};
use std::sync::Arc;

use dispatch2::{DispatchQueue, MainThreadBound};
use fmshot_core::RgbaImage;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{
    AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSCursor, NSEvent, NSPanel, NSResponder, NSScreen,
    NSScreenSaverWindowLevel, NSTrackingArea, NSTrackingAreaOptions, NSView,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapInfo, CGColor, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage,
    CGImageAlphaInfo, CGImageByteOrderInfo, CGMutablePath, kCGColorSpaceSRGB,
};
use objc2_foundation::{NSNumber, NSObject, NSRect, NSString};
use objc2_quartz_core::{
    CALayer, CAShapeLayer, CATextLayer, CATransaction, kCAAlignmentCenter, kCAFillRuleEvenOdd,
    kCAFilterNearest, kCAGravityResize,
};

use crate::{
    DoneCallback, OverlayError, OverlayMode, OverlayResult, ScrollAction, Selection, WindowTarget,
    points_to_pixels, rect_from_points, window_at,
};

/// Escape-Taste (virtueller Tastencode).
const KEY_ESCAPE: u16 = 53;
/// Kleinere Auswahl gilt als Klick ohne Ziehen.
const MIN_DRAG_POINTS: f64 = 3.0;
const MAGNIFIER_SIZE: f64 = 128.0;
const MAGNIFIER_ZOOM: f64 = 8.0;
const DIM_ALPHA: f64 = 0.35;

pub(crate) struct Handle {
    session: Weak<RefCell<Session>>,
}

impl Handle {
    pub fn set_frozen(&self, display_id: u32, image: Arc<RgbaImage>) {
        if let Some(session) = self.session.upgrade() {
            let s = session.borrow();
            if let Some(st) = s.screens.iter().find(|st| st.display_id == display_id) {
                st.set_frozen(image);
            }
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
            .is_some_and(|s| s.borrow().on_done.is_some())
    }
}

pub(crate) fn open(
    mode: OverlayMode,
    windows: Vec<WindowTarget>,
    on_done: DoneCallback,
) -> Result<Handle, OverlayError> {
    let mtm = MainThreadMarker::new().ok_or(OverlayError::NotMainThread)?;
    let session = Rc::new(RefCell::new(Session {
        mode,
        windows,
        screens: Vec::new(),
        on_done: Some(on_done),
        drag: None,
    }));

    let ns_screens = NSScreen::screens(mtm);
    // Höhe des Hauptmonitors: Umrechnung Cocoa (unten links) ↔ global (oben links).
    let main_height = ns_screens
        .firstObject()
        .map(|s| s.frame().size.height)
        .unwrap_or(0.0);
    for (index, ns_screen) in ns_screens.iter().enumerate() {
        let Some(display_id) = screen_number(&ns_screen) else {
            continue;
        };
        let state = ScreenState::new(
            mtm,
            &ns_screen,
            display_id,
            main_height,
            index,
            session.clone(),
        );
        session.borrow_mut().screens.push(state);
    }
    if session.borrow().screens.is_empty() {
        return Err(OverlayError::NoScreens);
    }

    // Erst alles zeigen, dann das Panel unter der Maus zum Schlüsselfenster
    // machen – dort landen Tastendrücke (Esc).
    let mouse = NSEvent::mouseLocation();
    {
        let s = session.borrow();
        for st in &s.screens {
            st.panel.orderFrontRegardless();
        }
        if let Some(st) = s
            .screens
            .iter()
            .find(|st| contains(st.panel.frame(), mouse))
        {
            st.panel.makeKeyWindow();
            st.panel.makeFirstResponder(Some(&st.view));
            st.pointer_moved(&s, st.view.convert_from_screen(mouse));
        }
    }
    NSCursor::crosshairCursor().set();
    Ok(Handle {
        session: Rc::downgrade(&session),
    })
}

struct Session {
    mode: OverlayMode,
    windows: Vec<WindowTarget>,
    screens: Vec<ScreenState>,
    on_done: Option<DoneCallback>,
    drag: Option<Drag>,
}

struct Drag {
    screen: usize,
    start: (f64, f64),
}

impl Session {
    /// Beendet das Overlay. Gibt den Rückruf zurück – er wird erst aufgerufen,
    /// wenn kein `borrow` mehr aktiv ist.
    fn finish(&mut self, result: OverlayResult) -> Option<(DoneCallback, OverlayResult)> {
        let on_done = self.on_done.take()?;
        let screens = std::mem::take(&mut self.screens);
        for st in &screens {
            st.panel.orderOut(None);
        }
        // Die Panels erst nach dem laufenden Ereignis freigeben, nicht mitten
        // in ihrem eigenen `mouseUp:`.
        if let Some(mtm) = MainThreadMarker::new() {
            let bound = MainThreadBound::new(screens, mtm);
            DispatchQueue::main().exec_async(move || {
                if let Some(mtm) = MainThreadMarker::new() {
                    drop(bound.into_inner(mtm));
                }
            });
        }
        NSCursor::arrowCursor().set();
        Some((on_done, result))
    }
}

/// Ruft `f` mit der Sitzung auf und danach – ausserhalb des Borrows – einen
/// eventuell fälligen Abschluss-Rückruf.
fn with_session(
    session: &Rc<RefCell<Session>>,
    f: impl FnOnce(&mut Session) -> Option<(DoneCallback, OverlayResult)>,
) {
    let done = f(&mut session.borrow_mut());
    if let Some((on_done, result)) = done {
        on_done(result);
    }
}

struct ScreenState {
    display_id: u32,
    /// Ursprung des Monitors in globalen Punkten (oben links).
    origin: (f64, f64),
    points_size: (f64, f64),
    scale: f64,
    panel: Retained<OverlayPanel>,
    view: Retained<OverlayView>,
    image: Retained<CALayer>,
    dim: Retained<CAShapeLayer>,
    border: Retained<CAShapeLayer>,
    highlight: Retained<CAShapeLayer>,
    guide_shadow: Retained<CAShapeLayer>,
    guide: Retained<CAShapeLayer>,
    label: Retained<CATextLayer>,
    magnifier: Retained<CALayer>,
    magnifier_image: Retained<CALayer>,
    frozen: RefCell<bool>,
}

impl ScreenState {
    fn new(
        mtm: MainThreadMarker,
        ns_screen: &NSScreen,
        display_id: u32,
        main_height: f64,
        index: usize,
        session: Rc<RefCell<Session>>,
    ) -> Self {
        let frame = ns_screen.frame();
        let scale = ns_screen.backingScaleFactor();
        let bounds = NSRect::new(CGPoint::new(0.0, 0.0), frame.size);
        let origin = (
            frame.origin.x,
            main_height - (frame.origin.y + frame.size.height),
        );

        let panel = OverlayPanel::new(mtm, frame);
        let view = OverlayView::new(mtm, bounds, ViewIvars { session, index });
        view.setWantsLayer(true);
        let root = view.layer().expect("Ebene nach setWantsLayer");

        // Eingefrorenes Bild (kommt nach)
        let image = CALayer::new();
        image.setFrame(bounds);
        image.setContentsGravity(unsafe { kCAGravityResize });
        root.addSublayer(&image);

        // Abdunklung mit Loch für die Auswahl
        let dim = shape_layer(bounds);
        dim.setFillRule(unsafe { kCAFillRuleEvenOdd });
        dim.setFillColor(Some(&CGColor::new_srgb(0.0, 0.0, 0.0, DIM_ALPHA)));
        root.addSublayer(&dim);

        // Fenster-Hervorhebung
        let highlight = shape_layer(bounds);
        highlight.setFillColor(Some(&CGColor::new_srgb(0.2, 0.5, 1.0, 0.25)));
        highlight.setStrokeColor(Some(&CGColor::new_srgb(0.2, 0.5, 1.0, 0.95)));
        highlight.setLineWidth(2.0);
        root.addSublayer(&highlight);

        // Fadenkreuz über den ganzen Monitor – dunkler Schatten für Kontrast
        // auf hellem Grund, darüber eine helle Linie.
        let guide_shadow = shape_layer(bounds);
        guide_shadow.setStrokeColor(Some(&CGColor::new_srgb(0.0, 0.0, 0.0, 0.45)));
        guide_shadow.setLineWidth(3.0);
        root.addSublayer(&guide_shadow);
        let guide = shape_layer(bounds);
        guide.setStrokeColor(Some(&CGColor::new_srgb(1.0, 1.0, 1.0, 0.9)));
        guide.setLineWidth(1.0);
        root.addSublayer(&guide);

        // Auswahlrahmen
        let border = shape_layer(bounds);
        border.setStrokeColor(Some(&CGColor::new_srgb(1.0, 1.0, 1.0, 0.95)));
        border.setLineWidth(1.0);
        root.addSublayer(&border);

        // Grössenanzeige „1200 × 800“
        let label = CATextLayer::new();
        label.setFontSize(12.0);
        label.setContentsScale(scale);
        label.setAlignmentMode(unsafe { kCAAlignmentCenter });
        label.setForegroundColor(Some(&CGColor::new_srgb(1.0, 1.0, 1.0, 1.0)));
        label.setBackgroundColor(Some(&CGColor::new_srgb(0.0, 0.0, 0.0, 0.75)));
        label.setCornerRadius(4.0);
        label.setHidden(true);
        root.addSublayer(&label);

        // Lupe: Behälter mit Maske, darin das ganze Bild vergrössert.
        let magnifier = CALayer::new();
        magnifier.setMasksToBounds(true);
        magnifier.setCornerRadius(MAGNIFIER_SIZE / 2.0);
        magnifier.setBorderWidth(2.0);
        magnifier.setBorderColor(Some(&CGColor::new_srgb(1.0, 1.0, 1.0, 0.9)));
        magnifier.setBackgroundColor(Some(&CGColor::new_srgb(0.0, 0.0, 0.0, 1.0)));
        magnifier.setHidden(true);
        let magnifier_image = CALayer::new();
        magnifier_image.setContentsGravity(unsafe { kCAGravityResize });
        magnifier_image.setMagnificationFilter(unsafe { kCAFilterNearest });
        magnifier.addSublayer(&magnifier_image);
        // Ein Pixel in der Mitte der Lupe markieren
        let center = CAShapeLayer::new();
        let px = MAGNIFIER_ZOOM / scale.max(1.0);
        center.setPath(Some(&rect_path(CGRect::new(
            CGPoint::new((MAGNIFIER_SIZE - px) / 2.0, (MAGNIFIER_SIZE - px) / 2.0),
            CGSize::new(px, px),
        ))));
        center.setFillColor(None);
        center.setStrokeColor(Some(&CGColor::new_srgb(1.0, 0.25, 0.2, 1.0)));
        center.setLineWidth(1.0);
        magnifier.addSublayer(&center);
        root.addSublayer(&magnifier);

        panel.setContentView(Some(&view));

        let tracking = unsafe {
            NSTrackingArea::initWithRect_options_owner_userInfo(
                NSTrackingArea::alloc(),
                bounds,
                NSTrackingAreaOptions::MouseMoved
                    | NSTrackingAreaOptions::MouseEnteredAndExited
                    | NSTrackingAreaOptions::ActiveAlways
                    | NSTrackingAreaOptions::InVisibleRect,
                Some(&view),
                None,
            )
        };
        view.addTrackingArea(&tracking);

        let state = Self {
            display_id,
            origin,
            points_size: (bounds.size.width, bounds.size.height),
            scale,
            panel,
            view,
            image,
            dim,
            border,
            highlight,
            guide_shadow,
            guide,
            label,
            magnifier,
            magnifier_image,
            frozen: RefCell::new(false),
        };
        state.update_selection(None);
        state
    }

    fn pixel_size(&self) -> (u32, u32) {
        (
            (self.points_size.0 * self.scale).round() as u32,
            (self.points_size.1 * self.scale).round() as u32,
        )
    }

    fn set_frozen(&self, image: Arc<RgbaImage>) {
        let Some(cg) = cgimage_from_rgba(image) else {
            return;
        };
        without_animation(|| {
            set_contents(&self.image, &cg);
            set_contents(&self.magnifier_image, &cg);
            let (w, h) = self.points_size;
            self.magnifier_image.setFrame(CGRect::new(
                CGPoint::new(0.0, 0.0),
                CGSize::new(w * MAGNIFIER_ZOOM, h * MAGNIFIER_ZOOM),
            ));
        });
        *self.frozen.borrow_mut() = true;
    }

    /// Mausbewegung ohne gedrückte Taste: Fadenkreuz, Lupe, Fenster-Hervorhebung.
    fn pointer_moved(&self, session: &Session, p: (f64, f64)) {
        self.update_guides(Some(p));
        self.update_magnifier(Some(p));
        if session.mode == OverlayMode::Window {
            let global = (self.origin.0 + p.0, self.origin.1 + p.1);
            let rect = window_at(&session.windows, global).map(|w| {
                let (x, y, ww, wh) = w.bounds;
                (x - self.origin.0, y - self.origin.1, ww, wh)
            });
            self.update_highlight(rect);
        }
    }

    fn pointer_left(&self) {
        self.update_guides(None);
        self.update_magnifier(None);
        self.update_highlight(None);
    }

    /// Waagrechte und senkrechte Linie durch den Mauszeiger.
    fn update_guides(&self, p: Option<(f64, f64)>) {
        without_animation(|| match p {
            Some((x, y)) => {
                let (w, h) = self.points_size;
                let path = CGMutablePath::new();
                // Halber Punkt Versatz: 1-Punkt-Linie liegt scharf auf dem Raster.
                let (x, y) = (x.floor() + 0.5, y.floor() + 0.5);
                unsafe {
                    CGMutablePath::move_to_point(Some(&path), std::ptr::null(), 0.0, y);
                    CGMutablePath::add_line_to_point(Some(&path), std::ptr::null(), w, y);
                    CGMutablePath::move_to_point(Some(&path), std::ptr::null(), x, 0.0);
                    CGMutablePath::add_line_to_point(Some(&path), std::ptr::null(), x, h);
                }
                self.guide.setPath(Some(&path));
                self.guide_shadow.setPath(Some(&path));
            }
            None => {
                self.guide.setPath(None);
                self.guide_shadow.setPath(None);
            }
        });
    }

    fn update_highlight(&self, rect: Option<(f64, f64, f64, f64)>) {
        without_animation(|| match rect {
            Some((x, y, w, h)) => {
                let r = CGRect::new(
                    CGPoint::new(x + 1.0, y + 1.0),
                    CGSize::new(w - 2.0, h - 2.0),
                );
                self.highlight.setPath(Some(&rect_path(r)));
            }
            None => self.highlight.setPath(None),
        });
    }

    /// Zeichnet Abdunklung, Rahmen und Grössenanzeige für die Auswahl.
    fn update_selection(&self, rect: Option<(f64, f64, f64, f64)>) {
        without_animation(|| {
            let full = CGRect::new(
                CGPoint::new(0.0, 0.0),
                CGSize::new(self.points_size.0, self.points_size.1),
            );
            let dim_path = CGMutablePath::new();
            unsafe { CGMutablePath::add_rect(Some(&dim_path), std::ptr::null(), full) };
            match rect {
                Some(r @ (x, y, w, h)) => {
                    let sel = CGRect::new(CGPoint::new(x, y), CGSize::new(w, h));
                    unsafe { CGMutablePath::add_rect(Some(&dim_path), std::ptr::null(), sel) };
                    self.border.setPath(Some(&rect_path(CGRect::new(
                        CGPoint::new(x + 0.5, y + 0.5),
                        CGSize::new((w - 1.0).max(0.0), (h - 1.0).max(0.0)),
                    ))));

                    let (_, _, pw, ph) = points_to_pixels(r, self.points_size, self.pixel_size());
                    let text = NSString::from_str(&format!("{pw} × {ph}"));
                    unsafe { self.label.setString(Some(as_any(&text))) };
                    let lw = 12.0 + 7.0 * text.length() as f64;
                    let lh = 20.0;
                    // Unter der Auswahl, sonst darüber; nie ausserhalb des Monitors.
                    let mut ly = y + h + 6.0;
                    if ly + lh > self.points_size.1 {
                        ly = (y - lh - 6.0).max(0.0);
                    }
                    let lx = (x + w - lw).clamp(0.0, (self.points_size.0 - lw).max(0.0));
                    self.label
                        .setFrame(CGRect::new(CGPoint::new(lx, ly), CGSize::new(lw, lh)));
                    self.label.setHidden(false);
                }
                None => {
                    self.border.setPath(None);
                    self.label.setHidden(true);
                }
            }
            self.dim.setPath(Some(&dim_path));
        });
    }

    /// Lupe neben dem Mauszeiger; zeigt die Umgebung des Cursors vergrössert.
    fn update_magnifier(&self, p: Option<(f64, f64)>) {
        without_animation(|| {
            let (w, h) = self.points_size;
            let Some(p) = p.filter(|p| {
                *self.frozen.borrow() && p.0 >= 0.0 && p.1 >= 0.0 && p.0 <= w && p.1 <= h
            }) else {
                self.magnifier.setHidden(true);
                return;
            };
            self.magnifier.setHidden(false);
            // Rechts unterhalb des Cursors, an Rändern auf die andere Seite.
            let offset = 24.0;
            let mut mx = p.0 + offset;
            let mut my = p.1 + offset;
            if mx + MAGNIFIER_SIZE > w {
                mx = p.0 - offset - MAGNIFIER_SIZE;
            }
            if my + MAGNIFIER_SIZE > h {
                my = p.1 - offset - MAGNIFIER_SIZE;
            }
            self.magnifier.setFrame(CGRect::new(
                CGPoint::new(mx, my),
                CGSize::new(MAGNIFIER_SIZE, MAGNIFIER_SIZE),
            ));
            let half = MAGNIFIER_SIZE / 2.0;
            self.magnifier_image.setFrame(CGRect::new(
                CGPoint::new(half - p.0 * MAGNIFIER_ZOOM, half - p.1 * MAGNIFIER_ZOOM),
                CGSize::new(w * MAGNIFIER_ZOOM, h * MAGNIFIER_ZOOM),
            ));
        });
    }

    fn clamp(&self, p: (f64, f64)) -> (f64, f64) {
        (
            p.0.clamp(0.0, self.points_size.0),
            p.1.clamp(0.0, self.points_size.1),
        )
    }
}

// --- Panel -----------------------------------------------------------------

define_class!(
    /// Randloses Panel, das Tastendrücke annimmt, ohne die App zu aktivieren.
    #[unsafe(super(NSPanel, objc2_app_kit::NSWindow, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FMShotOverlayPanel"]
    struct OverlayPanel;

    impl OverlayPanel {
        #[unsafe(method(canBecomeKeyWindow))]
        fn can_become_key_window(&self) -> bool {
            true
        }
    }
);

impl OverlayPanel {
    fn new(mtm: MainThreadMarker, frame: NSRect) -> Retained<Self> {
        let panel: Retained<Self> = unsafe {
            msg_send![
                Self::alloc(mtm),
                initWithContentRect: frame,
                styleMask: NSWindowStyleMask::Borderless | NSWindowStyleMask::NonactivatingPanel,
                backing: NSBackingStoreType::Buffered,
                defer: false
            ]
        };
        unsafe { panel.setReleasedWhenClosed(false) };
        panel.setLevel(NSScreenSaverWindowLevel);
        panel.setCollectionBehavior(
            NSWindowCollectionBehavior::CanJoinAllSpaces
                | NSWindowCollectionBehavior::FullScreenAuxiliary
                | NSWindowCollectionBehavior::Stationary
                | NSWindowCollectionBehavior::IgnoresCycle,
        );
        // Durchsichtig, bis das eingefrorene Bild da ist.
        panel.setOpaque(false);
        panel.setBackgroundColor(Some(&NSColor::clearColor()));
        panel.setHasShadow(false);
        panel.setAcceptsMouseMovedEvents(true);
        panel.setFrame_display(frame, false);
        panel
    }
}

// --- View ------------------------------------------------------------------

struct ViewIvars {
    session: Rc<RefCell<Session>>,
    index: usize,
}

define_class!(
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FMShotOverlayView"]
    #[ivars = ViewIvars]
    struct OverlayView;

    impl OverlayView {
        /// Ursprung oben links – wie in den Pixeldaten.
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstResponder))]
        fn accepts_first_responder(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            // Tastendrücke gehen an den Monitor, auf dem die Maus ist.
            if let Some(window) = self.window() {
                window.makeKeyWindow();
                window.makeFirstResponder(Some(self));
            }
            NSCursor::crosshairCursor().set();
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            let ivars = self.ivars();
            let s = ivars.session.borrow();
            // Während des Ziehens bleibt das Fadenkreuz auf dem Start-Monitor.
            if s.drag.as_ref().is_some_and(|d| d.screen == ivars.index) {
                return;
            }
            if let Some(st) = s.screens.get(ivars.index) {
                st.pointer_left();
            }
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            let p = self.location(event);
            let ivars = self.ivars();
            let s = ivars.session.borrow();
            if let Some(st) = s.screens.get(ivars.index) {
                st.pointer_moved(&s, p);
            }
            NSCursor::crosshairCursor().set();
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let p = self.location(event);
            let ivars = self.ivars();
            let mut s = ivars.session.borrow_mut();
            s.drag = Some(Drag { screen: ivars.index, start: p });
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let ivars = self.ivars();
            let s = ivars.session.borrow();
            if s.mode != OverlayMode::Region {
                return;
            }
            // Ziehen bleibt auf dem Monitor, auf dem es begonnen hat.
            let Some(drag) = s.drag.as_ref().filter(|d| d.screen == ivars.index) else {
                return;
            };
            let Some(st) = s.screens.get(ivars.index) else {
                return;
            };
            let p = st.clamp(self.location(event));
            st.update_selection(Some(rect_from_points(drag.start, p)));
            st.update_guides(Some(p));
            st.update_magnifier(Some(p));
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let p = self.location(event);
            let ivars = self.ivars();
            with_session(&ivars.session, |s| {
                let drag = s.drag.take()?;
                let st = s.screens.get(drag.screen)?;
                if s.mode == OverlayMode::Window {
                    let global = (st.origin.0 + p.0, st.origin.1 + p.1);
                    let id = window_at(&s.windows, global)?.id;
                    return s.finish(OverlayResult::Window(id));
                }
                let rect = rect_from_points(drag.start, st.clamp(p));
                if rect.2 < MIN_DRAG_POINTS || rect.3 < MIN_DRAG_POINTS {
                    // Nur geklickt – Overlay bleibt offen.
                    st.update_selection(None);
                    return None;
                }
                let (x, y, width, height) = points_to_pixels(rect, st.points_size, st.pixel_size());
                let selection = Selection { display_id: st.display_id, x, y, width, height };
                s.finish(OverlayResult::Selected(selection))
            });
        }

        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, _event: &NSEvent) {
            with_session(&self.ivars().session, |s| s.finish(OverlayResult::Cancelled));
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            if event.keyCode() == KEY_ESCAPE {
                with_session(&self.ivars().session, |s| s.finish(OverlayResult::Cancelled));
            }
        }
    }
);

impl OverlayView {
    fn new(mtm: MainThreadMarker, frame: NSRect, ivars: ViewIvars) -> Retained<Self> {
        let this = Self::alloc(mtm).set_ivars(ivars);
        unsafe { msg_send![super(this), initWithFrame: frame] }
    }

    /// Mausposition in Punkten der View (Ursprung oben links).
    fn location(&self, event: &NSEvent) -> (f64, f64) {
        let p = self.convertPoint_fromView(event.locationInWindow(), None);
        (p.x, p.y)
    }

    /// Bildschirmkoordinaten (Cocoa, Ursprung unten links) → View-Punkte.
    fn convert_from_screen(&self, p: CGPoint) -> (f64, f64) {
        let Some(window) = self.window() else {
            return (p.x, p.y);
        };
        let in_window = window.convertPointFromScreen(p);
        let v = self.convertPoint_fromView(in_window, None);
        (v.x, v.y)
    }
}

// --- Scroll-Leiste ------------------------------------------------------------

const BAR_WIDTH: f64 = 340.0;
const BAR_HEIGHT: f64 = 44.0;
/// x-Bereiche der Knöpfe in der Leiste (Punkte).
const FINISH_X: (f64, f64) = (190.0, 262.0);
const CANCEL_X: (f64, f64) = (268.0, 334.0);

pub(crate) struct ScrollUi {
    border: Retained<OverlayPanel>,
    bar: Retained<OverlayPanel>,
    status: Retained<CATextLayer>,
}

impl ScrollUi {
    pub fn set_status(&self, text: &str) {
        let s = NSString::from_str(text);
        without_animation(|| unsafe { self.status.setString(Some(as_any(&s))) });
    }

    pub fn close(&self) {
        self.border.orderOut(None);
        self.bar.orderOut(None);
    }
}

struct BarIvars {
    on_action: Box<dyn Fn(ScrollAction)>,
}

define_class!(
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FMShotScrollBarView"]
    #[ivars = BarIvars]
    struct ScrollBarView;

    impl ScrollBarView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let p = self.convertPoint_fromView(event.locationInWindow(), None);
            let action = if p.x >= FINISH_X.0 && p.x <= FINISH_X.1 {
                Some(ScrollAction::Finish)
            } else if p.x >= CANCEL_X.0 && p.x <= CANCEL_X.1 {
                Some(ScrollAction::Cancel)
            } else {
                None
            };
            if let Some(a) = action {
                (self.ivars().on_action)(a);
            }
        }
    }
);

pub(crate) fn open_scroll_ui(
    display_id: u32,
    rect: (u32, u32, u32, u32),
    labels: (&str, &str, &str),
    on_action: Box<dyn Fn(ScrollAction)>,
) -> Result<ScrollUi, OverlayError> {
    let mtm = MainThreadMarker::new().ok_or(OverlayError::NotMainThread)?;
    let screens = NSScreen::screens(mtm);
    let screen = screens
        .iter()
        .find(|s| screen_number(s) == Some(display_id))
        .ok_or(OverlayError::NoScreens)?;
    let sf = screen.frame();
    let scale = screen.backingScaleFactor().max(1.0);
    // Pixel (oben links im Monitor) → Cocoa-Koordinaten (unten links global)
    let (x, y, w, h) = (
        rect.0 as f64 / scale,
        rect.1 as f64 / scale,
        rect.2 as f64 / scale,
        rect.3 as f64 / scale,
    );
    let cocoa = NSRect::new(
        CGPoint::new(sf.origin.x + x, sf.origin.y + sf.size.height - y - h),
        CGSize::new(w, h),
    );

    // Rahmen: durchlässig für Maus und Scrollen.
    let margin = 3.0;
    let border_frame = NSRect::new(
        CGPoint::new(cocoa.origin.x - margin, cocoa.origin.y - margin),
        CGSize::new(w + 2.0 * margin, h + 2.0 * margin),
    );
    let border = OverlayPanel::new(mtm, border_frame);
    border.setIgnoresMouseEvents(true);
    let bview = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(CGPoint::new(0.0, 0.0), border_frame.size),
    );
    bview.setWantsLayer(true);
    let shape = shape_layer(NSRect::new(CGPoint::new(0.0, 0.0), border_frame.size));
    shape.setPath(Some(&rect_path(CGRect::new(
        CGPoint::new(1.0, 1.0),
        CGSize::new(
            border_frame.size.width - 2.0,
            border_frame.size.height - 2.0,
        ),
    ))));
    shape.setStrokeColor(Some(&CGColor::new_srgb(0.9, 0.22, 0.21, 1.0)));
    shape.setLineWidth(2.0);
    bview.layer().expect("Ebene").addSublayer(&shape);
    border.setContentView(Some(&bview));

    // Leiste unter dem Bereich, sonst darüber, sonst innen unten.
    let vf = screen.visibleFrame();
    let bx = (cocoa.origin.x + (w - BAR_WIDTH) / 2.0).clamp(
        vf.origin.x + 4.0,
        vf.origin.x + vf.size.width - BAR_WIDTH - 4.0,
    );
    let below = cocoa.origin.y - margin - 10.0 - BAR_HEIGHT;
    let above = cocoa.origin.y + h + margin + 10.0;
    let by = if below >= vf.origin.y {
        below
    } else if above + BAR_HEIGHT <= vf.origin.y + vf.size.height {
        above
    } else {
        cocoa.origin.y + 12.0
    };
    let bar_frame = NSRect::new(CGPoint::new(bx, by), CGSize::new(BAR_WIDTH, BAR_HEIGHT));
    let bar = OverlayPanel::new(mtm, bar_frame);
    bar.setHasShadow(true);
    let view = ScrollBarView::alloc(mtm).set_ivars(BarIvars { on_action });
    let view: Retained<ScrollBarView> = unsafe {
        msg_send![super(view), initWithFrame: NSRect::new(CGPoint::new(0.0, 0.0), bar_frame.size)]
    };
    view.setWantsLayer(true);
    let root = view.layer().expect("Ebene");
    let bg = CALayer::new();
    bg.setFrame(CGRect::new(CGPoint::new(0.0, 0.0), bar_frame.size));
    bg.setBackgroundColor(Some(&CGColor::new_srgb(0.12, 0.12, 0.13, 0.94)));
    bg.setCornerRadius(10.0);
    root.addSublayer(&bg);

    let text = |frame: CGRect, s: &str, size: f64| {
        let l = CATextLayer::new();
        l.setFrame(frame);
        l.setFontSize(size);
        l.setForegroundColor(Some(&CGColor::new_srgb(1.0, 1.0, 1.0, 1.0)));
        l.setContentsScale(scale);
        let ns = NSString::from_str(s);
        unsafe { l.setString(Some(as_any(&ns))) };
        l
    };
    let status = text(
        CGRect::new(
            CGPoint::new(14.0, 13.0),
            CGSize::new(FINISH_X.0 - 20.0, 18.0),
        ),
        labels.0,
        13.0,
    );
    root.addSublayer(&status);

    let finish_bg = CALayer::new();
    finish_bg.setFrame(CGRect::new(
        CGPoint::new(FINISH_X.0, 8.0),
        CGSize::new(FINISH_X.1 - FINISH_X.0, 28.0),
    ));
    finish_bg.setBackgroundColor(Some(&CGColor::new_srgb(0.9, 0.22, 0.21, 1.0)));
    finish_bg.setCornerRadius(6.0);
    root.addSublayer(&finish_bg);
    let finish = text(
        CGRect::new(
            CGPoint::new(FINISH_X.0, 13.0),
            CGSize::new(FINISH_X.1 - FINISH_X.0, 18.0),
        ),
        labels.1,
        13.0,
    );
    finish.setAlignmentMode(unsafe { kCAAlignmentCenter });
    root.addSublayer(&finish);
    let cancel = text(
        CGRect::new(
            CGPoint::new(CANCEL_X.0, 13.0),
            CGSize::new(CANCEL_X.1 - CANCEL_X.0, 18.0),
        ),
        labels.2,
        13.0,
    );
    cancel.setAlignmentMode(unsafe { kCAAlignmentCenter });
    cancel.setForegroundColor(Some(&CGColor::new_srgb(0.8, 0.8, 0.8, 1.0)));
    root.addSublayer(&cancel);

    bar.setContentView(Some(&view));
    border.orderFrontRegardless();
    bar.orderFrontRegardless();
    Ok(ScrollUi {
        border,
        bar,
        status,
    })
}

// --- Hinweis ---------------------------------------------------------------

/// Wie lange der Hinweis sichtbar bleibt.
const TOAST_SECONDS: f64 = 1.6;

pub(crate) fn show_toast(text: &str) {
    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Some(screen) = NSScreen::mainScreen(mtm) else {
        return;
    };
    let ns_text = NSString::from_str(text);
    let font_size = 15.0;
    let width = (40.0 + 8.5 * ns_text.length() as f64).min(screen.frame().size.width - 80.0);
    let height = 40.0;
    let sf = screen.visibleFrame();
    let frame = NSRect::new(
        CGPoint::new(
            sf.origin.x + (sf.size.width - width) / 2.0,
            sf.origin.y + 90.0,
        ),
        CGSize::new(width, height),
    );
    let panel = OverlayPanel::new(mtm, frame);
    panel.setIgnoresMouseEvents(true);
    panel.setHasShadow(true);
    let view = NSView::initWithFrame(
        NSView::alloc(mtm),
        NSRect::new(CGPoint::new(0.0, 0.0), frame.size),
    );
    view.setWantsLayer(true);
    let root = view.layer().expect("Ebene nach setWantsLayer");
    let label = CATextLayer::new();
    label.setFrame(CGRect::new(CGPoint::new(0.0, 0.0), frame.size));
    label.setBackgroundColor(Some(&CGColor::new_srgb(0.1, 0.1, 0.1, 0.88)));
    label.setCornerRadius(height / 2.0);
    label.setForegroundColor(Some(&CGColor::new_srgb(1.0, 1.0, 1.0, 1.0)));
    // Text vertikal mittig: CATextLayer zeichnet oben – Zeile nach unten rücken.
    let text_layer = CATextLayer::new();
    text_layer.setFrame(CGRect::new(
        CGPoint::new(0.0, (height - font_size * 1.3) / 2.0),
        CGSize::new(width, font_size * 1.3),
    ));
    text_layer.setFontSize(font_size);
    text_layer.setAlignmentMode(unsafe { kCAAlignmentCenter });
    text_layer.setForegroundColor(Some(&CGColor::new_srgb(1.0, 1.0, 1.0, 1.0)));
    text_layer.setContentsScale(screen.backingScaleFactor());
    unsafe { text_layer.setString(Some(as_any(&ns_text))) };
    label.addSublayer(&text_layer);
    root.addSublayer(&label);
    panel.setContentView(Some(&view));
    panel.orderFrontRegardless();

    let bound = MainThreadBound::new(panel, mtm);
    let _ = DispatchQueue::main().after(
        dispatch2::DispatchTime::NOW.time((TOAST_SECONDS * 1e9) as i64),
        move || {
            if let Some(mtm) = MainThreadMarker::new() {
                bound.into_inner(mtm).orderOut(None);
            }
        },
    );
}

// --- Hilfsfunktionen -------------------------------------------------------

fn without_animation(f: impl FnOnce()) {
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    f();
    CATransaction::commit();
}

fn shape_layer(bounds: CGRect) -> Retained<CAShapeLayer> {
    let layer = CAShapeLayer::new();
    layer.setFrame(bounds);
    layer.setFillColor(None);
    layer
}

fn rect_path(r: CGRect) -> CFRetained<CGMutablePath> {
    let path = CGMutablePath::new();
    unsafe { CGMutablePath::add_rect(Some(&path), std::ptr::null(), r) };
    path
}

fn contains(r: CGRect, p: CGPoint) -> bool {
    p.x >= r.origin.x
        && p.y >= r.origin.y
        && p.x < r.origin.x + r.size.width
        && p.y < r.origin.y + r.size.height
}

/// `CGDirectDisplayID` eines `NSScreen` (über `NSScreenNumber`).
fn screen_number(screen: &NSScreen) -> Option<u32> {
    let desc = screen.deviceDescription();
    let key = NSString::from_str("NSScreenNumber");
    let value = desc.objectForKey(&key)?;
    let number = value.downcast::<NSNumber>().ok()?;
    Some(number.unsignedIntValue())
}

fn as_any<T: AsRef<AnyObject>>(obj: &T) -> &AnyObject {
    obj.as_ref()
}

fn set_contents(layer: &CALayer, image: &CGImage) {
    // SAFETY: CGImage ist ein gültiger Inhalt für `contents` (CFType ↔ id).
    let obj = unsafe { &*(image as *const CGImage as *const AnyObject) };
    unsafe { layer.setContents(Some(obj)) };
}

/// CGImage direkt auf den Pixeln von `image` – ohne Kopie. Das `Arc` bleibt
/// am Leben, bis Core Graphics das Bild freigibt.
fn cgimage_from_rgba(image: Arc<RgbaImage>) -> Option<CFRetained<CGImage>> {
    unsafe extern "C-unwind" fn release(info: *mut c_void, _data: NonNull<c_void>, _size: usize) {
        // SAFETY: `info` stammt aus `Arc::into_raw` unten.
        drop(unsafe { Arc::from_raw(info as *const RgbaImage) });
    }
    let (w, h) = (image.width() as usize, image.height() as usize);
    let data = image.data().as_ptr() as *const c_void;
    let len = image.data().len();
    let info = Arc::into_raw(image) as *mut c_void;
    // SAFETY: `data` bleibt gültig, solange `info` (das Arc) lebt.
    let provider = unsafe { CGDataProvider::with_data(info, data, len, Some(release)) }?;
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))?;
    let bitmap_info = CGBitmapInfo::from_bits_retain(
        CGImageAlphaInfo::NoneSkipLast.0 | CGImageByteOrderInfo::Order32Big.0,
    );
    unsafe {
        CGImage::new(
            w,
            h,
            8,
            32,
            w * 4,
            Some(&space),
            bitmap_info,
            Some(&provider),
            std::ptr::null(),
            false,
            CGColorRenderingIntent::RenderingIntentDefault,
        )
    }
}
