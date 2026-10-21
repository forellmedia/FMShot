//! Bildinhalt erkennen (ADR 0010): Bild und Suchtext werden mit SigLIP 2 zu
//! Vektoren im selben Raum. Ähnliche Inhalte liegen nahe beieinander – „Wald“
//! findet so das Foto mit Bäumen, ohne dass dort Text steht.
//!
//! Plattformunabhängig: ONNX Runtime auf dem Prozessor. Die Modelldateien
//! liefert die App mit (`scripts/fetch-model.sh` stellt sie bereit).

mod preprocess;
mod tagger;
pub mod terms;
mod views;

pub use preprocess::{SIZE, pixel_values};
pub use tagger::{Tagger, tags_version};
pub use views::parts;

/// Kennung von Modell und Vorverarbeitung. Ändert sie sich, sind gespeicherte
/// Vektoren ungültig und werden neu berechnet.
pub const MODEL_ID: &str = "siglip2-base-p16-224/w8-q4/2";

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

/// Vektoren einer Aufnahme: das ganze Bild (Suche) und die Ausschnitte aus
/// [`parts`] (Stichwörter). Gespeichert wird das ganze Bild genau, die
/// Ausschnitte auf 8 Bit gerundet – ein Viertel des Platzes, die Logits
/// weichen dadurch um rund 0,1 ab.
#[derive(Debug, Clone, PartialEq)]
pub struct ImageVectors {
    pub whole: Embedding,
    pub parts: Vec<Embedding>,
}

/// Bytes eines Ausschnitts: Massstab (f32) und 768 Werte (i8).
const PART_BYTES: usize = 4 + DIM;

impl ImageVectors {
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut out = self.whole.to_bytes();
        for part in &self.parts {
            let max = part.0.iter().fold(0.0f32, |m, v| m.max(v.abs()));
            let scale = (max / 127.0).max(f32::MIN_POSITIVE);
            out.extend(scale.to_le_bytes());
            out.extend(part.0.iter().map(|v| (v / scale).round() as i8 as u8));
        }
        out
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, VisionError> {
        let whole_len = DIM * 4;
        if bytes.len() < whole_len || !(bytes.len() - whole_len).is_multiple_of(PART_BYTES) {
            return Err(VisionError::Bytes(bytes.len()));
        }
        let (whole, rest) = bytes.split_at(whole_len);
        let parts = rest
            .chunks(PART_BYTES)
            .map(|c| {
                let (scale, values) = c.split_at(4);
                let scale = f32::from_le_bytes([scale[0], scale[1], scale[2], scale[3]]);
                Embedding::normalized(values.iter().map(|&b| f32::from(b as i8) * scale).collect())
            })
            .collect::<Result<_, _>>()?;
        Ok(ImageVectors {
            whole: Embedding::from_bytes(whole)?,
            parts,
        })
    }

    /// Ganzes Bild und Ausschnitte.
    pub fn all(&self) -> impl Iterator<Item = &Embedding> {
        std::iter::once(&self.whole).chain(&self.parts)
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
    fn ausschnitte_gerundet_speichern() {
        let unit = |seed: f32| {
            let values = (0..DIM).map(|i| ((i as f32 + seed) * 0.37).sin()).collect();
            Embedding::normalized(values).unwrap()
        };
        let v = ImageVectors {
            whole: unit(1.0),
            parts: vec![unit(2.0), unit(3.0)],
        };
        let bytes = v.to_bytes();
        assert_eq!(bytes.len(), DIM * 4 + 2 * PART_BYTES);
        let back = ImageVectors::from_bytes(&bytes).unwrap();
        assert_eq!(back.whole, v.whole);
        for (a, b) in back.parts.iter().zip(&v.parts) {
            assert!(a.similarity(b) > 0.9999, "{}", a.similarity(b));
        }
        // Alte Vektoren ohne Ausschnitte bleiben lesbar.
        let alt = ImageVectors::from_bytes(&v.whole.to_bytes()).unwrap();
        assert!(alt.parts.is_empty());
        assert!(ImageVectors::from_bytes(&bytes[..bytes.len() - 1]).is_err());
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
