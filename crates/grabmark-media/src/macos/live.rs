//! Webcam während der Aufnahme (Etappe 3): Gesichter erkennen (~10 je
//! Sekunde, für `<id>_faces.json`) und – bei Kopfausschnitt oder
//! Hintergrund unscharf/entfernt – das Bild der Live-Blase selbst rechnen.
//!
//! Alles läuft in einer eigenen Warteschlange. Ist sie noch beschäftigt,
//! wird das Bild für Erkennung und Blase ausgelassen; die Webcam-Spur selbst
//! bekommt trotzdem jedes Bild.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex, MutexGuard};

use dispatch2::{DispatchQueue, DispatchRetained};
use grabmark_core::{FaceSample, Rect, Size, WebcamBackground, WebcamFraming, WebcamLayout};
use grabmark_video::{LiveHead, head_crop};
use objc2::rc::{Retained, autoreleasepool};
use objc2::runtime::AnyObject;
use objc2_core_foundation::{CGAffineTransform, CGPoint, CGRect, CGSize};
use objc2_core_image::{CIContext, CIImage};
use objc2_core_media::CMSampleBuffer;
use objc2_core_video::{CVPixelBuffer, CVPixelBufferGetHeight, CVPixelBufferGetWidth};
use objc2_foundation::{NSDictionary, NSString};
use objc2_quartz_core::{CALayer, CATransaction, kCAGravityResizeAspectFill};

use super::vision::{FaceDetector, PersonSegmenter};

/// Abstand der Gesichtserkennungen in Sekunden.
const FACE_INTERVAL: f64 = 0.1;
/// Längere Kante des Blasenbilds in Pixeln (die Blase ist höchstens ein
/// paar hundert Punkte gross).
const PREVIEW_PX: f64 = 720.0;
/// Weichzeichnen wie im Export (Scheibe mit 4 % der Breite ≈ Gauss mit 2 %).
const BLUR_SIGMA: f64 = 0.02;

/// Braucht die Blase ein eigenes Bild (statt des Kamerabilds des Systems)?
pub(crate) fn needs_processing(layout: &WebcamLayout) -> bool {
    layout.framing == WebcamFraming::Head || layout.background != WebcamBackground::Original
}

pub(crate) struct LiveVision {
    queue: DispatchRetained<DispatchQueue>,
    busy: AtomicBool,
    /// Zeitstempel (Sekunden, Bits eines f64) der letzten Erkennung.
    last_face: AtomicU64,
    preview_on: AtomicBool,
    inner: Mutex<Inner>,
    /// Zähler fürs Protokoll: verarbeitete und (weil beschäftigt)
    /// ausgelassene Bilder, Rechenzeit in µs.
    processed: AtomicU64,
    skipped: AtomicU64,
    busy_us: AtomicU64,
}

struct Inner {
    detector: Option<FaceDetector>,
    faces: Vec<FaceSample>,
    preview: Option<Preview>,
}

// SAFETY: Vision- und CoreImage-Objekte werden nur unter dem Mutex benutzt,
// die Ebene nur im Haupt-Thread verändert.
unsafe impl Send for Inner {}

struct SendCf<T>(T);
// SAFETY: Pixelpuffer und Bilder werden nach dem Erzeugen nicht verändert;
// die Ebene nur im Haupt-Thread.
unsafe impl<T> Send for SendCf<T> {}

impl LiveVision {
    pub(crate) fn new() -> Arc<Self> {
        Arc::new(Self {
            queue: DispatchQueue::new("ch.forellmedia.fmshot.webcam.vision", None),
            busy: AtomicBool::new(false),
            last_face: AtomicU64::new(f64::NEG_INFINITY.to_bits()),
            preview_on: AtomicBool::new(false),
            inner: Mutex::new(Inner {
                detector: None,
                faces: Vec::new(),
                preview: None,
            }),
            processed: AtomicU64::new(0),
            skipped: AtomicU64::new(0),
            busy_us: AtomicU64::new(0),
        })
    }

