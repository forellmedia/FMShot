//! Bildinhalt erkennen (ADR 0010): Bild und Suchtext werden mit SigLIP 2 zu
//! Vektoren im selben Raum. Ähnliche Inhalte liegen nahe beieinander – „Wald“
//! findet so das Foto mit Bäumen, ohne dass dort Text steht.
//!
//! Plattformunabhängig: ONNX Runtime auf dem Prozessor. Die Modelldateien
//! liefert die App mit (`scripts/fetch-model.sh` stellt sie bereit).

mod preprocess;
mod tagger;
pub mod terms;

pub use preprocess::{SIZE, pixel_values};
pub use tagger::{Tagger, tags_version};

/// Kennung von Modell und Vorverarbeitung. Ändert sie sich, sind gespeicherte
/// Vektoren ungültig und werden neu berechnet.
pub const MODEL_ID: &str = "siglip2-base-p16-224/w8-q4/1";

/// Länge eines Vektors.
pub const DIM: usize = 768;

/// Dateinamen im Modellordner.
pub const VISION_FILE: &str = "vision.onnx";
pub const TEXT_FILE: &str = "text.onnx";
pub const TOKENIZER_FILE: &str = "tokenizer.json";

/// So viele Tokens verarbeitet das Textmodell (mit diesem Wert trainiert).
#[cfg_attr(not(feature = "onnx"), allow(dead_code))]
const TEXT_TOKENS: usize = 64;

#[derive(Debug, thiserror::Error)]
pub enum VisionError {
    #[error("Bildmodell konnte nicht geladen oder ausgeführt werden: {0}")]
    Runtime(String),
    #[error("Tokenizer: {0}")]
    Tokenizer(String),
    #[error("Modell liefert unerwartete Ausgabe ({0} statt {DIM} Werte)")]
    Output(usize),
    #[error("Vektor hat {0} Bytes, erwartet waren {expected}", expected = DIM * 4)]
    Bytes(usize),
}

#[cfg(feature = "onnx")]
impl<T> From<ort::Error<T>> for VisionError {
    fn from(e: ort::Error<T>) -> Self {
        VisionError::Runtime(e.to_string())
    }
}

/// Normierter Vektor (Länge 1) eines Bildes oder Suchtexts.
#[derive(Debug, Clone, PartialEq)]
pub struct Embedding(Vec<f32>);

impl Embedding {
    pub(crate) fn normalized(mut values: Vec<f32>) -> Result<Self, VisionError> {
        if values.len() != DIM {
            return Err(VisionError::Output(values.len()));
        }
        let norm = values
            .iter()
            .map(|v| v * v)
            .sum::<f32>()
            .sqrt()
            .max(f32::EPSILON);
        values.iter_mut().for_each(|v| *v /= norm);
        Ok(Embedding(values))
    }

    /// Ähnlichkeit −1 … 1 (Kosinus). Bild zu passendem Suchwort liegt bei
    /// SigLIP typisch um 0,1, unpassend um 0,0 bis 0,05.
    pub fn similarity(&self, other: &Embedding) -> f32 {
        self.0.iter().zip(&other.0).map(|(a, b)| a * b).sum()
    }

    pub fn values(&self) -> &[f32] {
        &self.0
    }

    /// Zum Speichern (BLOB): f32 little-endian.
    pub fn to_bytes(&self) -> Vec<u8> {
        self.0.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, VisionError> {
        if bytes.len() != DIM * 4 {
            return Err(VisionError::Bytes(bytes.len()));
        }
        let values = bytes
            .as_chunks::<4>()
            .0
            .iter()
            .map(|&c| f32::from_le_bytes(c))
            .collect();
        Ok(Embedding(values))
    }
}

#[cfg(feature = "onnx")]
mod onnx;
#[cfg(feature = "onnx")]
pub use onnx::{ImageEncoder, TextEncoder};

#[cfg(not(feature = "onnx"))]
mod unavailable;
#[cfg(not(feature = "onnx"))]
pub use unavailable::{ImageEncoder, TextEncoder};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vektor_wird_normiert_und_als_bytes_gespeichert() {
        let mut values = vec![0.0; DIM];
        values[0] = 3.0;
        values[1] = 4.0;
        let e = Embedding::normalized(values).unwrap();
        assert!((e.values()[0] - 0.6).abs() < 1e-6);
        assert!((e.similarity(&e) - 1.0).abs() < 1e-6);
        let back = Embedding::from_bytes(&e.to_bytes()).unwrap();
        assert_eq!(back, e);
    }

    #[test]
    fn falsche_laengen_werden_abgelehnt() {
        assert!(matches!(
            Embedding::normalized(vec![1.0; 3]),
            Err(VisionError::Output(3))
        ));
        assert!(matches!(
            Embedding::from_bytes(&[0; 8]),
            Err(VisionError::Bytes(8))
        ));
    }
}
