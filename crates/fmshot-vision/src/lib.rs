//! Bildinhalt erkennen (ADR 0010): Bild und Suchtext werden mit SigLIP 2 zu
//! Vektoren im selben Raum. Ähnliche Inhalte liegen nahe beieinander – „Wald“
//! findet so das Foto mit Bäumen, ohne dass dort Text steht.
//!
//! Plattformunabhängig: ONNX Runtime auf dem Prozessor. Die Modelldateien
//! liefert die App mit (`scripts/fetch-model.sh` stellt sie bereit).

use std::path::Path;
use std::sync::Mutex;

use fmshot_core::RgbaImage;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;
use tokenizers::Tokenizer;

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

impl<T> From<ort::Error<T>> for VisionError {
    fn from(e: ort::Error<T>) -> Self {
        VisionError::Runtime(e.to_string())
    }
}

/// Normierter Vektor (Länge 1) eines Bildes oder Suchtexts.
#[derive(Debug, Clone, PartialEq)]
pub struct Embedding(Vec<f32>);

impl Embedding {
    fn normalized(mut values: Vec<f32>) -> Result<Self, VisionError> {
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

fn session(path: &Path, threads: usize) -> Result<Session, VisionError> {
    Ok(Session::builder()?
        .with_optimization_level(GraphOptimizationLevel::Level3)?
        .with_intra_threads(threads.max(1))?
        .commit_from_file(path)?)
}

fn pooled(outputs: &ort::session::SessionOutputs) -> Result<Embedding, VisionError> {
    let (_, values) = outputs["pooler_output"].try_extract_tensor::<f32>()?;
    Embedding::normalized(values.to_vec())
}

/// Bild → Vektor. Läuft im Hintergrund nach dem Speichern einer Aufnahme.
pub struct ImageEncoder {
    session: Mutex<Session>,
}

impl ImageEncoder {
    /// `threads`: Rechenkerne für ein Bild – klein halten, damit die Erkennung
    /// im Hintergrund die übrige Arbeit nicht bremst.
    pub fn load(model_dir: &Path, threads: usize) -> Result<Self, VisionError> {
        Ok(ImageEncoder {
            session: Mutex::new(session(&model_dir.join(VISION_FILE), threads)?),
        })
    }

    pub fn embed(&self, image: &RgbaImage) -> Result<Embedding, VisionError> {
        let input = Tensor::from_array(([1usize, 3, SIZE, SIZE], pixel_values(image)))?;
        let mut session = self.session.lock().unwrap_or_else(|e| e.into_inner());
        let outputs = session.run(ort::inputs!["pixel_values" => input])?;
        pooled(&outputs)
    }
}

/// Suchtext → Vektor.
pub struct TextEncoder {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
}

impl TextEncoder {
    pub fn load(model_dir: &Path, threads: usize) -> Result<Self, VisionError> {
        let tokenizer = Tokenizer::from_file(model_dir.join(TOKENIZER_FILE))
            .map_err(|e| VisionError::Tokenizer(e.to_string()))?;
        Ok(TextEncoder {
            session: Mutex::new(session(&model_dir.join(TEXT_FILE), threads)?),
            tokenizer,
        })
    }

    /// Token-IDs wie beim Training: klein geschrieben, `<eos>` angehängt, auf
    /// 64 aufgefüllt bzw. gekürzt (das Auffüllen legt `tokenizer.json` fest).
    pub fn token_ids(&self, text: &str) -> Result<Vec<i64>, VisionError> {
        let encoding = self
            .tokenizer
            .encode(text.to_lowercase(), true)
            .map_err(|e| VisionError::Tokenizer(e.to_string()))?;
        let mut ids: Vec<i64> = encoding.get_ids().iter().map(|&id| i64::from(id)).collect();
        if ids.len() > TEXT_TOKENS {
            // Zu lang: kürzen, aber mit `<eos>` enden lassen.
            let eos = *ids.last().unwrap_or(&1);
            ids.truncate(TEXT_TOKENS);
            ids[TEXT_TOKENS - 1] = eos;
        }
        ids.resize(TEXT_TOKENS, 0);
        Ok(ids)
    }

    pub fn embed(&self, text: &str) -> Result<Embedding, VisionError> {
        let input = Tensor::from_array(([1usize, TEXT_TOKENS], self.token_ids(text)?))?;
        let mut session = self.session.lock().unwrap_or_else(|e| e.into_inner());
        let outputs = session.run(ort::inputs!["input_ids" => input])?;
        pooled(&outputs)
    }
}

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