    fn lock(&self) -> MutexGuard<'_, Inner> {
        self.inner.lock().unwrap_or_else(|e| e.into_inner())
    }

    /// Eigene Ebene für die Blase anlegen (Haupt-Thread), die dieses Modul
    /// Bild für Bild nach `layout` füllt.
    pub(crate) fn open_preview(&self, layout: &WebcamLayout) -> Retained<CALayer> {
        let layer = CALayer::new();
        layer.setContentsGravity(unsafe { kCAGravityResizeAspectFill });
        let preview = Preview {
            layer: layer.clone(),
            head: (layout.framing == WebcamFraming::Head).then(|| LiveHead::new(layout.follow)),
            background: layout.background,
            mirror: layout.mirror,
            aspect: layout.shape.aspect(),
            segmenter: None,
            context: unsafe { CIContext::contextWithOptions(None) },
            last_pts: None,
        };
        self.lock().preview = Some(preview);
        self.preview_on.store(true, Ordering::Release);
        log::debug!(
            "Eigenes Webcam-Bild: {:?}, Hintergrund {:?}, gespiegelt={}",
            layout.framing,
            layout.background,
            layout.mirror
        );
        layer
    }

    /// Ein Bild der Webcam (aus ihrer Warteschlange). `t_ms` = Zeit im
    /// Video, solange aufgenommen wird (sonst wird nichts gesammelt).
    pub(crate) fn submit(self: &Arc<Self>, sample: &CMSampleBuffer, t_ms: Option<u32>) {
        let pts = unsafe { sample.presentation_time_stamp().seconds() };
        let detect = pts - f64::from_bits(self.last_face.load(Ordering::Relaxed)) >= FACE_INTERVAL;
        if !detect && !self.preview_on.load(Ordering::Acquire) {
            return;
        }
        if self.busy.swap(true, Ordering::AcqRel) {
            self.skipped.fetch_add(1, Ordering::Relaxed);
            return;
        }
        let Some(image) = (unsafe { sample.image_buffer() }) else {
            self.busy.store(false, Ordering::Release);
            return;
        };
        if detect {
            self.last_face.store(pts.to_bits(), Ordering::Relaxed);
        }
        let this = self.clone();
        let image = SendCf(image);
        self.queue.exec_async(move || {
            let image = image;
            let started = std::time::Instant::now();
            this.process(&image.0, pts, t_ms, detect);
            this.busy_us
                .fetch_add(started.elapsed().as_micros() as u64, Ordering::Relaxed);
            this.processed.fetch_add(1, Ordering::Relaxed);
            this.busy.store(false, Ordering::Release);
        });
    }

    fn process(&self, image: &CVPixelBuffer, pts: f64, t_ms: Option<u32>, detect: bool) {
        let mut inner = self.lock();
        let face = if detect {
            let face = inner
                .detector
                .get_or_insert_with(FaceDetector::new)
                .detect(image);
            if let Some(t_ms) = t_ms {
                inner.faces.push(FaceSample { t_ms, face });
            }
            face
        } else {
            None
        };
        if let Some(preview) = inner.preview.as_mut() {
            preview.render(image, pts, face);
        }
    }

    /// Wartet auf die laufende Erkennung und gibt alle Gesichter zurück.
    pub(crate) fn finish(&self) -> Vec<FaceSample> {
        self.preview_on.store(false, Ordering::Release);
        self.queue.exec_sync(|| {});
        let mut inner = self.lock();
        inner.preview = None;
        let faces = std::mem::take(&mut inner.faces);
        let processed = self.processed.swap(0, Ordering::Relaxed);
        let busy_us = self.busy_us.swap(0, Ordering::Relaxed);
        let skipped = self.skipped.swap(0, Ordering::Relaxed);
        if let Some(avg_us) = busy_us.checked_div(processed) {
            log::debug!(
                "Webcam-Erkennung: {processed} Bilder verarbeitet (Ø {avg_us} µs), {skipped} ausgelassen, {} Gesichtsproben, davon {} mit Gesicht",
                faces.len(),
                faces.iter().filter(|f| f.face.is_some()).count()
            );
        }
        faces
    }
}

/// Bild der Blase: Ausschnitt, Hintergrund, Spiegeln.
struct Preview {
    layer: Retained<CALayer>,
    head: Option<LiveHead>,
    background: WebcamBackground,
    mirror: bool,
    /// Seitenverhältnis der Blase (Breite / Höhe).
    aspect: f32,
    segmenter: Option<PersonSegmenter>,
    context: Retained<CIContext>,
    last_pts: Option<f64>,
}

