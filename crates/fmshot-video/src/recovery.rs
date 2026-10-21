//! Nach einem Absturz (ADR 0012): Dokument einer abgebrochenen Aufnahme aus
//! dem Zwischenstand (`<id>_recording.json`) und den Spuren, die da sind.

use fmshot_core::{
    CheckpointDrawing, RecordingCheckpoint, VideoDoc, WebcamMode, WebcamSection, WebcamTrack,
};

/// Was sich wiederherstellen lässt.
#[derive(Debug, Clone, PartialEq)]
pub struct Recovered {
    pub video: VideoDoc,
    /// Pixel je Punkt für das Dokument (1 bei „nur Webcam“).
    pub scale: f32,
    /// Live-Zeichnungen, auf die wirkliche Länge gekürzt.
    pub drawings: Vec<CheckpointDrawing>,
}

/// Baut das Video-Dokument. `checkpoint` fehlt, wenn die App vor dem ersten
/// Zwischenstand abstürzte – dann gilt `fallback` (aus den Einstellungen).
/// `duration_ms` = Länge der Hauptspur, so weit sie lesbar ist.
pub fn recovered(
    checkpoint: Option<RecordingCheckpoint>,
    fallback: RecordingCheckpoint,
    duration_ms: u32,
    has_screen: bool,
    has_webcam: bool,
) -> Recovered {
    let cp = checkpoint.unwrap_or(fallback);
    // Ohne Bildschirm-Spur war es „nur Webcam“.
    let camera_only = (!has_screen).then(|| cp.camera_only.unwrap_or_default());
    let webcam = has_webcam.then(|| {
        let mut sections: Vec<WebcamSection> = cp
            .webcam_sections
            .iter()
            .copied()
            .filter(|s| s.start_ms < duration_ms)
            .collect();
        if camera_only.is_some() {
            sections = vec![WebcamSection {
                start_ms: 0,
                mode: WebcamMode::Large,
            }];
        } else if sections.is_empty() {
            // Die Spur gibt es, also war die Webcam an – wann, ist unbekannt.
            sections.push(WebcamSection {
                start_ms: 0,
                mode: WebcamMode::Overlay,
            });
        }
        WebcamTrack {
            layout: cp.webcam_layout,
            sections,
        }
    });
    let webcam = webcam.filter(|w| w.sections.iter().any(|s| s.mode != WebcamMode::Hidden));
    let drawings = cp
        .drawings
        .into_iter()
        .filter(|d| d.start_ms < duration_ms)
        .map(|d| CheckpointDrawing {
            end_ms: d.end_ms.min(duration_ms),
            ..d
        })
        .collect();
    Recovered {
        video: VideoDoc {
            duration_ms,
            // Die Kamera liefert 30 Bilder je Sekunde.
            fps: if camera_only.is_some() { 30 } else { cp.fps },
            webcam,
            camera_only,
            ..VideoDoc::default()
        },
        scale: if camera_only.is_some() { 1.0 } else { cp.scale },
        drawings,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use fmshot_core::{CameraFormat, Color, LayerKind, Point, Stroke};

    fn checkpoint() -> RecordingCheckpoint {
        RecordingCheckpoint {
            fps: 60,
            scale: 2.0,
            webcam_sections: vec![
                WebcamSection {
                    start_ms: 0,
                    mode: WebcamMode::Overlay,
                },
                WebcamSection {
                    start_ms: 9_000,
                    mode: WebcamMode::Large,
                },
            ],
            drawings: vec![drawing(1_000, 4_000), drawing(7_000, 11_000)],
            ..RecordingCheckpoint::default()
        }
    }

    fn drawing(start_ms: u32, end_ms: u32) -> CheckpointDrawing {
        CheckpointDrawing {
            kind: LayerKind::Line {
                from: Point::new(0.0, 0.0),
                to: Point::new(10.0, 10.0),
                stroke: Stroke::new(Color::rgb(255, 0, 0), 4.0),
            },
            opacity: 1.0,
            start_ms,
            end_ms,
        }
    }

    #[test]
    fn zwischenstand_ergibt_webcam_und_zeichnungen_bis_zum_abbruch() {
        let r = recovered(
            Some(checkpoint()),
            RecordingCheckpoint::default(),
            8_000,
            true,
            true,
        );
        assert_eq!(r.video.duration_ms, 8_000);
        assert_eq!(r.video.fps, 60);
        assert_eq!(r.scale, 2.0);
        assert_eq!(r.video.camera_only, None);
        // Der Abschnitt nach dem Abbruch fällt weg.
        let webcam = r.video.webcam.unwrap();
        assert_eq!(webcam.sections.len(), 1);
        assert_eq!(webcam.sections[0].mode, WebcamMode::Overlay);
        // Zweite Zeichnung endet mit dem Video.
        assert_eq!(
            r.drawings
                .iter()
                .map(|d| (d.start_ms, d.end_ms))
                .collect::<Vec<_>>(),
            [(1_000, 4_000), (7_000, 8_000)]
        );
    }

    #[test]
    fn ohne_zwischenstand_gelten_die_einstellungen() {
        let fallback = RecordingCheckpoint {
            fps: 30,
            scale: 2.0,
            ..RecordingCheckpoint::default()
        };
        let r = recovered(None, fallback, 5_000, true, true);
        assert_eq!(r.video.fps, 30);
        // Webcam-Spur da, Abschnitte unbekannt: durchgehend klein.
        let webcam = r.video.webcam.unwrap();
        assert_eq!(webcam.sections[0].mode, WebcamMode::Overlay);
        assert!(r.drawings.is_empty());
    }

    #[test]
    fn ohne_bildschirm_ist_es_nur_webcam() {
        let r = recovered(
            Some(checkpoint()),
            RecordingCheckpoint::default(),
            3_000,
            false,
            true,
        );
        assert_eq!(r.video.camera_only, Some(CameraFormat::default()));
        assert_eq!(r.video.fps, 30);
        assert_eq!(r.scale, 1.0);
        assert_eq!(r.video.webcam.unwrap().sections[0].mode, WebcamMode::Large);
    }

    #[test]
    fn ohne_webcam_spur_keine_webcam() {
        let r = recovered(
            Some(checkpoint()),
            RecordingCheckpoint::default(),
            8_000,
            true,
            false,
        );
        assert_eq!(r.video.webcam, None);
    }

    #[test]
    fn zwischenstand_liest_sich_zurueck() {
        let cp = checkpoint();
        let json = serde_json::to_string(&cp).unwrap();
        let back: RecordingCheckpoint = serde_json::from_str(&json).unwrap();
        assert_eq!(back, cp);
        // Ältere oder unvollständige Datei: fehlende Felder mit Standard.
        let partial: RecordingCheckpoint = serde_json::from_str(r#"{"fps": 24}"#).unwrap();
        assert_eq!(partial.fps, 24);
        assert_eq!(partial.scale, 1.0);
    }
}
