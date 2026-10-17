//! Stichwörter vergeben und nach Bildinhalt suchen – auf fertigen Vektoren,
//! ohne Modell testbar. Schwellen geeicht mit 32 Testbildern (ADR 0010).

use crate::terms::{NEUTRAL, TERMS};
use crate::{DIM, Embedding, MODEL_ID, TextEncoder, VisionError};

/// SigLIP rechnet Ähnlichkeit in „Logits“ um: `SCALE · cos + BIAS`
/// (aus `google/siglip2-base-patch16-224`). 0 entspricht 50 % Treffer.
const SCALE: f32 = 112.680_67;
const BIAS: f32 = -16.771_725;

/// Stichwort, wenn der Begriff mindestens so gut passt …
const TAG_MIN: f32 = -6.5;
/// … und nicht weiter als das hinter dem besten Begriff liegt.
const TAG_SPREAD: f32 = 3.0;
const TAG_MAX: usize = 3;

/// Suchtreffer: mindestens so gut wie absolut …
const SEARCH_MIN: f32 = -8.5;
/// … und so weit über dem Grundwert des Bildes …
const SEARCH_ABOVE_NEUTRAL: f32 = 2.5;
/// … und nicht weiter als das hinter dem besten Treffer.
const SEARCH_SPREAD: f32 = 4.0;

fn logit(a: &Embedding, b: &Embedding) -> f32 {
    SCALE * a.similarity(b) + BIAS
}

/// Kennung der Stichwörter: ändert sich mit Modell, Begriffen oder Schwellen.
/// Gespeicherte Stichwörter mit anderer Kennung werden neu berechnet.
pub fn tags_version() -> String {
    let mut hash: u64 = 0xcbf2_9ce4_8422_2325;
    let mut feed = |s: &str| {
        for b in s.bytes().chain([0]) {
            hash = (hash ^ u64::from(b)).wrapping_mul(0x0100_0000_01b3);
        }
    };
    for term in TERMS {
        feed(term.key);
        feed(term.prompt);
    }
    feed(&format!("{TAG_MIN}/{TAG_SPREAD}/{TAG_MAX}"));
    format!("{MODEL_ID}/{hash:016x}")
}

/// Vektoren der Begriffe und der Füllwörter. Einmal mit dem Textmodell
/// berechnet (rund 2 s), danach als Bytes zwischengespeichert.
pub struct Tagger {
    terms: Vec<Embedding>,
    neutral: Embedding,
}

impl Tagger {
    pub fn build(text: &TextEncoder) -> Result<Self, VisionError> {
        let terms = TERMS
            .iter()
            .map(|t| text.embed(t.prompt))
            .collect::<Result<Vec<_>, _>>()?;
        let mut sum = vec![0.0; DIM];
        for word in NEUTRAL {
            for (s, v) in sum.iter_mut().zip(text.embed(word)?.values()) {
                *s += v;
            }
        }
        Ok(Tagger {
            terms,
            neutral: Embedding::normalized(sum)?,
        })
    }

