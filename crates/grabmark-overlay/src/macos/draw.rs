//! Zeichenfläche während der Video-Aufnahme: ein durchsichtiges Panel über
//! dem ganzen Monitor (unter Leiste und Webcam-Blase), links eine
//! Werkzeugleiste aus SF Symbols. Gezeichnet wird nicht hier – die App
//! liefert ein fertiges Bild ([`Surface::set_image`]), die Ebene skaliert es
//! auf den aufgenommenen Bereich.

use std::cell::{Cell, RefCell};
use std::ffi::c_void;
use std::ptr::NonNull;
use std::sync::Arc;

use grabmark_core::RgbaImage;
use objc2::rc::Retained;
use objc2::{DefinedClass, MainThreadMarker, MainThreadOnly, define_class, msg_send};
use objc2_app_kit::{
    NSCursor, NSEvent, NSEventModifierFlags, NSImage, NSResponder, NSScreen,
    NSScreenSaverWindowLevel, NSView,
};
use objc2_core_foundation::{CFRetained, CGPoint, CGRect, CGSize};
use objc2_core_graphics::{
    CGBitmapInfo, CGColor, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage,
    CGImageAlphaInfo, CGImageByteOrderInfo, kCGColorSpaceSRGB,
};
use objc2_foundation::{NSObject, NSRect, NSString};
use objc2_quartz_core::{CALayer, CATextLayer, kCAAlignmentRight, kCAGravityResize};

use super::{KEY_ESCAPE, OverlayPanel, as_any, screen_number, set_contents, without_animation};
use crate::{DrawEvent, DrawTool, OverlayError};

const BUTTON: f64 = 40.0;
const GAP: f64 = 6.0;
const PAD: f64 = 8.0;
const SWATCH: f64 = 22.0;
const ICON: f64 = 22.0;
/// Abstand der Leiste zum linken Rand.
const MARGIN: f64 = 12.0;

const ACCENT: (f64, f64, f64) = (0.9, 0.22, 0.21);

/// Was in der Leiste wo liegt (Punkte, Ursprung oben links im Monitor).
#[derive(Clone, Copy, PartialEq)]
enum Hit {
    Tool(usize),
    Color(usize),
    Exit,
}

struct Toolbar {
    root: Retained<CALayer>,
    frame: CGRect,
    tools: Vec<Retained<CALayer>>,
    colors: Vec<Retained<CALayer>>,
    /// Klickflächen relativ zur Leiste.
    hits: Vec<(CGRect, Hit)>,
}

struct DrawIvars {
    on_event: Box<dyn Fn(DrawEvent)>,
    keys: Vec<char>,
    active: Cell<bool>,
    /// Ziehen hat auf der Fläche begonnen (nicht in der Leiste).
    pressing: Cell<bool>,
    /// Bereich in Punkten der View und Pixel je Punkt.
    area: CGRect,
    scale: f64,
    toolbar: RefCell<Option<Toolbar>>,
}

define_class!(
    #[unsafe(super(NSView, NSResponder, NSObject))]
    #[thread_kind = MainThreadOnly]
    #[name = "GrabmarkDrawView"]
    #[ivars = DrawIvars]
    struct DrawView;

    impl DrawView {
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

        #[unsafe(method(resetCursorRects))]
        fn reset_cursor_rects(&self) {
            if self.ivars().active.get() {
                self.addCursorRect_cursor(self.bounds(), &NSCursor::crosshairCursor());
            }
        }

        #[unsafe(method(mouseDown:))]
        fn mouse_down(&self, event: &NSEvent) {
            let p = self.location(event);
            let iv = self.ivars();
            if let Some(hit) = self.toolbar_hit(p) {
                let e = match hit {
                    Hit::Tool(i) => DrawEvent::Tool(i),
                    Hit::Color(i) => DrawEvent::Color(i),
                    Hit::Exit => DrawEvent::Exit,
                };
                (iv.on_event)(e);
                return;
            }
            if self.toolbar_contains(p) {
                return;
            }
            iv.pressing.set(true);
            let (x, y) = self.to_pixels(p);
            (iv.on_event)(DrawEvent::Press { x, y });
        }

        #[unsafe(method(mouseDragged:))]
        fn mouse_dragged(&self, event: &NSEvent) {
            let iv = self.ivars();
            if !iv.pressing.get() {
                return;
            }
            let (x, y) = self.to_pixels(self.location(event));
            (iv.on_event)(DrawEvent::Drag { x, y, shift: shift(event) });
        }

        #[unsafe(method(mouseUp:))]
        fn mouse_up(&self, event: &NSEvent) {
            let iv = self.ivars();
            if !iv.pressing.replace(false) {
                return;
            }
            let (x, y) = self.to_pixels(self.location(event));
            (iv.on_event)(DrawEvent::Release { x, y, shift: shift(event) });
        }

        /// Rechtsklick beendet das Zeichnen (ausser mitten in einem Strich).
        #[unsafe(method(rightMouseDown:))]
        fn right_mouse_down(&self, _event: &NSEvent) {
            let iv = self.ivars();
            if !iv.pressing.get() {
                (iv.on_event)(DrawEvent::Exit);
            }
        }

        #[unsafe(method(keyDown:))]
        fn key_down(&self, event: &NSEvent) {
            let iv = self.ivars();
            if event.keyCode() == KEY_ESCAPE {
                (iv.on_event)(DrawEvent::Escape);
                return;
            }
            let Some(chars) = event.charactersIgnoringModifiers() else {
                return;
            };
            let chars = chars.to_string().to_lowercase();
            if let Some(i) = chars
                .chars()
                .next()
                .and_then(|c| iv.keys.iter().position(|k| *k == c))
            {
                (iv.on_event)(DrawEvent::Tool(i));
            }
        }
    }
);

