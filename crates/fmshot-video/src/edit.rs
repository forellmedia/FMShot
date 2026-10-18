//! Bearbeiten im Video-Editor (Etappe 4–6). Der Editor arbeitet in Zeit der
//! Ausgabe (was man sieht); gespeichert wird in Zeit der Aufnahme. Alle
//! Funktionen geben ein neues Dokument zurück, damit Rückgängig einfach bleibt.

use fmshot_core::{LayerTime, TimeRange, VideoDoc, WebcamMode, WebcamSection, ZoomSection};

use crate::timeline::{kept, normalized, output_duration, source_time};

/// Länge eines neuen Zoom-Abschnitts.
pub const ZOOM_LENGTH_MS: u32 = 3000;
/// Standard-Vergrösserung beim Zoom.
pub const ZOOM_SCALE: f32 = 2.0;

/// Stücke der Aufnahme (Quelle), die im Ausgabe-Bereich liegen.
fn source_pieces(doc: &VideoDoc, out_start: f64, out_end: f64) -> Vec<TimeRange> {
    let (a, b) = (out_start.min(out_end), out_start.max(out_end));
    let mut pieces = Vec::new();
    let mut offset = 0.0;
    for r in kept(doc) {
        let len = (r.end_ms - r.start_ms) as f64;
        let (s, e) = (a.max(offset), b.min(offset + len));
        if e > s {
            pieces.push(TimeRange {
                start_ms: (r.start_ms as f64 + s - offset).round() as u32,
                end_ms: (r.start_ms as f64 + e - offset).round() as u32,
            });
        }
        offset += len;
    }
    pieces
}

/// Schneidet den Bereich (Ausgabe-Zeit) heraus.
pub fn cut(doc: &VideoDoc, out_start: f64, out_end: f64) -> VideoDoc {
    let mut d = doc.clone();
    d.cuts.extend(source_pieces(doc, out_start, out_end));
    d.cuts = normalized(&d.cuts, d.duration_ms);
    // Nie alles wegschneiden.
    if output_duration(&d) == 0 {
        return doc.clone();
    }
    d
}

/// Behält nur den Bereich (Ausgabe-Zeit): schneidet davor und danach.
pub fn trim(doc: &VideoDoc, out_start: f64, out_end: f64) -> VideoDoc {
    let total = output_duration(doc) as f64;
    let (a, b) = (
        out_start.min(out_end).max(0.0),
        out_start.max(out_end).min(total),
    );
    let d = cut(doc, b, total);
    cut(&d, 0.0, a)
}

/// Alle Schnitte zurücknehmen.
pub fn uncut(doc: &VideoDoc) -> VideoDoc {
    VideoDoc {
        cuts: Vec::new(),
        ..doc.clone()
    }
}

/// Modus der Webcam zur Quellzeit `t` laut Abschnitten.
fn mode_at(sections: &[WebcamSection], t: u32) -> WebcamMode {
    sections
        .iter()
        .take_while(|s| s.start_ms <= t)
        .last()
        .map_or(WebcamMode::Hidden, |s| s.mode)
}

/// Webcam im Bereich (Ausgabe-Zeit) gross, klein oder aus. Danach gilt
/// wieder, was vorher galt.
pub fn set_webcam_mode(doc: &VideoDoc, out_start: f64, out_end: f64, mode: WebcamMode) -> VideoDoc {
    let mut d = doc.clone();
    let Some(track) = d.webcam.as_mut() else {
        return d;
    };
    let (a, b) = (out_start.min(out_end), out_start.max(out_end));
    let s = source_time(doc, a).round() as u32;
    let e = source_time(doc, b).round() as u32;
    if e <= s {
        return d;
    }
    let after = mode_at(&track.sections, e);
    track.sections.retain(|x| x.start_ms < s || x.start_ms > e);
    track.sections.push(WebcamSection { start_ms: s, mode });
    if e < doc.duration_ms {
        track.sections.push(WebcamSection {
            start_ms: e,
            mode: after,
        });
    }
    track.sections.sort_by_key(|x| x.start_ms);
    // Gleiche Modi hintereinander zusammenlegen.
    let mut merged: Vec<WebcamSection> = Vec::with_capacity(track.sections.len());
    for x in track.sections.drain(..) {
        let previous = merged.last().map(|p| p.mode);
        if previous != Some(x.mode)
            && !(previous.is_none() && x.mode == WebcamMode::Hidden && x.start_ms == 0)
        {
            merged.push(x);
        }
    }
    track.sections = merged;
    d
}

/// Neuer Zoom ab `out_t` (Ausgabe-Zeit) auf die Stelle `x`, `y`.
pub fn add_zoom(doc: &VideoDoc, out_t: f64, x: f32, y: f32) -> VideoDoc {
    let mut d = doc.clone();
    let start = source_time(doc, out_t).round() as u32;
    let end = (start + ZOOM_LENGTH_MS).min(doc.duration_ms);
    if end <= start {
        return d;
    }
    d.zooms.push(ZoomSection {
        start_ms: start,
        end_ms: end,
        x: x.clamp(0.0, 1.0),
        y: y.clamp(0.0, 1.0),
        scale: ZOOM_SCALE,
    });
    d.zooms.sort_by_key(|z| z.start_ms);
    d
}