    /// Bis zu drei Stichwörter (Schlüssel, Logit), bester zuerst.
    pub fn tags(&self, image: &Embedding) -> Vec<(&'static str, f32)> {
        let mut scored: Vec<(&'static str, f32)> = TERMS
            .iter()
            .zip(&self.terms)
            .map(|(t, e)| (t.key, logit(image, e)))
            .collect();
        scored.sort_by(|a, b| b.1.total_cmp(&a.1));
        let best = scored.first().map_or(f32::MIN, |s| s.1);
        scored
            .into_iter()
            .take_while(|&(_, l)| l >= TAG_MIN && l >= best - TAG_SPREAD)
            .take(TAG_MAX)
            .collect()
    }

    /// Bilder, die zum Suchtext passen (ID, Wert), bester zuerst.
    pub fn search<'a>(
        &self,
        query: &Embedding,
        images: impl IntoIterator<Item = (&'a str, &'a Embedding)>,
    ) -> Vec<(&'a str, f32)> {
        let mut hits: Vec<(&'a str, f32)> = images
            .into_iter()
            .filter_map(|(id, image)| {
                let l = logit(query, image);
                let above = l - logit(&self.neutral, image);
                (l >= SEARCH_MIN && above >= SEARCH_ABOVE_NEUTRAL).then_some((id, above))
            })
            .collect();
        hits.sort_by(|a, b| b.1.total_cmp(&a.1));
        let best = hits.first().map_or(0.0, |h| h.1);
        hits.retain(|h| h.1 >= best - SEARCH_SPREAD);
        hits
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        std::iter::once(&self.neutral)
            .chain(&self.terms)
            .flat_map(Embedding::to_bytes)
            .collect()
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, VisionError> {
        let size = DIM * 4;
        if bytes.len() != size * (TERMS.len() + 1) {
            return Err(VisionError::Bytes(bytes.len()));
        }
        let mut all = bytes
            .chunks(size)
            .map(Embedding::from_bytes)
            .collect::<Result<Vec<_>, _>>()?;
        let neutral = all.remove(0);
        Ok(Tagger {
            terms: all,
            neutral,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Einheitsvektor entlang Achse `axis`, mit Anteil `cos` an Achse 0.
    fn unit(axis: usize, cos: f32) -> Embedding {
        let mut v = vec![0.0; DIM];
        v[0] = cos;
        v[axis] = (1.0 - cos * cos).sqrt();
        Embedding::normalized(v).unwrap()
    }

    fn tagger() -> Tagger {
        // Alle Begriffe orthogonal, Füllwörter ebenfalls.
        Tagger {
            terms: (0..TERMS.len()).map(|i| unit(10 + i, 0.0)).collect(),
            neutral: unit(9, 0.0),
        }
    }

    /// Bild, das zu Begriff `i` mit Logit `l` passt.
    fn image_for(parts: &[(usize, f32)]) -> Embedding {
        let mut v = vec![0.0; DIM];
        for &(axis, l) in parts {
            v[axis] = (l - BIAS) / SCALE;
        }
        let rest: f32 = 1.0 - v.iter().map(|x| x * x).sum::<f32>();
        v[DIM - 1] = rest.max(0.0).sqrt();
        Embedding::normalized(v).unwrap()
    }

    #[test]
    fn stichwoerter_nur_nahe_am_besten_und_hoechstens_drei() {
        let tg = tagger();
        let img = image_for(&[(10, -2.0), (11, -4.0), (12, -5.5), (13, -6.0), (14, -9.0)]);
        let keys: Vec<_> = tg.tags(&img).into_iter().map(|t| t.0).collect();
        assert_eq!(keys, vec![TERMS[0].key, TERMS[1].key]);

        let viele = image_for(&[(10, -1.0), (11, -1.1), (12, -1.2), (13, -1.3)]);
        assert_eq!(tg.tags(&viele).len(), 3);

        let nichts = image_for(&[(10, -7.0)]);
        assert!(tg.tags(&nichts).is_empty());
    }

    #[test]
    fn suche_zieht_den_grundwert_ab() {
        let tg = tagger();
        let query = unit(5, 0.0);
        // a passt klar; b passt zu allem (hoher Grundwert); c passt nicht.
        let a = image_for(&[(5, -3.0), (9, -12.0)]);
        let b = image_for(&[(5, -6.0), (9, -5.0)]);
        let c = image_for(&[(5, -11.0)]);
        let hits = tg.search(&query, [("a", &a), ("b", &b), ("c", &c)]);
        assert_eq!(hits.iter().map(|h| h.0).collect::<Vec<_>>(), vec!["a"]);
    }

    #[test]
    fn begriffe_als_bytes_zwischenspeichern() {
        let tg = tagger();
        let back = Tagger::from_bytes(&tg.to_bytes()).unwrap();
        assert_eq!(back.terms, tg.terms);
        assert!(Tagger::from_bytes(&[0; 12]).is_err());
        assert!(tags_version().starts_with(MODEL_ID));
    }
}
