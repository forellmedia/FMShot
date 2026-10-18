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
use objc2::runtime::{AnyObject, ProtocolObject};
use objc2::{
    AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
};
use objc2_app_kit::{
    NSBackingStoreType, NSColor, NSCursor, NSDragOperation, NSDraggingContext, NSDraggingItem,
    NSDraggingSession, NSDraggingSource, NSEvent, NSImage, NSPanel, NSResponder, NSScreen,
    NSScreenSaverWindowLevel, NSTrackingArea, NSTrackingAreaOptions, NSView,
    NSWindowCollectionBehavior, NSWindowStyleMask,
};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapInfo, CGColor, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage,
    CGImageAlphaInfo, CGImageByteOrderInfo, CGMutablePath, kCGColorSpaceSRGB,
};
use objc2_foundation::{
    NSArray, NSNumber, NSObject, NSObjectProtocol, NSPoint, NSRect, NSString, NSURL,
};
use objc2_quartz_core::{
    CALayer, CAShapeLayer, CATextLayer, CATransaction, kCAAlignmentCenter, kCAFillRuleEvenOdd,
    kCAFilterNearest, kCAGravityResize,
};

use crate::{
    BubbleStyle, DoneCallback, OverlayError, OverlayMode, OverlayResult, PreviewAction,
    PreviewPlacement, RecordAction, RecordLabels, ScrollAction, Selection, WindowTarget,
    bubble_resize, points_to_pixels, rect_from_points, window_at,
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

// --- Aufnahmeleiste (Scroll, Video) ------------------------------------------

const BAR_HEIGHT: f64 = 44.0;
const BAR_STATUS_WIDTH: f64 = 176.0;
const BAR_BUTTON_WIDTH: f64 = 84.0;
const BAR_GAP: f64 = 6.0;

/// Rahmen um den Bereich (optional) und eine Leiste mit Status und Knöpfen.
struct Bar {
    border: Option<Retained<OverlayPanel>>,
    bar: Retained<OverlayPanel>,
    status: Retained<CATextLayer>,
    labels: Vec<Retained<CATextLayer>>,
}

impl Bar {
    fn set_status(&self, text: &str) {
        set_text(&self.status, text);
    }

    fn set_label(&self, index: usize, text: &str) {
        if let Some(l) = self.labels.get(index) {
            set_text(l, text);
        }
    }

    fn close(&self) {
        if let Some(border) = &self.border {
            border.orderOut(None);
        }
        self.bar.orderOut(None);
    }
}

fn set_text(layer: &CATextLayer, text: &str) {
    let s = NSString::from_str(text);
    without_animation(|| unsafe { layer.setString(Some(as_any(&s))) });
}

pub(crate) struct ScrollUi(Bar);

impl ScrollUi {
    pub fn set_status(&self, text: &str) {
        self.0.set_status(text);
    }

    pub fn close(&self) {
        self.0.close();
    }
}

pub(crate) struct RecordUi {
    bar: Bar,
    /// Aktion je Knopf, in der Reihenfolge der Leiste.
    actions: Vec<RecordAction>,
}

impl RecordUi {
    pub fn set_status(&self, text: &str) {
        self.bar.set_status(text);
    }

    pub fn set_label(&self, action: RecordAction, text: &str) {
        if let Some(i) = self.actions.iter().position(|a| *a == action) {
            self.bar.set_label(i, text);
        }
    }

    pub fn close(&self) {
        self.bar.close();
    }
}

struct BarIvars {
    /// x-Bereiche der Knöpfe (Punkte).
    buttons: Vec<(f64, f64)>,
    on_click: Box<dyn Fn(usize)>,
}

define_class!(
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FMShotScrollBarView"]
    #[ivars = BarIvars]
    struct BarView;

    impl BarView {
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
            let iv = self.ivars();
            if let Some(i) = iv.buttons.iter().position(|(a, b)| p.x >= *a && p.x <= *b) {
                (iv.on_click)(i);
            }
        }
    }
);

