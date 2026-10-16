//! macOS-Overlay: pro Monitor ein randloses `NSPanel` über allem (auch über
//! Menüleiste und Vollbild-Spaces). Das Panel aktiviert die App nicht – der
//! Fokus der vorherigen App bleibt erhalten.
//!
//! Gezeichnet wird ausschliesslich mit Core-Animation-Ebenen: Das eingefrorene
//! Bild liegt einmal als Textur auf der GPU, beim Ziehen ändern sich nur Pfade
//! und Positionen. Kein Neuzeichnen von Pixeln pro Mausbewegung.

use std::cell::RefCell;
use std::ffi::c_void;
use std::ptr::NonNull;
use std::rc::Rc;
use std::sync::Arc;

use dispatch2::{DispatchQueue, MainThreadBound};
use fmshot_core::RgbaImage;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2::{
    AllocAnyThread, DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send,
};
use objc2_app_kit::{
    NSBackingStoreType, NSCursor, NSEvent, NSPanel, NSResponder, NSScreen,
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
    DoneCallback, OverlayError, OverlayResult, OverlayScreen, Selection, points_to_pixels,
    rect_from_points,
};

/// Escape-Taste (virtueller Tastencode).
const KEY_ESCAPE: u16 = 53;
/// Kleinere Auswahl gilt als Klick ohne Ziehen.
const MIN_DRAG_POINTS: f64 = 3.0;
const MAGNIFIER_SIZE: f64 = 128.0;
const MAGNIFIER_ZOOM: f64 = 8.0;
const DIM_ALPHA: f64 = 0.35;

pub(crate) fn select_region(
    screens: Vec<OverlayScreen>,
    on_done: DoneCallback,
) -> Result<(), OverlayError> {
    let mtm = MainThreadMarker::new().ok_or(OverlayError::NotMainThread)?;
    let session = Rc::new(RefCell::new(Session {
        screens: Vec::new(),
        on_done: Some(on_done),
        drag: None,
    }));

    let ns_screens = NSScreen::screens(mtm);
    for (index, screen) in screens.into_iter().enumerate() {
        let Some(ns_screen) = ns_screens
            .iter()
            .find(|s| screen_number(s) == Some(screen.display.id))
        else {
            continue;
        };
        let state = ScreenState::new(mtm, &ns_screen, screen, index, session.clone());
        session.borrow_mut().screens.push(state);
    }
    if session.borrow().screens.is_empty() {
        return Err(OverlayError::NoScreens);
    }

    // Erst alles zeigen, dann das Panel unter der Maus zum Schlüsselfenster
    // machen – dort landen Tastendrücke (Esc).
    let mouse = NSEvent::mouseLocation();
    let s = session.borrow();
    for st in &s.screens {
        st.panel.orderFrontRegardless();
    }
    let key = s
        .screens
        .iter()
        .find(|st| contains(st.panel.frame(), mouse))
        .or(s.screens.first());
    if let Some(st) = key {
        st.panel.makeKeyWindow();
        st.panel.makeFirstResponder(Some(&st.view));
        st.update_magnifier(st.view.convert_from_screen(mouse));
    }
    NSCursor::crosshairCursor().set();
    Ok(())
}

struct Session {
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
    image_size: (u32, u32),
    points_size: (f64, f64),
    panel: Retained<OverlayPanel>,
    view: Retained<OverlayView>,
    dim: Retained<CAShapeLayer>,
    border: Retained<CAShapeLayer>,
    label: Retained<CATextLayer>,
    magnifier: Retained<CALayer>,
    magnifier_image: Retained<CALayer>,
}

