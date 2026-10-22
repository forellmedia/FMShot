//! Lautstärke über die Zeit und Ein-/Ausblenden. Editor-Vorschau und Export
//! rechnen beide hiermit.
//!
//! Lautstärke-Punkte stehen in Zeit der Aufnahme (wie alles im Dokument);
//! das Ein- und Ausblenden gilt am Anfang und Ende des fertigen Videos.

use grabmark_core::{VideoDoc, VolumeKey};

use crate::timeline;

/// Die beiden Tonspuren einer Aufnahme.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AudioTrack {
    System,
    Microphone,
}

impl AudioTrack {
    pub fn parse(name: &str) -> Option<Self> {
        match name {
            "system" => Some(Self::System),
            "mic" | "microphone" => Some(Self::Microphone),
            _ => None,
        }
    }

    /// Lautstärke der ganzen Spur (0–1).
    pub fn base(self, doc: &VideoDoc) -> f32 {
        match self {
            Self::System => doc.audio.system,
            Self::Microphone => doc.audio.microphone,
        }
        .clamp(0.0, 1.0)
    }

    pub fn keys(self, doc: &VideoDoc) -> &[VolumeKey] {
        match self {
            Self::System => &doc.audio.system_keys,
            Self::Microphone => &doc.audio.microphone_keys,
        }
    }

    pub fn keys_mut(self, doc: &mut VideoDoc) -> &mut Vec<VolumeKey> {
        match self {
            Self::System => &mut doc.audio.system_keys,
            Self::Microphone => &mut doc.audio.microphone_keys,
        }
    }
}

/// Faktor der Lautstärke-Punkte zur Quellzeit (ohne Punkte: 1).
pub fn envelope(keys: &[VolumeKey], source_ms: f64) -> f32 {
    let (Some(first), Some(last)) = (keys.first(), keys.last()) else {
        return 1.0;
    };
    if source_ms <= first.t_ms as f64 {
        return first.volume.clamp(0.0, 1.0);
    }
    if source_ms >= last.t_ms as f64 {
        return last.volume.clamp(0.0, 1.0);
    }
    for pair in keys.windows(2) {
        let (a, b) = (pair[0], pair[1]);
        if source_ms <= b.t_ms as f64 {
            let span = (b.t_ms - a.t_ms) as f64;
            let t = if span > 0.0 {
                ((source_ms - a.t_ms as f64) / span) as f32
            } else {
                1.0
            };
            return (a.volume + (b.volume - a.volume) * t).clamp(0.0, 1.0);
        }
    }
    last.volume.clamp(0.0, 1.0)
}

/// Lautstärke einer Spur zur Quellzeit: ganze Spur × Punkte (ohne Blenden).
pub fn volume_at(doc: &VideoDoc, track: AudioTrack, source_ms: f64) -> f32 {
    track.base(doc) * envelope(track.keys(doc), source_ms)
}

/// Länge von Ein- und Ausblenden in ms. Zusammen nie länger als das Video.
pub fn fade_ms(doc: &VideoDoc) -> (f64, f64) {
    let per_frame = 1000.0 / doc.fps.max(1) as f64;
    let total = timeline::output_duration(doc) as f64;
    let fade_in = doc.fade.in_frames as f64 * per_frame;
    let fade_out = doc.fade.out_frames as f64 * per_frame;
    let sum = fade_in + fade_out;
    if sum > total && sum > 0.0 {
        let k = total / sum;
        (fade_in * k, fade_out * k)
    } else {
        (fade_in, fade_out)
    }
}

/// Deckkraft des Bilds bzw. Faktor des Tons durch die Blenden (0–1) zur
/// Zeit des fertigen Videos.
pub fn fade_at(doc: &VideoDoc, output_ms: f64) -> f32 {
    let (fade_in, fade_out) = fade_ms(doc);
    let total = timeline::output_duration(doc) as f64;
    let mut k: f64 = 1.0;
    if fade_in > 0.0 {
        k = k.min(output_ms / fade_in);
    }
    if fade_out > 0.0 {
        k = k.min((total - output_ms) / fade_out);
    }
    k.clamp(0.0, 1.0) as f32
}

/// Lautstärke, die man zur Zeit des fertigen Videos hört (mit Blenden).
pub fn gain_at(doc: &VideoDoc, track: AudioTrack, output_ms: f64) -> f32 {
    let source = timeline::source_time(doc, output_ms);
    volume_at(doc, track, source) * fade_at(doc, output_ms)
}

/// Lineare Änderung der Lautstärke im fertigen Video (für den Export).
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct VolumeRamp {
    pub start_ms: f64,
    pub end_ms: f64,
    pub from: f32,
    pub to: f32,
}

