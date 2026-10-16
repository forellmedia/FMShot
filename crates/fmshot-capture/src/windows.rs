//! Wird in Meilenstein 2 (Durchstich) umgesetzt.

use crate::{CaptureError, Display, Frame, Result, ScreenCapturer};

pub struct WindowsCapturer;

impl WindowsCapturer {
    pub fn new() -> Self {
        Self
    }
}

impl ScreenCapturer for WindowsCapturer {
    fn displays(&self) -> Result<Vec<Display>> {
        Err(CaptureError::NotImplemented("Monitore auflisten"))
    }

    fn capture_display(&self, _display_id: u32) -> Result<Frame> {
        Err(CaptureError::NotImplemented("Monitor aufnehmen"))
    }
}