/// Knopf der Leiste: Text und ob er rot hervorgehoben ist.
type BarButton<'a> = (&'a str, bool);

/// Zeigt die Leiste. `rect` (physische Pixel des Monitors) bekommt einen
/// roten Rahmen, die Leiste sitzt darunter; ohne `rect` sitzt sie unten in
/// der Mitte des Monitors. Mausklicks gehen weiterhin an die Apps darunter.
fn open_bar(
    display_id: u32,
    rect: Option<(u32, u32, u32, u32)>,
    status: &str,
    buttons: &[BarButton<'_>],
    on_click: Box<dyn Fn(usize)>,
) -> Result<Bar, OverlayError> {
    let mtm = MainThreadMarker::new().ok_or(OverlayError::NotMainThread)?;
    let screens = NSScreen::screens(mtm);
    let screen = screens
        .iter()
        .find(|s| screen_number(s) == Some(display_id))
        .or_else(|| screens.firstObject())
        .ok_or(OverlayError::NoScreens)?;
    let sf = screen.frame();
    let vf = screen.visibleFrame();
    let scale = screen.backingScaleFactor().max(1.0);
    let margin = 3.0;

    // Pixel (oben links im Monitor) → Cocoa-Koordinaten (unten links global)
    let area = rect.map(|r| {
        let (x, y, w, h) = (
            r.0 as f64 / scale,
            r.1 as f64 / scale,
            r.2 as f64 / scale,
            r.3 as f64 / scale,
        );
        NSRect::new(
            CGPoint::new(sf.origin.x + x, sf.origin.y + sf.size.height - y - h),
            CGSize::new(w, h),
        )
    });

    // Rahmen: durchlässig für Maus und Scrollen.
    let border = area.map(|cocoa| {
        let border_frame = NSRect::new(
            CGPoint::new(cocoa.origin.x - margin, cocoa.origin.y - margin),
            CGSize::new(
                cocoa.size.width + 2.0 * margin,
                cocoa.size.height + 2.0 * margin,
            ),
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
        border
    });

    let bar_width = 14.0 + BAR_STATUS_WIDTH + buttons.len() as f64 * (BAR_BUTTON_WIDTH + BAR_GAP);
    // Unter dem Bereich, sonst darüber, sonst innen unten.
    let (bx, by) = match area {
        Some(cocoa) => {
            let bx = (cocoa.origin.x + (cocoa.size.width - bar_width) / 2.0).clamp(
                vf.origin.x + 4.0,
                vf.origin.x + vf.size.width - bar_width - 4.0,
            );
            let below = cocoa.origin.y - margin - 10.0 - BAR_HEIGHT;
            let above = cocoa.origin.y + cocoa.size.height + margin + 10.0;
            let by = if below >= vf.origin.y {
                below
            } else if above + BAR_HEIGHT <= vf.origin.y + vf.size.height {
                above
            } else {
                cocoa.origin.y + 12.0
            };
            (bx, by)
        }
        None => (
            vf.origin.x + (vf.size.width - bar_width) / 2.0,
            vf.origin.y + 24.0,
        ),
    };
    let bar_frame = NSRect::new(CGPoint::new(bx, by), CGSize::new(bar_width, BAR_HEIGHT));
    let bar = OverlayPanel::new(mtm, bar_frame);
    bar.setHasShadow(true);

    let ranges: Vec<(f64, f64)> = (0..buttons.len())
        .map(|i| {
            let x = 14.0 + BAR_STATUS_WIDTH + i as f64 * (BAR_BUTTON_WIDTH + BAR_GAP);
            (x, x + BAR_BUTTON_WIDTH)
        })
        .collect();
    let view = BarView::alloc(mtm).set_ivars(BarIvars {
        buttons: ranges.clone(),
        on_click,
    });
    let view: Retained<BarView> = unsafe {
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
    let status_layer = text(
        CGRect::new(
            CGPoint::new(14.0, 13.0),
            CGSize::new(BAR_STATUS_WIDTH - 6.0, 18.0),
        ),
        status,
        13.0,
    );
    root.addSublayer(&status_layer);

    let mut labels = Vec::new();
    for ((label, primary), (x0, x1)) in buttons.iter().zip(&ranges) {
        let button_bg = CALayer::new();
        button_bg.setFrame(CGRect::new(
            CGPoint::new(*x0, 8.0),
            CGSize::new(x1 - x0, 28.0),
        ));
        let color = if *primary {
            CGColor::new_srgb(0.9, 0.22, 0.21, 1.0)
        } else {
            CGColor::new_srgb(1.0, 1.0, 1.0, 0.12)
        };
        button_bg.setBackgroundColor(Some(&color));
        button_bg.setCornerRadius(6.0);
        root.addSublayer(&button_bg);
        let l = text(
            CGRect::new(CGPoint::new(*x0, 13.0), CGSize::new(x1 - x0, 18.0)),
            label,
            13.0,
        );
        l.setAlignmentMode(unsafe { kCAAlignmentCenter });
        root.addSublayer(&l);
        labels.push(l);
    }

    bar.setContentView(Some(&view));
    if let Some(border) = &border {
        border.orderFrontRegardless();
    }
    bar.orderFrontRegardless();
    Ok(Bar {
        border,
        bar,
        status: status_layer,
        labels,
    })
}

pub(crate) fn open_scroll_ui(
    display_id: u32,
    rect: (u32, u32, u32, u32),
    labels: (&str, &str, &str),
    on_action: Box<dyn Fn(ScrollAction)>,
) -> Result<ScrollUi, OverlayError> {
    open_bar(
        display_id,
        Some(rect),
        labels.0,
        &[(labels.1, true), (labels.2, false)],
        Box::new(move |i| {
            on_action(if i == 0 {
                ScrollAction::Finish
            } else {
                ScrollAction::Cancel
            })
        }),
    )
    .map(ScrollUi)
}

pub(crate) fn open_record_ui(
    display_id: u32,
    rect: Option<(u32, u32, u32, u32)>,
    labels: &RecordLabels<'_>,
    on_action: Box<dyn Fn(RecordAction)>,
) -> Result<RecordUi, OverlayError> {
    let mut buttons = vec![(labels.pause, false, RecordAction::Pause)];
    if let Some(webcam) = labels.webcam {
        buttons.push((webcam, false, RecordAction::Webcam));
        if let Some(large) = labels.webcam_large {
            buttons.push((large, false, RecordAction::WebcamLarge));
        }
    }
    buttons.push((labels.stop, true, RecordAction::Stop));
    buttons.push((labels.cancel, false, RecordAction::Cancel));
    let actions: Vec<RecordAction> = buttons.iter().map(|b| b.2).collect();
    let bar_buttons: Vec<BarButton<'_>> = buttons.iter().map(|b| (b.0, b.1)).collect();
    let click = actions.clone();
    let bar = open_bar(
        display_id,
        rect,
        labels.status,
        &bar_buttons,
        Box::new(move |i| {
            if let Some(action) = click.get(i) {
                on_action(*action);
            }
        }),
    )?;
    Ok(RecordUi { bar, actions })
}

// --- Webcam-Blase ------------------------------------------------------------

/// Kleinste und grösste Breite der Blase in Punkten.
const BUBBLE_MIN: f64 = 80.0;
const BUBBLE_MAX: f64 = 640.0;

struct BubbleIvars {
    /// Schneidet das Kamerabild in Form und trägt den Rahmen.
    container: Retained<CALayer>,
    preview: Retained<CALayer>,
    style: BubbleStyle,
}

define_class!(
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FMShotBubbleView"]
    #[ivars = BubbleIvars]
    struct BubbleView;

    impl BubbleView {
        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            if let Some(window) = self.window() {
                window.performWindowDragWithEvent(event);
            }
        }

        #[unsafe(method(scrollWheel:))]
        fn scroll_wheel(&self, event: &NSEvent) {
            let mut delta = event.scrollingDeltaY();
            if !event.hasPreciseScrollingDeltas() {
                // Mausrad: wenige, grobe Schritte
                delta *= 4.0;
            }
            if delta.abs() < f64::EPSILON {
                return;
            }
            let Some(window) = self.window() else {
                return;
            };
            let f = window.frame();
            let (w, h) = bubble_resize(
                (f.size.width, f.size.height),
                delta,
                BUBBLE_MIN,
                BUBBLE_MAX,
            );
            let frame = NSRect::new(
                CGPoint::new(
                    f.origin.x + (f.size.width - w) / 2.0,
                    f.origin.y + (f.size.height - h) / 2.0,
                ),
                CGSize::new(w, h),
            );
            window.setFrame_display(frame, true);
            self.layout_layers(frame.size);
            window.invalidateShadow();
        }

        #[unsafe(method(resetCursorRects))]
        fn reset_cursor_rects(&self) {
            self.addCursorRect_cursor(self.bounds(), &NSCursor::openHandCursor());
        }
    }
);

impl BubbleView {
    /// Form, Rahmen und Kamerabild an die Grösse anpassen.
    fn layout_layers(&self, size: CGSize) {
        let iv = self.ivars();
        let bounds = CGRect::new(CGPoint::new(0.0, 0.0), size);
        let short = size.width.min(size.height);
        without_animation(|| {
            iv.container.setFrame(bounds);
            iv.container
                .setCornerRadius(iv.style.corner_radius.clamp(0.0, 0.5) * short);
            match iv.style.border {
                Some((width, [r, g, b, a])) => {
                    iv.container.setBorderWidth((width * short).max(1.0));
                    iv.container
                        .setBorderColor(Some(&CGColor::new_srgb(r, g, b, a)));
                }
                None => iv.container.setBorderWidth(0.0),
            }
            iv.preview.setFrame(bounds);
        });
    }
}

pub(crate) struct Bubble {
    panel: Retained<OverlayPanel>,
    preview: Retained<CALayer>,
    /// Monitor beim Öffnen (Cocoa-Koordinaten) und sein Massstab.
    screen: NSRect,
    scale: f64,
}

impl Bubble {
    pub fn frame(&self) -> (f64, f64, f64, f64) {
        let f = self.panel.frame();
        let sf = self.screen;
        let s = self.scale;
        (
            (f.origin.x - sf.origin.x) * s,
            (sf.origin.y + sf.size.height - f.origin.y - f.size.height) * s,
            f.size.width * s,
            f.size.height * s,
        )
    }

    pub fn set_visible(&self, visible: bool) {
        if visible {
            self.panel.orderFrontRegardless();
        } else {
            self.panel.orderOut(None);
        }
    }

    pub fn close(&self) {
        self.panel.orderOut(None);
        self.preview.removeFromSuperlayer();
    }
}

/// # Safety
/// `layer` zeigt auf einen gültigen `CALayer`.
pub(crate) unsafe fn open_bubble(
    display_id: u32,
    rect: (f64, f64, f64, f64),
    style: BubbleStyle,
    layer: *mut c_void,
) -> Result<Bubble, OverlayError> {
    let mtm = MainThreadMarker::new().ok_or(OverlayError::NotMainThread)?;
    // SAFETY: vom Aufrufer zugesichert; wir halten eine eigene Referenz.
    let preview = unsafe { Retained::retain(layer as *mut CALayer) }
        .ok_or_else(|| OverlayError::Backend("Kein Kamerabild".into()))?;
    let screens = NSScreen::screens(mtm);
    let screen = screens
        .iter()
        .find(|s| screen_number(s) == Some(display_id))
        .or_else(|| screens.firstObject())
        .ok_or(OverlayError::NoScreens)?;
    let sf = screen.frame();
    let scale = screen.backingScaleFactor().max(1.0);
    // Grösse begrenzen, Seitenverhältnis und Mittelpunkt behalten.
    let (w0, h0) = ((rect.2 / scale).max(1.0), (rect.3 / scale).max(1.0));
    let w = w0.clamp(BUBBLE_MIN, BUBBLE_MAX);
    let h = h0 * w / w0;
    let x = rect.0 / scale + (w0 - w) / 2.0;
    let y = rect.1 / scale + (h0 - h) / 2.0;
    let frame = NSRect::new(
        CGPoint::new(sf.origin.x + x, sf.origin.y + sf.size.height - y - h),
        CGSize::new(w, h),
    );
    let panel = OverlayPanel::new(mtm, frame);
    panel.setHasShadow(style.shadow);
    panel.setHidesOnDeactivate(false);

    let container = CALayer::new();
    container.setMasksToBounds(true);
    container.setBackgroundColor(Some(&CGColor::new_srgb(0.1, 0.1, 0.11, 1.0)));
    container.addSublayer(&preview);
    let view = BubbleView::alloc(mtm).set_ivars(BubbleIvars {
        container: container.clone(),
        preview: preview.clone(),
        style,
    });
    let view: Retained<BubbleView> = unsafe {
        msg_send![super(view), initWithFrame: NSRect::new(CGPoint::new(0.0, 0.0), frame.size)]
    };
    view.setWantsLayer(true);
    view.layer().expect("Ebene").addSublayer(&container);
    view.layout_layers(frame.size);
    panel.setContentView(Some(&view));
    panel.orderFrontRegardless();
    panel.invalidateShadow();
    Ok(Bubble {
        panel,
        preview,
        screen: sf,
        scale,
    })
}

// --- Schnellzugriff-Vorschau ---------------------------------------------------

const PREVIEW_W: f64 = 252.0;
const PREVIEW_IMG_MAX_H: f64 = 170.0;
const PREVIEW_PAD: f64 = 8.0;
const PREVIEW_BAR_H: f64 = 34.0;
/// Ohne Maus darüber verschwindet die Vorschau nach dieser Zeit.
const PREVIEW_SECONDS: f64 = 6.0;

struct PreviewIvars {
    on_action: Box<dyn Fn(PreviewAction)>,
    file: String,
    image_rect: CGRect,
    image: CFRetained<CGImage>,
    /// Mauszeiger über der Vorschau → nicht automatisch schliessen.
    hovered: RefCell<bool>,
    /// Zieh-Start (zum Unterscheiden von Klick und Ziehen).
    down: RefCell<Option<NSPoint>>,
    dragging: RefCell<bool>,
    closed: RefCell<bool>,
}

define_class!(
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "FMShotPreviewView"]
    #[ivars = PreviewIvars]
    struct PreviewView;

    impl PreviewView {
        #[unsafe(method(isFlipped))]
        fn is_flipped(&self) -> bool {
            true
        }

        #[unsafe(method(acceptsFirstMouse:))]
        fn accepts_first_mouse(&self, _event: Option<&NSEvent>) -> bool {
            true
        }

        #[unsafe(method(mouseEntered:))]
        fn mouse_entered(&self, _event: &NSEvent) {
            *self.ivars().hovered.borrow_mut() = true;
        }

        #[unsafe(method(mouseExited:))]
        fn mouse_exited(&self, _event: &NSEvent) {
            *self.ivars().hovered.borrow_mut() = false;
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let p = self.convertPoint_fromView(event.locationInWindow(), None);
            *self.ivars().down.borrow_mut() = Some(p);
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let iv = self.ivars();
            let Some(start) = *iv.down.borrow() else { return };
            if *iv.dragging.borrow() || !contains(iv.image_rect, start) {
                return;
            }
            let p = self.convertPoint_fromView(event.locationInWindow(), None);
            if (p.x - start.x).hypot(p.y - start.y) < 4.0 {
                return;
            }
            *iv.dragging.borrow_mut() = true;
            self.start_drag(event);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let iv = self.ivars();
            let start = iv.down.borrow_mut().take();
            if *iv.dragging.borrow() {
                return;
            }
            let p = self.convertPoint_fromView(event.locationInWindow(), None);
            let Some(start) = start else { return };
            if (p.x - start.x).hypot(p.y - start.y) > 4.0 {
                return;
            }
            if let Some(action) = preview_hit(iv.image_rect, p) {
                if action != PreviewAction::Copy && action != PreviewAction::Text {
                    *iv.closed.borrow_mut() = true;
                }
                (iv.on_action)(action);
                if *iv.closed.borrow()
                    && let Some(w) = self.window() {
                        w.orderOut(None);
                    }
            }
        }
    }

    unsafe impl NSObjectProtocol for PreviewView {}

    unsafe impl NSDraggingSource for PreviewView {
        #[unsafe(method(draggingSession:sourceOperationMaskForDraggingContext:))]
        fn source_operation_mask(
            &self,
            _session: &NSDraggingSession,
            _context: NSDraggingContext,
        ) -> NSDragOperation {
            NSDragOperation::Copy
        }

        #[unsafe(method(draggingSession:endedAtPoint:operation:))]
        fn ended(&self, _session: &NSDraggingSession, _point: NSPoint, operation: NSDragOperation) {
            let iv = self.ivars();
            *iv.dragging.borrow_mut() = false;
            if operation != NSDragOperation::None {
                *iv.closed.borrow_mut() = true;
                (iv.on_action)(PreviewAction::Dragged);
                if let Some(w) = self.window() {
                    w.orderOut(None);
                }
            }
        }
    }
);

impl PreviewView {
    fn start_drag(&self, event: &NSEvent) {
        let Some(mtm) = MainThreadMarker::new() else {
            return;
        };
        let iv = self.ivars();
        let url = NSURL::fileURLWithPath(&NSString::from_str(&iv.file));
        let writer = ProtocolObject::from_ref(&*url);
        let item = NSDraggingItem::initWithPasteboardWriter(NSDraggingItem::alloc(), writer);
        let r = iv.image_rect;
        let ns_image = NSImage::initWithCGImage_size(NSImage::alloc(), &iv.image, r.size);
        unsafe { item.setDraggingFrame_contents(r, Some(as_any(&ns_image))) };
        let _ = mtm;
        let source = ProtocolObject::from_ref(self);
        self.beginDraggingSessionWithItems_event_source(
            &NSArray::from_retained_slice(&[item]),
            event,
            source,
        );
    }
}

/// Knopf unter dem Punkt (Ansichtskoordinaten, oben links).
fn preview_hit(image_rect: CGRect, p: NSPoint) -> Option<PreviewAction> {
    // ✕ oben rechts auf dem Bild
    let close = CGRect::new(
        CGPoint::new(
            image_rect.origin.x + image_rect.size.width - 26.0,
            image_rect.origin.y + 4.0,
        ),
        CGSize::new(22.0, 22.0),
    );
    if contains(close, p) {
        return Some(PreviewAction::Close);
    }
    if contains(image_rect, p) {
        return Some(PreviewAction::Edit);
    }
    let bar_y = image_rect.origin.y + image_rect.size.height + 4.0;
    if p.y < bar_y || p.y > bar_y + PREVIEW_BAR_H {
        return None;
    }
    let w = (PREVIEW_W - 2.0 * PREVIEW_PAD) / 4.0;
    let i = ((p.x - PREVIEW_PAD) / w).floor() as i32;
    [
        PreviewAction::Copy,
        PreviewAction::Save,
        PreviewAction::Edit,
        PreviewAction::Text,
    ]
    .get(i as usize)
    .copied()
}

thread_local! {
    /// Offene Vorschauen (zum Stapeln in der Ecke).
    static PREVIEWS: RefCell<Vec<Retained<OverlayPanel>>> = const { RefCell::new(Vec::new()) };
}

pub(crate) fn show_preview(
    image: Arc<RgbaImage>,
    file: &str,
    placement: PreviewPlacement,
    labels: [&str; 4],
    on_action: Box<dyn Fn(PreviewAction)>,
) -> Result<(), OverlayError> {
    let mtm = MainThreadMarker::new().ok_or(OverlayError::NotMainThread)?;
    let screens = NSScreen::screens(mtm);
    let main = screens.firstObject().ok_or(OverlayError::NoScreens)?;
    let main_h = main.frame().size.height;

    let (iw, ih) = (image.width() as f64, image.height() as f64);
    let max_w = PREVIEW_W - 2.0 * PREVIEW_PAD;
    let scale = (max_w / iw).min(PREVIEW_IMG_MAX_H / ih).min(1.0);
    let (dw, dh) = ((iw * scale).max(40.0), (ih * scale).max(30.0));
    let height = PREVIEW_PAD + dh + 4.0 + PREVIEW_BAR_H + PREVIEW_PAD;
    let image_rect = CGRect::new(
        CGPoint::new((PREVIEW_W - dw) / 2.0, PREVIEW_PAD),
        CGSize::new(dw, dh),
    );

    // Position in Cocoa-Koordinaten (unten links).
    let origin = match placement {
        PreviewPlacement::At(x, y) => {
            let cocoa = CGPoint::new(x, main_h - y);
            let screen = screens
                .iter()
                .find(|s| contains(s.frame(), cocoa))
                .unwrap_or_else(|| main.clone());
            let vf = screen.visibleFrame();
            // Rechts unterhalb des Punkts, ganz auf dem Monitor.
            let px = (cocoa.x + 16.0).clamp(
                vf.origin.x + 8.0,
                vf.origin.x + vf.size.width - PREVIEW_W - 8.0,
            );
            let py = (cocoa.y - 16.0 - height).clamp(
                vf.origin.y + 8.0,
                vf.origin.y + vf.size.height - height - 8.0,
            );
            CGPoint::new(px, py)
        }
        PreviewPlacement::Corner => {
            let vf = main.visibleFrame();
            let stacked: f64 =
                PREVIEWS.with(|p| p.borrow().iter().map(|w| w.frame().size.height + 8.0).sum());
            CGPoint::new(
                vf.origin.x + vf.size.width - PREVIEW_W - 16.0,
                vf.origin.y + 16.0 + stacked,
            )
        }
    };
    let frame = NSRect::new(origin, CGSize::new(PREVIEW_W, height));
    let panel = OverlayPanel::new(mtm, frame);
    panel.setHasShadow(true);
    // Nicht über allem wie das Overlay, aber über normalen Fenstern.
    panel.setLevel(objc2_app_kit::NSFloatingWindowLevel);

    let cg = cgimage_from_rgba(image).ok_or(OverlayError::NoScreens)?;
    let view = PreviewView::alloc(mtm).set_ivars(PreviewIvars {
        on_action,
        file: file.to_string(),
        image_rect,
        image: cg.clone(),
        hovered: RefCell::new(false),
        down: RefCell::new(None),
        dragging: RefCell::new(false),
        closed: RefCell::new(false),
    });
    let bounds = NSRect::new(CGPoint::new(0.0, 0.0), frame.size);
    let view: Retained<PreviewView> = unsafe { msg_send![super(view), initWithFrame: bounds] };
    view.setWantsLayer(true);
    let root = view.layer().expect("Ebene");

    let bg = CALayer::new();
    bg.setFrame(bounds);
    bg.setBackgroundColor(Some(&CGColor::new_srgb(0.12, 0.12, 0.13, 0.95)));
    bg.setCornerRadius(12.0);
    root.addSublayer(&bg);

    let img = CALayer::new();
    img.setFrame(image_rect);
    img.setContentsGravity(unsafe { kCAGravityResize });
    img.setCornerRadius(6.0);
    img.setMasksToBounds(true);
    set_contents(&img, &cg);
    root.addSublayer(&img);

    // ✕
    let close_bg = CALayer::new();
    close_bg.setFrame(CGRect::new(
        CGPoint::new(
            image_rect.origin.x + image_rect.size.width - 26.0,
            image_rect.origin.y + 4.0,
        ),
        CGSize::new(22.0, 22.0),
    ));
    close_bg.setBackgroundColor(Some(&CGColor::new_srgb(0.0, 0.0, 0.0, 0.6)));
    close_bg.setCornerRadius(11.0);
    root.addSublayer(&close_bg);
    let scale_f = main.backingScaleFactor();
    let text_layer = |frame: CGRect, s: &str, size: f64| {
        let l = CATextLayer::new();
        l.setFrame(frame);
        l.setFontSize(size);
        l.setAlignmentMode(unsafe { kCAAlignmentCenter });
        l.setForegroundColor(Some(&CGColor::new_srgb(1.0, 1.0, 1.0, 1.0)));
        l.setContentsScale(scale_f);
        let ns = NSString::from_str(s);
        unsafe { l.setString(Some(as_any(&ns))) };
        l
    };
    root.addSublayer(&text_layer(
        CGRect::new(
            CGPoint::new(close_bg.frame().origin.x, close_bg.frame().origin.y + 3.0),
            CGSize::new(22.0, 16.0),
        ),
        "✕",
        12.0,
    ));

    // Knopfleiste
    let bar_y = image_rect.origin.y + image_rect.size.height + 4.0;
    let w = (PREVIEW_W - 2.0 * PREVIEW_PAD) / 4.0;
    for (i, label) in labels.iter().enumerate() {
        let x = PREVIEW_PAD + i as f64 * w;
        let btn = CALayer::new();
        btn.setFrame(CGRect::new(
            CGPoint::new(x + 2.0, bar_y + 3.0),
            CGSize::new(w - 4.0, PREVIEW_BAR_H - 6.0),
        ));
        btn.setBackgroundColor(Some(&CGColor::new_srgb(1.0, 1.0, 1.0, 0.1)));
        btn.setCornerRadius(6.0);
        root.addSublayer(&btn);
        root.addSublayer(&text_layer(
            CGRect::new(CGPoint::new(x, bar_y + 10.0), CGSize::new(w, 16.0)),
            label,
            11.5,
        ));
    }

    panel.setContentView(Some(&view));
    let tracking = unsafe {
        NSTrackingArea::initWithRect_options_owner_userInfo(
            NSTrackingArea::alloc(),
            bounds,
            NSTrackingAreaOptions::MouseEnteredAndExited
                | NSTrackingAreaOptions::ActiveAlways
                | NSTrackingAreaOptions::InVisibleRect,
            Some(&view),
            None,
        )
    };
    view.addTrackingArea(&tracking);
    panel.orderFrontRegardless();
    PREVIEWS.with(|p| p.borrow_mut().push(panel.clone()));
    schedule_preview_close(MainThreadBound::new((panel, view), mtm));
    Ok(())
}

/// Schliesst die Vorschau nach Ablauf – verschiebt, solange die Maus darüber ist.
fn schedule_preview_close(bound: MainThreadBound<(Retained<OverlayPanel>, Retained<PreviewView>)>) {
    let _ = DispatchQueue::main().after(
        dispatch2::DispatchTime::NOW.time((PREVIEW_SECONDS * 1e9) as i64),
        move || {
            let Some(mtm) = MainThreadMarker::new() else {
                return;
            };
            let (panel, view) = bound.into_inner(mtm);
            let iv = view.ivars();
            if !*iv.closed.borrow() && (*iv.hovered.borrow() || *iv.dragging.borrow()) {
                schedule_preview_close(MainThreadBound::new((panel, view), mtm));
                return;
            }
            panel.orderOut(None);
            PREVIEWS.with(|p| p.borrow_mut().retain(|w| !std::ptr::eq(&**w, &*panel)));
        },
    );
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