/// Rampen für den Export – leer, solange es weder Punkte noch Blenden gibt
/// (dann gilt die Lautstärke der ganzen Spur).
pub fn export_ramps(doc: &VideoDoc, track: AudioTrack) -> Vec<VolumeRamp> {
    if track.keys(doc).is_empty() && doc.fade.is_none() {
        Vec::new()
    } else {
        volume_ramps(doc, track)
    }
}

/// So fein werden Blenden in Rampen zerlegt, wenn in ihnen auch Punkte
/// liegen (das Produkt zweier Geraden ist keine Gerade mehr).
const FADE_STEPS: usize = 8;

/// Lautstärke einer Spur als Folge linearer Rampen in Zeit des fertigen
/// Videos – lückenlos von 0 bis zum Ende. An Schnitten springt der Wert.
pub fn volume_ramps(doc: &VideoDoc, track: AudioTrack) -> Vec<VolumeRamp> {
    let total = timeline::output_duration(doc) as f64;
    if total <= 0.0 {
        return Vec::new();
    }
    let (fade_in, fade_out) = fade_ms(doc);
    let keys = track.keys(doc);
    let mut ramps = Vec::new();
    let mut out_start = 0.0;
    for piece in timeline::kept(doc) {
        let len = (piece.end_ms - piece.start_ms) as f64;
        let out_end = out_start + len;
        // Stellen, an denen die Kurve abknickt: Punkte im Stück, Blenden.
        let mut cuts = vec![out_start, out_end];
        cuts.extend(
            keys.iter()
                .map(|k| k.t_ms as f64)
                .filter(|&t| t > piece.start_ms as f64 && t < piece.end_ms as f64)
                .map(|t| out_start + t - piece.start_ms as f64),
        );
        for (a, b) in [(0.0, fade_in), (total - fade_out, total)] {
            if b > a {
                cuts.extend((0..=FADE_STEPS).map(|i| a + (b - a) * i as f64 / FADE_STEPS as f64));
            }
        }
        cuts.retain(|&t| t >= out_start && t <= out_end);
        cuts.sort_by(f64::total_cmp);
        cuts.dedup_by(|a, b| (*a - *b).abs() < 0.5);
        let value = |out: f64| {
            let source = piece.start_ms as f64 + (out - out_start).clamp(0.0, len);
            volume_at(doc, track, source) * fade_at(doc, out)
        };
        for pair in cuts.windows(2) {
            ramps.push(VolumeRamp {
                start_ms: pair[0],
                end_ms: pair[1],
                from: value(pair[0]),
                to: value(pair[1]),
            });
        }
        out_start = out_end;
    }
    ramps
}

/// Lautstärke-Punkt setzen (Zeit des fertigen Videos). `index` = einen
/// vorhandenen Punkt verschieben, sonst neu. Gibt das neue Dokument und die
/// Stelle des Punkts in der sortierten Liste zurück.
pub fn set_volume_key(
    doc: &VideoDoc,
    track: AudioTrack,
    index: Option<usize>,
    output_ms: f64,
    volume: f32,
) -> (VideoDoc, usize) {
    let mut out = doc.clone();
    let t_ms = timeline::source_time(doc, output_ms)
        .round()
        .clamp(0.0, doc.duration_ms as f64) as u32;
    let key = VolumeKey {
        t_ms,
        volume: volume.clamp(0.0, 1.0),
    };
    let keys = track.keys_mut(&mut out);
    if let Some(i) = index.filter(|&i| i < keys.len()) {
        keys.remove(i);
    }
    // Ein Punkt an fast derselben Stelle wird ersetzt.
    keys.retain(|k| k.t_ms.abs_diff(t_ms) > 1);
    let at = keys.partition_point(|k| k.t_ms < t_ms);
    keys.insert(at, key);
    (out, at)
}

/// Lautstärke-Punkt löschen.
pub fn remove_volume_key(doc: &VideoDoc, track: AudioTrack, index: usize) -> VideoDoc {
    let mut out = doc.clone();
    let keys = track.keys_mut(&mut out);
    if index < keys.len() {
        keys.remove(index);
    }
    out
}