fn shift(event: &NSEvent) -> bool {
    event.modifierFlags().contains(NSEventModifierFlags::Shift)
}

impl DrawView {
    fn location(&self, event: &NSEvent) -> CGPoint {
        self.convertPoint_fromView(event.locationInWindow(), None)
    }

    /// View-Punkte → Pixel im aufgenommenen Bereich.
    fn to_pixels(&self, p: CGPoint) -> (f32, f32) {
        let iv = self.ivars();
        (
            ((p.x - iv.area.origin.x) * iv.scale) as f32,
            ((p.y - iv.area.origin.y) * iv.scale) as f32,
        )
    }

    fn toolbar_contains(&self, p: CGPoint) -> bool {
        self.ivars()
            .toolbar
            .borrow()
            .as_ref()
            .is_some_and(|t| super::contains(t.frame, p))
    }

    fn toolbar_hit(&self, p: CGPoint) -> Option<Hit> {
        let toolbar = self.ivars().toolbar.borrow();
        let t = toolbar.as_ref()?;
        let local = CGPoint::new(p.x - t.frame.origin.x, p.y - t.frame.origin.y);
        t.hits
            .iter()
            .find(|(r, _)| super::contains(*r, local))
            .map(|(_, h)| *h)
    }
}

pub(crate) struct Surface {
    panel: Retained<OverlayPanel>,
    view: Retained<DrawView>,
    image: Retained<CALayer>,
}

impl Surface {
    pub fn set_active(&self, active: bool) {
        let iv = self.view.ivars();
        if iv.active.replace(active) == active {
            return;
        }
        self.panel.setIgnoresMouseEvents(!active);
        if let Some(t) = iv.toolbar.borrow().as_ref() {
            without_animation(|| t.root.setHidden(!active));
        }
        if active {
            self.panel.makeKeyWindow();
            self.panel.makeFirstResponder(Some(&self.view));
            NSCursor::crosshairCursor().set();
        } else {
            iv.pressing.set(false);
            // Schlüsselfenster abgeben: Tastendrücke gehen wieder an die App
            // darunter.
            self.panel.orderOut(None);
            self.panel.orderFrontRegardless();
            NSCursor::arrowCursor().set();
        }
        self.panel.invalidateCursorRectsForView(&self.view);
    }

    pub fn is_active(&self) -> bool {
        self.view.ivars().active.get()
    }

    pub fn set_image(&self, image: Option<Arc<RgbaImage>>) {
        let cg = image.and_then(cgimage_with_alpha);
        without_animation(|| match &cg {
            Some(cg) => set_contents(&self.image, cg),
            None => unsafe { self.image.setContents(None) },
        });
    }

    pub fn set_selected(&self, tool: usize, color: usize) {
        let toolbar = self.view.ivars().toolbar.borrow();
        let Some(t) = toolbar.as_ref() else { return };
        without_animation(|| {
            for (i, l) in t.tools.iter().enumerate() {
                let c = if i == tool {
                    CGColor::new_srgb(ACCENT.0, ACCENT.1, ACCENT.2, 1.0)
                } else {
                    CGColor::new_srgb(1.0, 1.0, 1.0, 0.0)
                };
                l.setBackgroundColor(Some(&c));
            }
            for (i, l) in t.colors.iter().enumerate() {
                l.setBorderWidth(if i == color { 2.5 } else { 0.0 });
            }
        });
    }

