//! macOS: Apple Vision (`VNRecognizeTextRequest`) – offline, mehrsprachig.

use grabmark_core::{Rect, RgbaImage};
use objc2::AllocAnyThread;
use objc2::rc::{Retained, autoreleasepool};
use objc2_core_foundation::CFData;
use objc2_core_graphics::{
    CGBitmapInfo, CGColorRenderingIntent, CGColorSpace, CGDataProvider, CGImage, CGImageAlphaInfo,
    CGImageByteOrderInfo, kCGColorSpaceSRGB,
};
use objc2_foundation::{NSArray, NSDictionary, NSString};
use objc2_vision::{
    VNImageRequestHandler, VNRecognizeTextRequest, VNRequest, VNRequestTextRecognitionLevel,
};

use crate::{OcrEngine, OcrError, TextBlock};

/// Sprachen in Reihenfolge der Bevorzugung.
const LANGUAGES: [&str; 5] = ["de-DE", "en-US", "fr-FR", "it-IT", "es-ES"];

pub struct VisionOcr;

impl OcrEngine for VisionOcr {
    fn languages(&self) -> Vec<String> {
        LANGUAGES.iter().map(|s| s.to_string()).collect()
    }

    fn recognize(&self, image: &RgbaImage) -> Result<Vec<TextBlock>, OcrError> {
        let started = std::time::Instant::now();
        let result = autoreleasepool(|_| recognize(image));
        match &result {
            // Nur Zahlen – der erkannte Text gehört nicht ins Protokoll.
            Ok(blocks) => log::debug!(
                "Texterkennung {}×{}: {} Blöcke, {} Zeichen in {} ms",
                image.width(),
                image.height(),
                blocks.len(),
                blocks.iter().map(|b| b.text.chars().count()).sum::<usize>(),
                started.elapsed().as_millis()
            ),
            Err(e) => log::warn!(
                "Texterkennung {}×{} fehlgeschlagen: {e}",
                image.width(),
                image.height()
            ),
        }
        result
    }
}

fn recognize(image: &RgbaImage) -> Result<Vec<TextBlock>, OcrError> {
    let cg = to_cgimage(image).ok_or_else(|| OcrError::Backend("CGImage".into()))?;
    let handler = unsafe {
        VNImageRequestHandler::initWithCGImage_options(
            VNImageRequestHandler::alloc(),
            &cg,
            &NSDictionary::new(),
        )
    };
    let request = VNRecognizeTextRequest::new();
    request.setRecognitionLevel(VNRequestTextRecognitionLevel::Accurate);
    request.setUsesLanguageCorrection(true);
    let langs: Vec<Retained<NSString>> = LANGUAGES.iter().map(|l| NSString::from_str(l)).collect();
    request.setRecognitionLanguages(&NSArray::from_retained_slice(&langs));
    request.setAutomaticallyDetectsLanguage(true);

    let as_request: Retained<VNRequest> =
        Retained::into_super(Retained::into_super(request.clone()));
    handler
        .performRequests_error(&NSArray::from_retained_slice(&[as_request]))
        .map_err(|e| {
            log::warn!(
                "Vision: {} ({} {})",
                e.localizedDescription(),
                e.domain(),
                e.code()
            );
            OcrError::Backend(e.localizedDescription().to_string())
        })?;

    let (w, h) = (image.width() as f32, image.height() as f32);
    let mut blocks = Vec::new();
    for obs in request.results().iter().flat_map(|r| r.iter()) {
        let Some(best) = obs.topCandidates(1).firstObject() else {
            continue;
        };
        // Vision: normalisiert, Ursprung unten links.
        let b = unsafe { obs.boundingBox() };
        blocks.push(TextBlock {
            text: best.string().to_string(),
            bounds: Rect::new(
                b.origin.x as f32 * w,
                (1.0 - (b.origin.y + b.size.height) as f32) * h,
                b.size.width as f32 * w,
                b.size.height as f32 * h,
            ),
            confidence: best.confidence(),
        });
    }
    Ok(blocks)
}

fn to_cgimage(image: &RgbaImage) -> Option<objc2_core_foundation::CFRetained<CGImage>> {
    let data = CFData::from_bytes(image.data());
    let provider = CGDataProvider::with_cf_data(Some(&data))?;
    let space = CGColorSpace::with_name(Some(unsafe { kCGColorSpaceSRGB }))?;
    let info = CGBitmapInfo::from_bits_retain(
        CGImageAlphaInfo::Last.0 | CGImageByteOrderInfo::Order32Big.0,
    );
    let w = image.width() as usize;
    unsafe {
        CGImage::new(
            w,
            image.height() as usize,
            8,
            32,
            w * 4,
            Some(&space),
            info,
            Some(&provider),
            std::ptr::null(),
            false,
            CGColorRenderingIntent::RenderingIntentDefault,
        )
    }
}