/// Ein- bzw. Ausblenden setzen (Bilder, 0 = aus); `None` lässt es, wie es ist.
pub fn set_fade(doc: &VideoDoc, fade_in: Option<u32>, fade_out: Option<u32>) -> VideoDoc {
    let mut out = doc.clone();
    if let Some(f) = fade_in {
        out.fade.in_frames = f;
    }
    if let Some(f) = fade_out {
        out.fade.out_frames = f;
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use grabmark_core::{Fade, TimeRange};

    fn key(t_ms: u32, volume: f32) -> VolumeKey {
        VolumeKey { t_ms, volume }
    }

    fn doc() -> VideoDoc {
        VideoDoc {
            duration_ms: 10_000,
            fps: 25,
            ..VideoDoc::default()
        }
    }

    #[test]
    fn ohne_punkte_volle_lautstaerke() {
        let d = doc();
        assert_eq!(volume_at(&d, AudioTrack::System, 5000.0), 1.0);
        assert_eq!(fade_at(&d, 0.0), 1.0);
    }

    #[test]
    fn punkte_werden_linear_verbunden() {
        let keys = [key(1000, 1.0), key(3000, 0.0)];
        assert_eq!(envelope(&keys, 0.0), 1.0);
        assert_eq!(envelope(&keys, 2000.0), 0.5);
        assert_eq!(envelope(&keys, 9000.0), 0.0);
    }

    #[test]
    fn spur_lautstaerke_mal_punkte() {
        let mut d = doc();
        d.audio.microphone = 0.5;
        d.audio.microphone_keys = vec![key(0, 0.5)];
        assert_eq!(volume_at(&d, AudioTrack::Microphone, 100.0), 0.25);
        assert_eq!(volume_at(&d, AudioTrack::System, 100.0), 1.0);
    }

    #[test]
    fn blenden_in_bildern() {
        let mut d = doc();
        // 25 Bilder bei 25 fps = 1 s
        d.fade = Fade {
            in_frames: 25,
            out_frames: 50,
        };
        assert_eq!(fade_ms(&d), (1000.0, 2000.0));
        assert_eq!(fade_at(&d, 0.0), 0.0);
        assert_eq!(fade_at(&d, 500.0), 0.5);
        assert_eq!(fade_at(&d, 5000.0), 1.0);
        assert_eq!(fade_at(&d, 9000.0), 0.5);
        assert_eq!(fade_at(&d, 10_000.0), 0.0);
    }

    #[test]
    fn blenden_laenger_als_video_werden_gekuerzt() {
        let mut d = doc();
        d.duration_ms = 1000;
        d.fade = Fade {
            in_frames: 50,
            out_frames: 50,
        };
        assert_eq!(fade_ms(&d), (500.0, 500.0));
    }

    #[test]
    fn rampen_decken_das_ganze_video_ab() {
        let mut d = doc();
        d.cuts = vec![TimeRange {
            start_ms: 4000,
            end_ms: 6000,
        }];
        d.audio.system_keys = vec![key(2000, 1.0), key(8000, 0.0)];
        let ramps = volume_ramps(&d, AudioTrack::System);
        assert_eq!(ramps.first().unwrap().start_ms, 0.0);
        assert_eq!(ramps.last().unwrap().end_ms, 8000.0);
        for pair in ramps.windows(2) {
            assert_eq!(pair[0].end_ms, pair[1].start_ms);
        }
        // Am Schnitt springt die Lautstärke: vor 4000 ⅔, nach 6000 ⅓.
        let at_cut = ramps.iter().find(|r| r.end_ms == 4000.0).unwrap();
        assert!((at_cut.to - 2.0 / 3.0).abs() < 1e-4);
        let after = ramps.iter().find(|r| r.start_ms == 4000.0).unwrap();
        assert!((after.from - 1.0 / 3.0).abs() < 1e-4);
    }

    #[test]
    fn rampen_mit_blende() {
        let mut d = doc();
        d.fade.in_frames = 25;
        let ramps = volume_ramps(&d, AudioTrack::Microphone);
        assert_eq!(ramps[0].from, 0.0);
        let end_of_fade = ramps.iter().find(|r| r.end_ms == 1000.0).unwrap();
        assert_eq!(end_of_fade.to, 1.0);
    }

    #[test]
    fn punkte_setzen_verschieben_loeschen() {
        let d = doc();
        let (d, i) = set_volume_key(&d, AudioTrack::System, None, 5000.0, 0.2);
        assert_eq!(i, 0);
        let (d, i) = set_volume_key(&d, AudioTrack::System, None, 1000.0, 0.8);
        assert_eq!(i, 0);
        assert_eq!(d.audio.system_keys.len(), 2);
        // Ersten Punkt hinter den zweiten schieben: Liste bleibt sortiert.
        let (d, i) = set_volume_key(&d, AudioTrack::System, Some(0), 7000.0, 1.5);
        assert_eq!(i, 1);
        assert_eq!(d.audio.system_keys[1], key(7000, 1.0));
        let d = remove_volume_key(&d, AudioTrack::System, 0);
        assert_eq!(d.audio.system_keys, vec![key(7000, 1.0)]);
        assert!(d.audio.microphone_keys.is_empty());
    }
}