impl ScreenState {
    fn new(
        mtm: MainThreadMarker,
        ns_screen: &NSScreen,
        screen: OverlayScreen,
        index: usize,
        session: Rc<RefCell<Session>>,
    ) -> Self {
        let frame = ns_screen.frame();
        let scale = ns_screen.backingScaleFactor();
        let bounds = NSRect::new(CGPoint::new(0.0, 0.0), frame.size);
        let image_size = (screen.image.width(), screen.image.height());
        let cg_image = cgimage_from_rgba(screen.image.clone());

        let panel = OverlayPanel::new(mtm, frame);
        let view = OverlayView::new(mtm, bounds, ViewIvars { session, index });
        view.setWantsLayer(true);
        let root = view.layer().expect("Ebene nach setWantsLayer");

        // Eingefrorenes Bild
        let image_layer = CALayer::new();
        image_layer.setFrame(bounds);
        set_image(&image_layer, cg_image.as_deref());
        image_layer.setContentsGravity(unsafe { kCAGravityResize });
        root.addSublayer(&image_layer);

        // Abdunklung mit Loch für die Auswahl
        let dim = CAShapeLayer::new();
        dim.setFrame(bounds);
        dim.setFillRule(unsafe { kCAFillRuleEvenOdd });
        dim.setFillColor(Some(&CGColor::new_srgb(0.0, 0.0, 0.0, DIM_ALPHA)));
        root.addSublayer(&dim);

        // Auswahlrahmen
        let border = CAShapeLayer::new();
        border.setFrame(bounds);
        border.setFillColor(None);
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
        let magnifier_image = CALayer::new();
        set_image(&magnifier_image, cg_image.as_deref());
        magnifier_image.setContentsGravity(unsafe { kCAGravityResize });
        magnifier_image.setMagnificationFilter(unsafe { kCAFilterNearest });
        magnifier_image.setFrame(CGRect::new(
            CGPoint::new(0.0, 0.0),
            CGSize::new(
                bounds.size.width * MAGNIFIER_ZOOM,
                bounds.size.height * MAGNIFIER_ZOOM,
            ),
        ));
        magnifier.addSublayer(&magnifier_image);
        // Fadenkreuz-Pixel in der Mitte der Lupe
        let center = CAShapeLayer::new();
        let px = MAGNIFIER_ZOOM / scale.max(1.0);
        let path = CGMutablePath::new();
        unsafe {
            CGMutablePath::add_rect(
                Some(&path),
                std::ptr::null(),
                CGRect::new(
                    CGPoint::new((MAGNIFIER_SIZE - px) / 2.0, (MAGNIFIER_SIZE - px) / 2.0),
                    CGSize::new(px, px),
                ),
            );
        }
        center.setPath(Some(&path));
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
            display_id: screen.display.id,
            image_size,
            points_size: (bounds.size.width, bounds.size.height),
            panel,
            view,
            dim,
            border,
            label,
            magnifier,
            magnifier_image,
        };
        state.update_selection(None);
        state
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
                Some((x, y, w, h)) => {
                    let sel = CGRect::new(CGPoint::new(x, y), CGSize::new(w, h));
                    unsafe { CGMutablePath::add_rect(Some(&dim_path), std::ptr::null(), sel) };
                    let border_path = CGMutablePath::new();
                    // Halber Punkt Versatz: 1-Punkt-Linie liegt scharf auf dem Pixelraster.
                    unsafe {
                        CGMutablePath::add_rect(
                            Some(&border_path),
                            std::ptr::null(),
                            CGRect::new(
                                CGPoint::new(x + 0.5, y + 0.5),
                                CGSize::new(w - 1.0, h - 1.0),
                            ),
                        )
                    };
                    self.border.setPath(Some(&border_path));

                    let (_, _, pw, ph) =
                        points_to_pixels(rect.unwrap(), self.points_size, self.image_size);
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
    fn update_magnifier(&self, p: (f64, f64)) {
        without_animation(|| {
            let (w, h) = self.points_size;
            let inside = p.0 >= 0.0 && p.1 >= 0.0 && p.0 <= w && p.1 <= h;
            self.magnifier.setHidden(!inside);
            if !inside {
                return;
            }
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
        panel.setOpaque(true);
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
            if let Some(st) = ivars.session.borrow().screens.get(ivars.index) {
                st.magnifier.setHidden(true);
            }
        }

        #[unsafe(method(mouseMoved:))]
        fn mouse_moved(&self, event: &NSEvent) {
            let p = self.location(event);
            let ivars = self.ivars();
            if let Some(st) = ivars.session.borrow().screens.get(ivars.index) {
                st.update_magnifier(p);
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
            // Ziehen bleibt auf dem Monitor, auf dem es begonnen hat.
            let Some(drag) = s.drag.as_ref().filter(|d| d.screen == ivars.index) else {
                return;
            };
            let Some(st) = s.screens.get(ivars.index) else {
                return;
            };
            let p = st.clamp(self.location(event));
            st.update_selection(Some(rect_from_points(drag.start, p)));
            st.update_magnifier(p);
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let p = self.location(event);
            let ivars = self.ivars();
            with_session(&ivars.session, |s| {
                let drag = s.drag.take()?;
                let st = s.screens.get(drag.screen)?;
                let rect = rect_from_points(drag.start, st.clamp(p));
                if rect.2 < MIN_DRAG_POINTS || rect.3 < MIN_DRAG_POINTS {
                    // Nur geklickt – Overlay bleibt offen.
                    st.update_selection(None);
                    return None;
                }
                let (x, y, width, height) = points_to_pixels(rect, st.points_size, st.image_size);
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

// --- Hilfsfunktionen -------------------------------------------------------

fn without_animation(f: impl FnOnce()) {
    CATransaction::begin();
    CATransaction::setDisableActions(true);
    f();
    CATransaction::commit();
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

fn set_image(layer: &CALayer, image: Option<&CGImage>) {
    if let Some(image) = image {
        // SAFETY: CGImage ist ein gültiger Inhalt für `contents` (CFType ↔ id).
        let obj = unsafe { &*(image as *const CGImage as *const AnyObject) };
        unsafe { layer.setContents(Some(obj)) };
    }
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
