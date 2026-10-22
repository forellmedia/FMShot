//! Abschnitte während der Aufnahme mitschreiben: Webcam ein/aus und
//! gross/klein ergeben jeweils einen neuen Abschnitt.

use nabshot_core::{WebcamMode, WebcamSection};

/// Sammelt die Abschnitte einer Aufnahme.
#[derive(Debug, Clone, Default, PartialEq)]
pub struct SectionLog {
    sections: Vec<WebcamSection>,
}

impl SectionLog {
    pub fn new() -> Self {
        Self::default()
    }

    /// Ab `at_ms` gilt `mode`. Gleicher Modus wie bisher ändert nichts; zwei
    /// Wechsel im selben Augenblick ergeben nur den letzten.
    pub fn set(&mut self, at_ms: u32, mode: WebcamMode) {
        if self.current() == mode {
            return;
        }
        if let Some(last) = self.sections.last_mut()
            && last.start_ms >= at_ms
        {
            last.mode = mode;
            let n = self.sections.len();
            // Zurück zum Modus davor: der Abschnitt entfällt ganz.
            if n >= 2 && self.sections[n - 2].mode == mode {
                self.sections.pop();
            }
            return;
        }
        self.sections.push(WebcamSection {
            start_ms: at_ms,
            mode,
        });
    }

    /// Modus am Ende der bisherigen Aufnahme.
    pub fn current(&self) -> WebcamMode {
        self.sections
            .last()
            .map(|s| s.mode)
            .unwrap_or(WebcamMode::Hidden)
    }

    /// Wurde die Webcam überhaupt einmal gezeigt?
    pub fn shows_webcam(&self) -> bool {
        self.sections.iter().any(|s| s.mode != WebcamMode::Hidden)
    }

    pub fn into_sections(self) -> Vec<WebcamSection> {
        self.sections
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn modes(log: &SectionLog) -> Vec<(u32, WebcamMode)> {
        log.clone()
            .into_sections()
            .iter()
            .map(|s| (s.start_ms, s.mode))
            .collect()
    }

    #[test]
    fn wechsel_werden_abschnitte() {
        let mut log = SectionLog::new();
        log.set(0, WebcamMode::Overlay);
        log.set(2000, WebcamMode::Large);
        log.set(2000, WebcamMode::Large);
        log.set(5000, WebcamMode::Hidden);
        assert_eq!(
            modes(&log),
            [
                (0, WebcamMode::Overlay),
                (2000, WebcamMode::Large),
                (5000, WebcamMode::Hidden)
            ]
        );
        assert!(log.shows_webcam());
    }

    #[test]
    fn hin_und_zurueck_im_selben_augenblick_verschwindet() {
        let mut log = SectionLog::new();
        log.set(0, WebcamMode::Overlay);
        log.set(3000, WebcamMode::Hidden);
        log.set(3000, WebcamMode::Overlay);
        assert_eq!(modes(&log), [(0, WebcamMode::Overlay)]);
    }

    #[test]
    fn nie_eingeschaltet() {
        let mut log = SectionLog::new();
        log.set(0, WebcamMode::Hidden);
        assert!(!log.shows_webcam());
        assert_eq!(modes(&log), []);
    }
}