    pub fn close(&self) {
        self.panel.orderOut(None);
    }
}

pub(crate) fn open(
    display_id: u32,
    area: (u32, u32, u32, u32),
    tools: &[DrawTool<'_>],
    colors: &[[f64; 4]],
    on_event: Box<dyn Fn(DrawEvent)>,
) -> Result<Surface, OverlayError> {
    let mtm = MainThreadMarker::new().ok_or(OverlayError::NotMainThread)?;
    let screens = NSScreen::screens(mtm);
    let screen = screens
        .iter()
        .find(|s| screen_number(s) == Some(display_id))
        .or_else(|| {
            log::warn!("Zeichenfläche: Monitor {display_id} nicht gefunden – nehme den ersten");
            screens.firstObject()
        })
        .ok_or(OverlayError::NoScreens)?;
    let sf = screen.frame();
    let vf = screen.visibleFrame();
    let scale = screen.backingScaleFactor().max(1.0);
    let area_pt = CGRect::new(
        CGPoint::new(area.0 as f64 / scale, area.1 as f64 / scale),
        CGSize::new(area.2 as f64 / scale, area.3 as f64 / scale),
    );

    let panel = OverlayPanel::new(mtm, sf);
    // Unter Aufnahmeleiste und Webcam-Blase, damit sie bedienbar bleiben.
    panel.setLevel(NSScreenSaverWindowLevel - 1);
    panel.setIgnoresMouseEvents(true);
    panel.setHidesOnDeactivate(false);

    let view = DrawView::alloc(mtm).set_ivars(DrawIvars {
        on_event,
        keys: tools.iter().map(|t| t.key).collect(),
        active: Cell::new(false),
        pressing: Cell::new(false),
        area: area_pt,
        scale,
        toolbar: RefCell::new(None),
    });
    let view: Retained<DrawView> = unsafe {
        msg_send![super(view), initWithFrame: NSRect::new(CGPoint::new(0.0, 0.0), sf.size)]
    };
    view.setWantsLayer(true);
    let root = view.layer().expect("Ebene");

    let image = CALayer::new();
    image.setFrame(area_pt);
    image.setContentsGravity(unsafe { kCAGravityResize });
    root.addSublayer(&image);

    // Leiste links, senkrecht mittig im sichtbaren Bereich.
    let rows = tools.len() as f64;
    let height = PAD * 2.0
        + rows * BUTTON
        + (rows - 1.0).max(0.0) * GAP
        + GAP * 2.0
        + colors.len() as f64 * (SWATCH + GAP)
        + GAP
        + BUTTON;
    let width = BUTTON + PAD * 2.0;
    // Sichtbarer Bereich in View-Koordinaten (oben links).
    let visible_top = sf.origin.y + sf.size.height - (vf.origin.y + vf.size.height);
    let x = vf.origin.x - sf.origin.x + MARGIN;
    let y = visible_top + ((vf.size.height - height) / 2.0).max(0.0);
    let frame = CGRect::new(CGPoint::new(x, y), CGSize::new(width, height));
    let toolbar = build_toolbar(frame, tools, colors, scale);
    toolbar.root.setHidden(true);
    root.addSublayer(&toolbar.root);
    *view.ivars().toolbar.borrow_mut() = Some(toolbar);

    panel.setContentView(Some(&view));
    panel.orderFrontRegardless();
    Ok(Surface { panel, view, image })
}

fn build_toolbar(
    frame: CGRect,
    tools: &[DrawTool<'_>],
    colors: &[[f64; 4]],
    scale: f64,
) -> Toolbar {
    let root = CALayer::new();
    root.setFrame(frame);
    root.setBackgroundColor(Some(&CGColor::new_srgb(0.12, 0.12, 0.13, 0.94)));
    root.setCornerRadius(12.0);
    root.setShadowOpacity(0.35);
    root.setShadowRadius(10.0);

    let mut hits = Vec::new();
    let mut tool_layers = Vec::new();
    let mut y = PAD;
    for (i, tool) in tools.iter().enumerate() {
        let r = CGRect::new(CGPoint::new(PAD, y), CGSize::new(BUTTON, BUTTON));
        let button = CALayer::new();
        button.setFrame(r);
        button.setCornerRadius(8.0);
        root.addSublayer(&button);
        if let Some(icon) = symbol_layer(tool.symbol) {
            icon.setFrame(CGRect::new(
                CGPoint::new((BUTTON - ICON) / 2.0, (BUTTON - ICON) / 2.0),
                CGSize::new(ICON, ICON),
            ));
            button.addSublayer(&icon);
        }
        // Taste klein unten rechts.
        let key = CATextLayer::new();
        key.setFrame(CGRect::new(
            CGPoint::new(BUTTON - 13.0, BUTTON - 13.0),
            CGSize::new(10.0, 11.0),
        ));
        key.setFontSize(8.5);
        key.setContentsScale(scale);
        key.setAlignmentMode(unsafe { kCAAlignmentRight });
        key.setForegroundColor(Some(&CGColor::new_srgb(1.0, 1.0, 1.0, 0.55)));
        let s = NSString::from_str(&tool.key.to_uppercase().to_string());
        unsafe { key.setString(Some(as_any(&s))) };
        button.addSublayer(&key);
        hits.push((r, Hit::Tool(i)));
        tool_layers.push(button);
        y += BUTTON + GAP;
    }
    y += GAP;
    let mut color_layers = Vec::new();
    for (i, [r, g, b, a]) in colors.iter().enumerate() {
        let rect = CGRect::new(
            CGPoint::new(PAD + (BUTTON - SWATCH) / 2.0, y),
            CGSize::new(SWATCH, SWATCH),
        );
        let swatch = CALayer::new();
        swatch.setFrame(rect);
        swatch.setCornerRadius(SWATCH / 2.0);
        swatch.setBackgroundColor(Some(&CGColor::new_srgb(*r, *g, *b, *a)));
        swatch.setBorderColor(Some(&CGColor::new_srgb(1.0, 1.0, 1.0, 1.0)));
        root.addSublayer(&swatch);
        // Etwas grössere Klickfläche.
        let hit = CGRect::new(
            CGPoint::new(PAD, y - GAP / 2.0),
            CGSize::new(BUTTON, SWATCH + GAP),
        );
        hits.push((hit, Hit::Color(i)));
        color_layers.push(swatch);
        y += SWATCH + GAP;
    }
    y += GAP;
    let exit = CGRect::new(CGPoint::new(PAD, y), CGSize::new(BUTTON, BUTTON));
    let exit_layer = CALayer::new();
    exit_layer.setFrame(exit);
    exit_layer.setCornerRadius(8.0);
    exit_layer.setBackgroundColor(Some(&CGColor::new_srgb(1.0, 1.0, 1.0, 0.12)));
    if let Some(icon) = symbol_layer("checkmark") {
        icon.setFrame(CGRect::new(
            CGPoint::new((BUTTON - ICON) / 2.0, (BUTTON - ICON) / 2.0),
            CGSize::new(ICON, ICON),
        ));
        exit_layer.addSublayer(&icon);
    }
    root.addSublayer(&exit_layer);
    hits.push((exit, Hit::Exit));

    Toolbar {
        root,
        frame,
        tools: tool_layers,
        colors: color_layers,
        hits,
    }
}

/// Weisses SF Symbol: Fläche in Weiss, das Symbol als Maske.
fn symbol_layer(name: &str) -> Option<Retained<CALayer>> {
    let image = NSImage::imageWithSystemSymbolName_accessibilityDescription(
        &NSString::from_str(name),
        None,
    )?;
    let layer = CALayer::new();
    layer.setBackgroundColor(Some(&CGColor::new_srgb(1.0, 1.0, 1.0, 1.0)));
    let mask = CALayer::new();
    mask.setFrame(CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(ICON, ICON)));
    mask.setContentsGravity(unsafe { objc2_quartz_core::kCAGravityResizeAspect });
    unsafe { mask.setContents(Some(as_any(&image))) };
    // SAFETY: Die Maske ist eine eigene, neue Ebene.
    unsafe { layer.setMask(Some(&mask)) };
    Some(layer)
}

/// CGImage mit gerader Deckkraft direkt auf den Pixeln von `image`.
fn cgimage_with_alpha(image: Arc<RgbaImage>) -> Option<CFRetained<CGImage>> {
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
        CGImageAlphaInfo::Last.0 | CGImageByteOrderInfo::Order32Big.0,
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
