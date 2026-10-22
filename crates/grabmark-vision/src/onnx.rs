//! Ausführung mit ONNX Runtime (Feature `onnx`).

use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use grabmark_core::RgbaImage;
use ort::session::Session;
use ort::session::builder::GraphOptimizationLevel;
use ort::value::Tensor;
use tokenizers::Tokenizer;

use crate::{
    Embedding, ImageVectors, SIZE, TEXT_FILE, TEXT_TOKENS, TOKENIZER_FILE, VISION_FILE,
    VisionError, pixel_values,
};

fn session(path: &Path, threads: usize) -> Result<Session, VisionError> {
    let started = Instant::now();
    let result = (|| -> Result<Session, VisionError> {
        Ok(Session::builder()?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_intra_threads(threads.max(1))?
            .commit_from_file(path)?)
    })();
    match &result {
        Ok(_) => log::info!(
            "Modell {} geladen ({} Thread(s)) in {} ms",
            path.display(),
            threads.max(1),
            started.elapsed().as_millis()
        ),
        Err(e) => log::error!("Modell {} laden fehlgeschlagen: {e}", path.display()),
    }
    result
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

    /// Ganzes Bild und Ausschnitte ([`crate::parts`]) – für die Stichwörter.
    pub fn embed_all(&self, image: &RgbaImage) -> Result<ImageVectors, VisionError> {
        let started = Instant::now();
        let parts = crate::parts(image);
        let result = (|| {
            Ok(ImageVectors {
                whole: self.embed(image)?,
                parts: parts
                    .iter()
                    .map(|p| self.embed(p))
                    .collect::<Result<_, _>>()?,
            })
        })();
        match &result {
            Ok(_) => log::debug!(
                "Bildinhalt {}×{}: ganzes Bild + {} Ausschnitte in {} ms",
                image.width(),
                image.height(),
                parts.len(),
                started.elapsed().as_millis()
            ),
            Err(e) => log::warn!("Bildinhalt berechnen fehlgeschlagen: {e}"),
        }
        result
    }
}

/// Suchtext → Vektor.
pub struct TextEncoder {
    session: Mutex<Session>,
    tokenizer: Tokenizer,
}

impl TextEncoder {
    pub fn load(model_dir: &Path, threads: usize) -> Result<Self, VisionError> {
        let tokenizer = Tokenizer::from_file(model_dir.join(TOKENIZER_FILE)).map_err(|e| {
            log::error!(
                "Tokenizer in {} laden fehlgeschlagen: {e}",
                model_dir.display()
            );
            VisionError::Tokenizer(e.to_string())
        })?;
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
