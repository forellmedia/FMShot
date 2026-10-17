//! Ohne ONNX Runtime (Feature `onnx` aus, z. B. Linux vorerst – ADR 0010):
//! gleiche Schnittstelle, Laden schlägt mit klarer Meldung fehl.

use std::path::Path;

use fmshot_core::RgbaImage;

use crate::{Embedding, VisionError};

fn unavailable() -> VisionError {
    VisionError::Runtime(
        "Bildinhalt-Erkennung ist auf dieser Plattform noch nicht verfügbar".into(),
    )
}

pub struct ImageEncoder(());

impl ImageEncoder {
    pub fn load(_model_dir: &Path, _threads: usize) -> Result<Self, VisionError> {
        Err(unavailable())
    }

    pub fn embed(&self, _image: &RgbaImage) -> Result<Embedding, VisionError> {
        Err(unavailable())
    }
}

pub struct TextEncoder(());

impl TextEncoder {
    pub fn load(_model_dir: &Path, _threads: usize) -> Result<Self, VisionError> {
        Err(unavailable())
    }

    pub fn token_ids(&self, _text: &str) -> Result<Vec<i64>, VisionError> {
        Err(unavailable())
    }

    pub fn embed(&self, _text: &str) -> Result<Embedding, VisionError> {
        Err(unavailable())
    }
}
