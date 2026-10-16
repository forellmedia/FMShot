//! Wird in Meilenstein 2 (Durchstich) umgesetzt.

use crate::{CaptureError, Display, Frame, Result, ScreenCapturer};

pub struct MacCapturer;

impl MacCapturer {
    pub fn new() -> Self {
        Self
    }
}

impl ScreenCapturer for MacCapturer {
    fn displays(&self) -> Result<Vec<Display>> {
        Err(CaptureError::NotImplemented("Monitore auflisten"))
    }

    fn capture_display(&self, _display_id: u32) -> Result<Frame> {
        Err(CaptureError::NotImplemented("Monitor aufnehmen"))
    }
}
