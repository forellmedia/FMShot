//! Apple Vision (ADR 0011, Etappe 3): Gesichter finden und die Person vom
//! Hintergrund trennen – offline, ohne Modell-Download, auf der Neural
//! Engine.

use std::ptr::NonNull;

use fmshot_core::Rect;
use fmshot_video::{FrameRef, Mask, Segmenter};
use objc2::rc::{Retained, autoreleasepool};
use objc2::{AllocAnyThread, msg_send};
use objc2_core_foundation::{CFRetained, CGRect};
use objc2_core_video::{
    CVPixelBuffer, CVPixelBufferCreate, CVPixelBufferGetBaseAddress, CVPixelBufferGetBytesPerRow,
    CVPixelBufferGetHeight, CVPixelBufferGetWidth, CVPixelBufferLockBaseAddress,
    CVPixelBufferLockFlags, CVPixelBufferUnlockBaseAddress,
};
use objc2_foundation::NSArray;
use objc2_vision::{
    VNDetectFaceRectanglesRequest, VNGeneratePersonSegmentationRequest,
    VNGeneratePersonSegmentationRequestQualityLevel, VNRequest, VNSequenceRequestHandler,
};

/// `L008`: ein Byte je Pixel.
const PIXEL_FORMAT_ONE_COMPONENT_8: u32 = 0x4C30_3038;
/// `BGRA`
const PIXEL_FORMAT_BGRA: u32 = 0x4247_5241;

fn read_only() -> CVPixelBufferLockFlags {
    CVPixelBufferLockFlags(1)
}

fn sequence_handler() -> Retained<VNSequenceRequestHandler> {
    unsafe { msg_send![VNSequenceRequestHandler::alloc(), init] }
}

/// Vision rechnet mit Ursprung unten links → Ursprung oben links.
pub(crate) fn from_vision(b: CGRect) -> Rect {
    Rect::new(
        b.origin.x as f32,
        (1.0 - (b.origin.y + b.size.height)) as f32,
        b.size.width as f32,
        b.size.height as f32,
    )
}

/// Findet das grösste Gesicht in einem Kamerabild.
pub(crate) struct FaceDetector {
    handler: Retained<VNSequenceRequestHandler>,
    request: Retained<VNDetectFaceRectanglesRequest>,
}

impl FaceDetector {
    pub(crate) fn new() -> Self {
        Self {
            handler: sequence_handler(),
            request: unsafe { VNDetectFaceRectanglesRequest::new() },
        }
    }

    /// Grösstes Gesicht als Anteil des Bilds (Ursprung oben links, nicht
    /// gespiegelt); `None` = keins.
    pub(crate) fn detect(&self, image: &CVPixelBuffer) -> Option<Rect> {
        autoreleasepool(|_| {
            let request: Retained<VNRequest> =
                Retained::into_super(Retained::into_super(self.request.clone()));
            let result = unsafe {
                self.handler.performRequests_onCVPixelBuffer_error(
                    &NSArray::from_retained_slice(&[request]),
                    image,
                )
            };
            if let Err(e) = result {
                log::debug!("Gesichtserkennung: {}", e.localizedDescription());
                return None;
            }
            let results = unsafe { self.request.results() }?;
            results
                .iter()
                .map(|face| from_vision(unsafe { face.boundingBox() }))
                .max_by(|a, b| (a.width * a.height).total_cmp(&(b.width * b.height)))
        })
    }
}

/// Maske der Person (Qualität „balanced“ – gedacht für Video). Die
/// Sequenz glättet die Maske über mehrere Bilder.
pub(crate) struct PersonSegmenter {
    handler: Retained<VNSequenceRequestHandler>,
    request: Retained<VNGeneratePersonSegmentationRequest>,
}

impl PersonSegmenter {
    pub(crate) fn new() -> Self {
        let request = unsafe { VNGeneratePersonSegmentationRequest::new() };
        unsafe {
            request.setQualityLevel(VNGeneratePersonSegmentationRequestQualityLevel::Balanced);
            request.setOutputPixelFormat(PIXEL_FORMAT_ONE_COMPONENT_8);
        }
        Self {
            handler: sequence_handler(),
            request,
        }
    }

    /// Maske als Bild mit einem Byte je Pixel (255 = Person), Zeilen von
    /// oben nach unten wie das Kamerabild.
    pub(crate) fn mask(&self, image: &CVPixelBuffer) -> Option<Retained<CVPixelBuffer>> {
        autoreleasepool(|_| {
            let request: Retained<VNRequest> = Retained::into_super(Retained::into_super(
                Retained::into_super(self.request.clone()),
            ));
            let result = unsafe {
                self.handler.performRequests_onCVPixelBuffer_error(
                    &NSArray::from_retained_slice(&[request]),
                    image,
                )
            };
            if let Err(e) = result {
                log::debug!("Person freistellen: {}", e.localizedDescription());
                return None;
            }
            let results = unsafe { self.request.results() }?;
            let first = results.firstObject()?;
            Some(unsafe { first.pixelBuffer() })
        })
    }
}