/// Zoom ersetzen (verschieben, Stärke, Mitte) oder mit `None` löschen.
pub fn set_zoom(doc: &VideoDoc, index: usize, zoom: Option<ZoomSection>) -> VideoDoc {
    let mut d = doc.clone();
    if index >= d.zooms.len() {
        return d;
    }
    match zoom {
        Some(mut z) => {
            z.end_ms = z.end_ms.min(doc.duration_ms);
            z.scale = z.scale.clamp(1.0, 4.0);
            z.x = z.x.clamp(0.0, 1.0);
            z.y = z.y.clamp(0.0, 1.0);
            if z.end_ms > z.start_ms {
                d.zooms[index] = z;
            }
        }
        None => {
            d.zooms.remove(index);
        }
    }
    d.zooms.sort_by_key(|z| z.start_ms);
    d
}

/// Ebene nur im Bereich (Ausgabe-Zeit) zeigen; `None` = immer.
pub fn set_layer_time(doc: &VideoDoc, layer: u32, range: Option<(f64, f64)>) -> VideoDoc {
    let mut d = doc.clone();
    d.layer_times.retain(|t| t.layer != layer);
    if let Some((a, b)) = range {
        let s = source_time(doc, a.min(b)).floor() as u32;
        let e = source_time(doc, a.max(b)).ceil() as u32;
        if e > s {
            d.layer_times.push(LayerTime {
                layer,
                start_ms: s,
                end_ms: e,
            });
        }
    }
    d
}

/// Ist die Ebene zur Quellzeit `t` zu sehen?
pub fn layer_visible(doc: &VideoDoc, layer: u32, source_ms: f64) -> bool {
    doc.layer_times
        .iter()
        .find(|t| t.layer == layer)
        .is_none_or(|t| source_ms >= t.start_ms as f64 && source_ms < t.end_ms as f64)
}

#[cfg(test)]
mod tests {
    use fmshot_core::{WebcamLayout, WebcamTrack};

    use super::*;
    use crate::timeline::output_time;

    fn doc() -> VideoDoc {
        VideoDoc {
            duration_ms: 10_000,
            webcam: Some(WebcamTrack {
                layout: WebcamLayout::default(),
                sections: vec![WebcamSection {
                    start_ms: 0,
                    mode: WebcamMode::Overlay,
                }],
            }),
            ..VideoDoc::default()
        }
    }

    fn modes(d: &VideoDoc) -> Vec<(u32, WebcamMode)> {
        d.webcam
            .as_ref()
            .unwrap()
            .sections
            .iter()
            .map(|s| (s.start_ms, s.mode))
            .collect()
    }

    #[test]
    fn schneiden_ueber_einen_alten_schnitt_hinweg() {
        let d = cut(&doc(), 2000.0, 3000.0);
        // Ausgabe 1500–2500 = Quelle 1500–2000 und 3000–3500
        let d = cut(&d, 1500.0, 2500.0);
        assert_eq!(
            d.cuts,
            vec![TimeRange {
                start_ms: 1500,
                end_ms: 3500
            }]
        );
        assert_eq!(output_duration(&d), 8000);
    }

    #[test]
    fn alles_wegschneiden_geht_nicht() {
        let d = cut(&doc(), 0.0, 10_000.0);
        assert!(d.cuts.is_empty());
    }

    #[test]
    fn trimmen_schneidet_vorne_und_hinten() {
        let d = trim(&doc(), 1000.0, 8000.0);
        assert_eq!(output_duration(&d), 7000);
        assert_eq!(source_time(&d, 0.0), 1000.0);
        assert_eq!(output_time(&d, 7999.0), 6999.0);
    }

    #[test]
    fn webcam_gross_in_der_mitte() {
        let d = set_webcam_mode(&doc(), 2000.0, 4000.0, WebcamMode::Large);
        assert_eq!(
            modes(&d),
            vec![
                (0, WebcamMode::Overlay),
                (2000, WebcamMode::Large),
                (4000, WebcamMode::Overlay)
            ]
        );
        // Wieder klein → alles wieder ein Abschnitt
        let d = set_webcam_mode(&d, 2000.0, 4000.0, WebcamMode::Overlay);
        assert_eq!(modes(&d), vec![(0, WebcamMode::Overlay)]);
    }

    #[test]
    fn webcam_aus_bis_zum_ende() {
        let d = set_webcam_mode(&doc(), 5000.0, 10_000.0, WebcamMode::Hidden);
        assert_eq!(
            modes(&d),
            vec![(0, WebcamMode::Overlay), (5000, WebcamMode::Hidden)]
        );
    }

    #[test]
    fn zoom_hinzufuegen_und_loeschen() {
        let d = add_zoom(&doc(), 9000.0, 0.2, 0.3);
        assert_eq!(d.zooms.len(), 1);
        assert_eq!((d.zooms[0].start_ms, d.zooms[0].end_ms), (9000, 10_000));
        let d = set_zoom(&d, 0, None);
        assert!(d.zooms.is_empty());
    }

    #[test]
    fn ebene_nur_zeitweise() {
        let d = set_layer_time(&doc(), 7, Some((1000.0, 2000.0)));
        assert!(!layer_visible(&d, 7, 500.0));
        assert!(layer_visible(&d, 7, 1500.0));
        assert!(layer_visible(&d, 8, 500.0));
        let d = set_layer_time(&d, 7, None);
        assert!(layer_visible(&d, 7, 500.0));
    }
}
