//! Zeitachse mit Schnitten (Etappe 4): Alles im Dokument steht in Zeit der
//! Aufnahme („Quelle“). Das fertige Video lässt die Schnitte weg („Ausgabe“).
//! Editor und Export rechnen hiermit zwischen beiden um.

use fmshot_core::{TimeRange, VideoDoc};

/// Bleibende Stücke der Aufnahme (Quelle), sortiert.
pub fn kept(doc: &VideoDoc) -> Vec<TimeRange> {
    let total = doc.duration_ms;
    let mut out = Vec::new();
    let mut start = 0;
    for cut in normalized(&doc.cuts, total) {
        if cut.start_ms > start {
            out.push(TimeRange {
                start_ms: start,
                end_ms: cut.start_ms,
            });
        }
        start = start.max(cut.end_ms);
    }
    if start < total {
        out.push(TimeRange {
            start_ms: start,
            end_ms: total,
        });
    }
    out
}

/// Länge des fertigen Videos.
pub fn output_duration(doc: &VideoDoc) -> u32 {
    kept(doc).iter().map(|r| r.end_ms - r.start_ms).sum()
}

/// Ausgabe → Quelle. Am Ende eines Stücks gilt der Anfang des nächsten.
pub fn source_time(doc: &VideoDoc, output_ms: f64) -> f64 {
    let mut rest = output_ms.max(0.0);
    let pieces = kept(doc);
    for (i, r) in pieces.iter().enumerate() {
        let len = (r.end_ms - r.start_ms) as f64;
        if rest < len || i + 1 == pieces.len() {
            return r.start_ms as f64 + rest.min(len);
        }
        rest -= len;
    }
    0.0
}

/// Quelle → Ausgabe. In einem Schnitt: die Stelle direkt danach.
pub fn output_time(doc: &VideoDoc, source_ms: f64) -> f64 {
    let mut out = 0.0;
    for r in kept(doc) {
        if source_ms < r.start_ms as f64 {
            return out;
        }
        if source_ms < r.end_ms as f64 {
            return out + source_ms - r.start_ms as f64;
        }
        out += (r.end_ms - r.start_ms) as f64;
    }
    out
}

/// Liegt die Quellzeit in einem Schnitt? Dann dessen Ende.
pub fn skip_cut(doc: &VideoDoc, source_ms: f64) -> Option<f64> {
    normalized(&doc.cuts, doc.duration_ms)
        .into_iter()
        .find(|c| source_ms >= c.start_ms as f64 && source_ms < c.end_ms as f64)
        .map(|c| c.end_ms as f64)
}

/// Schnitte sortiert, zusammengelegt und auf die Länge begrenzt.
pub fn normalized(cuts: &[TimeRange], total: u32) -> Vec<TimeRange> {
    let mut v: Vec<TimeRange> = cuts
        .iter()
        .map(|c| TimeRange {
            start_ms: c.start_ms.min(total),
            end_ms: c.end_ms.min(total),
        })
        .filter(|c| c.end_ms > c.start_ms)
        .collect();
    v.sort_by_key(|c| c.start_ms);
    let mut out: Vec<TimeRange> = Vec::with_capacity(v.len());
    for c in v {
        match out.last_mut() {
            Some(last) if c.start_ms <= last.end_ms => last.end_ms = last.end_ms.max(c.end_ms),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn doc(cuts: &[(u32, u32)]) -> VideoDoc {
        VideoDoc {
            duration_ms: 10_000,
            cuts: cuts
                .iter()
                .map(|&(start_ms, end_ms)| TimeRange { start_ms, end_ms })
                .collect(),
            ..VideoDoc::default()
        }
    }

    #[test]
    fn ohne_schnitt_ist_alles_gleich() {
        let d = doc(&[]);
        assert_eq!(output_duration(&d), 10_000);
        assert_eq!(source_time(&d, 1234.0), 1234.0);
        assert_eq!(output_time(&d, 1234.0), 1234.0);
    }

    #[test]
    fn schnitte_werden_uebersprungen() {
        // Anfang getrimmt, Mitte geschnitten
        let d = doc(&[(0, 1000), (4000, 6000)]);
        assert_eq!(output_duration(&d), 7000);
        assert_eq!(source_time(&d, 0.0), 1000.0);
        assert_eq!(source_time(&d, 2999.0), 3999.0);
        assert_eq!(source_time(&d, 3000.0), 6000.0);
        assert_eq!(output_time(&d, 6500.0), 3500.0);
        // im Schnitt: Stelle danach
        assert_eq!(output_time(&d, 5000.0), 3000.0);
        assert_eq!(skip_cut(&d, 4500.0), Some(6000.0));
        assert_eq!(skip_cut(&d, 3500.0), None);
    }

    #[test]
    fn ueberlappende_schnitte_werden_zusammengelegt() {
        let n = normalized(
            &[
                TimeRange {
                    start_ms: 5000,
                    end_ms: 7000,
                },
                TimeRange {
                    start_ms: 2000,
                    end_ms: 5500,
                },
                TimeRange {
                    start_ms: 9000,
                    end_ms: 20_000,
                },
            ],
            10_000,
        );
        assert_eq!(
            n,
            vec![
                TimeRange {
                    start_ms: 2000,
                    end_ms: 7000
                },
                TimeRange {
                    start_ms: 9000,
                    end_ms: 10_000
                }
            ]
        );
    }

    #[test]
    fn ende_der_ausgabe_bleibt_im_letzten_stueck() {
        let d = doc(&[(8000, 10_000)]);
        assert_eq!(source_time(&d, 9000.0), 8000.0);
    }
}