/// [`Segmenter`] für den Export: BGRA-Bild → Maske.
pub(crate) struct VisionSegmenter {
    inner: PersonSegmenter,
    /// Wiederverwendet, solange die Grösse gleich bleibt.
    buffer: Option<CFRetained<CVPixelBuffer>>,
}

// SAFETY: Vision-Objekte und Puffer werden nur von dem Thread benutzt, der
// den Segmenter gerade besitzt (`&mut self`).
unsafe impl Send for VisionSegmenter {}

impl VisionSegmenter {
    pub(crate) fn new() -> Self {
        Self {
            inner: PersonSegmenter::new(),
            buffer: None,
        }
    }

    /// Kopiert das Bild in einen Pixelpuffer (Vision liest nur solche).
    fn upload(&mut self, frame: FrameRef<'_>) -> Option<()> {
        let (w, h) = (frame.width as usize, frame.height as usize);
        if frame.data.len() < frame.stride * h.saturating_sub(1) + w * 4 {
            return None;
        }
        let fits = self
            .buffer
            .as_ref()
            .is_some_and(|b| CVPixelBufferGetWidth(b) == w && CVPixelBufferGetHeight(b) == h);
        if !fits {
            let mut out: *mut CVPixelBuffer = std::ptr::null_mut();
            let status = unsafe {
                CVPixelBufferCreate(None, w, h, PIXEL_FORMAT_BGRA, None, NonNull::from(&mut out))
            };
            if status != 0 {
                return None;
            }
            // SAFETY: frisch erzeugt (+1).
            self.buffer = Some(unsafe { CFRetained::from_raw(NonNull::new(out)?) });
        }
        let buffer = self.buffer.as_deref()?;
        if unsafe { CVPixelBufferLockBaseAddress(buffer, CVPixelBufferLockFlags(0)) } != 0 {
            return None;
        }
        let base = CVPixelBufferGetBaseAddress(buffer) as *mut u8;
        let row = CVPixelBufferGetBytesPerRow(buffer);
        if !base.is_null() {
            for y in 0..h {
                let src = &frame.data[y * frame.stride..y * frame.stride + w * 4];
                // SAFETY: Puffer ist gesperrt und hat `h` Zeilen zu `row` Bytes.
                unsafe {
                    std::ptr::copy_nonoverlapping(src.as_ptr(), base.add(y * row), w * 4);
                }
            }
        }
        unsafe { CVPixelBufferUnlockBaseAddress(buffer, CVPixelBufferLockFlags(0)) };
        (!base.is_null()).then_some(())
    }
}

impl Segmenter for VisionSegmenter {
    fn mask(&mut self, frame: FrameRef<'_>) -> Option<Mask> {
        self.upload(frame)?;
        let mask = self.inner.mask(self.buffer.as_deref()?)?;
        copy_mask(&mask)
    }
}

/// Maske aus dem Pixelpuffer von Vision in eine lückenlose Bytefolge.
pub(crate) fn copy_mask(mask: &CVPixelBuffer) -> Option<Mask> {
    if unsafe { CVPixelBufferLockBaseAddress(mask, read_only()) } != 0 {
        return None;
    }
    let (w, h) = (CVPixelBufferGetWidth(mask), CVPixelBufferGetHeight(mask));
    let row = CVPixelBufferGetBytesPerRow(mask);
    let base = CVPixelBufferGetBaseAddress(mask) as *const u8;
    let result = (!base.is_null() && row >= w).then(|| {
        let mut data = Vec::with_capacity(w * h);
        for y in 0..h {
            // SAFETY: gesperrt, `h` Zeilen zu `row` Bytes.
            data.extend_from_slice(unsafe { std::slice::from_raw_parts(base.add(y * row), w) });
        }
        Mask {
            width: w as u32,
            height: h as u32,
            data,
        }
    });
    unsafe { CVPixelBufferUnlockBaseAddress(mask, read_only()) };
    result
}

#[cfg(test)]
mod tests {
    use objc2_core_foundation::{CGPoint, CGSize};

    use super::*;

    #[test]
    fn vision_ursprung_unten_links_wird_oben_links() {
        let r = from_vision(CGRect::new(CGPoint::new(0.25, 0.1), CGSize::new(0.5, 0.3)));
        assert_eq!(r, Rect::new(0.25, 0.6, 0.5, 0.3));
    }

    #[test]
    fn leeres_bild_ohne_person_ergibt_maske_oder_nichts() {
        // Vision läuft auch ohne Bildschirm; ein graues Bild hat keine Person.
        let (w, h) = (320u32, 180u32);
        let data = vec![128u8; (w * h * 4) as usize];
        let mut seg = VisionSegmenter::new();
        if let Some(mask) = seg.mask(FrameRef {
            width: w,
            height: h,
            stride: w as usize * 4,
            data: &data,
        }) {
            assert_eq!(mask.data.len(), (mask.width * mask.height) as usize);
            let person = mask.data.iter().filter(|&&v| v > 200).count();
            assert!(person < mask.data.len() / 10, "Person im leeren Bild");
        }
    }
}
