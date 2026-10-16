//! Texterkennung über die Systemengines (ADR 0005).
//!
//! - macOS: Apple Vision
//! - Windows: Windows.Media.Ocr
//! - Linux: Tesseract (zur Laufzeit geladen)
//!
//! Windows und Linux folgen; dort liefert [`platform_engine`] vorerst `None`.

use fmshot_core::{Rect, RgbaImage};

#[cfg(target_os = "macos")]
mod macos;

/// Die Texterkennung der aktuellen Plattform, falls vorhanden.
pub fn platform_engine() -> Option<Box<dyn OcrEngine>> {
    #[cfg(target_os = "macos")]
    return Some(Box::new(macos::VisionOcr));
    #[cfg(not(target_os = "macos"))]
    None
}

/// Ein erkannter Textblock (meist eine Zeile).
#[derive(Debug, Clone, PartialEq)]
pub struct TextBlock {
    pub text: String,
    /// Lage im Bild in Pixeln.
    pub bounds: Rect,
    /// 0.0 bis 1.0
    pub confidence: f32,
}

#[derive(Debug, thiserror::Error)]
pub enum OcrError {
    #[error("Keine Texterkennung verfügbar: {0}")]
    Unavailable(String),
    #[error("Texterkennung fehlgeschlagen: {0}")]
    Backend(String),
}

pub trait OcrEngine: Send + Sync {
    /// Sprachen im BCP-47-Format, z. B. `de-DE`, `en-US`.
    fn languages(&self) -> Vec<String>;
    fn recognize(&self, image: &RgbaImage) -> Result<Vec<TextBlock>, OcrError>;
}

/// Fügt die Blöcke in Lesereihenfolge zu einem Text zusammen.
pub fn join_text(blocks: &[TextBlock]) -> String {
    let mut sorted: Vec<&TextBlock> = blocks.iter().collect();
    sorted.sort_by(|a, b| {
        a.bounds
            .y
            .total_cmp(&b.bounds.y)
            .then(a.bounds.x.total_cmp(&b.bounds.x))
    });
    sorted
        .iter()
        .map(|b| b.text.as_str())
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn block(text: &str, x: f32, y: f32) -> TextBlock {
        TextBlock {
            text: text.into(),
            bounds: Rect::new(x, y, 10.0, 10.0),
            confidence: 1.0,
        }
    }

    #[test]
    fn text_in_lesereihenfolge() {
        let blocks = [block("unten", 0.0, 50.0), block("oben", 0.0, 0.0)];
        assert_eq!(join_text(&blocks), "oben\nunten");
    }
}