impl Preview {
    fn render(&mut self, image: &CVPixelBuffer, pts: f64, face: Option<Rect>) {
        let dt = self.last_pts.map_or(0.0, |last| (pts - last).max(0.0)) as f32;
        self.last_pts = Some(pts);
        let (w, h) = (
            CVPixelBufferGetWidth(image) as f64,
            CVPixelBufferGetHeight(image) as f64,
        );
        let pose = self.head.as_mut().and_then(|head| head.update(face, dt));
        let crop = match pose {
            Some(pose) => head_crop(pose, Size::new(w as f32, h as f32), self.aspect),
            None => Rect::new(0.0, 0.0, 1.0, 1.0),
        };
        // CoreImage: Ursprung unten links.
        let rect = CGRect::new(
            CGPoint::new(crop.x as f64 * w, (1.0 - crop.bottom() as f64) * h),
            CGSize::new(crop.width as f64 * w, crop.height as f64 * h),
        );
        let scale = (PREVIEW_PX / rect.size.width.max(rect.size.height)).min(1.0);
        let out = CGRect::new(
            CGPoint::new(0.0, 0.0),
            CGSize::new(
                (rect.size.width * scale).round().max(1.0),
                (rect.size.height * scale).round().max(1.0),
            ),
        );
        // Ausschnitt nach (0, 0), verkleinern, gegebenenfalls spiegeln.
        let place = if self.mirror {
            CGAffineTransform {
                a: -scale,
                b: 0.0,
                c: 0.0,
                d: scale,
                tx: scale * rect.origin.x + out.size.width,
                ty: -scale * rect.origin.y,
            }
        } else {
            CGAffineTransform {
                a: scale,
                b: 0.0,
                c: 0.0,
                d: scale,
                tx: -scale * rect.origin.x,
                ty: -scale * rect.origin.y,
            }
        };
        let mask = match self.background {
            WebcamBackground::Original => None,
            _ => self
                .segmenter
                .get_or_insert_with(PersonSegmenter::new)
                .mask(image),
        };
        autoreleasepool(|_| {
            let source = unsafe { CIImage::imageWithCVPixelBuffer(image) };
            let mut frame = unsafe {
                source
                    .imageByCroppingToRect(rect)
                    .imageByApplyingTransform(place)
            };
            if let Some(mask) = mask {
                let (mw, mh) = (
                    CVPixelBufferGetWidth(&mask).max(1) as f64,
                    CVPixelBufferGetHeight(&mask).max(1) as f64,
                );
                let to_frame = CGAffineTransform {
                    a: w / mw,
                    b: 0.0,
                    c: 0.0,
                    d: h / mh,
                    tx: 0.0,
                    ty: 0.0,
                };
                let mask = unsafe {
                    CIImage::imageWithCVPixelBuffer(&mask)
                        .imageByApplyingTransform(to_frame)
                        .imageByCroppingToRect(rect)
                        .imageByApplyingTransform(place)
                };
                let background = unsafe {
                    match self.background {
                        WebcamBackground::Blur => frame
                            .imageByClampingToExtent()
                            .imageByApplyingGaussianBlurWithSigma(BLUR_SIGMA * out.size.width)
                            .imageByCroppingToRect(out),
                        _ => CIImage::clearImage().imageByCroppingToRect(out),
                    }
                };
                let params = NSDictionary::from_retained_objects(
                    &[
                        &*NSString::from_str("inputBackgroundImage"),
                        &*NSString::from_str("inputMaskImage"),
                    ],
                    &[
                        Retained::into_super(Retained::into_super(background)),
                        Retained::into_super(Retained::into_super(mask)),
                    ],
                );
                frame = unsafe {
                    frame.imageByApplyingFilter_withInputParameters(
                        &NSString::from_str("CIBlendWithMask"),
                        &params,
                    )
                };
            }
            let Some(cg) = (unsafe { self.context.createCGImage_fromRect(&frame, out) }) else {
                return;
            };
            show(&self.layer, cg);
        });
    }
}

/// Setzt das Bild im Haupt-Thread (ohne Überblend-Animation).
fn show(layer: &Retained<CALayer>, image: Retained<objc2_core_graphics::CGImage>) {
    let job = SendCf((layer.clone(), image));
    DispatchQueue::main().exec_async(move || {
        let job = job;
        let (layer, image) = &job.0;
        CATransaction::begin();
        CATransaction::setDisableActions(true);
        // SAFETY: CGImage ist als Inhalt einer Ebene erlaubt (gebührenfrei
        // als Objective-C-Objekt verwendbar).
        unsafe {
            layer.setContents(Some(&*(Retained::as_ptr(image) as *const AnyObject)));
        }
        CATransaction::commit();
    });
}
