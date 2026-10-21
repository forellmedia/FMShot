//! Stichwörter vergeben und nach Bildinhalt suchen – auf fertigen Vektoren,
//! ohne Modell testbar. Schwellen geeicht mit 32 Testbildern und 64
//! Aufnahmen aus dem Alltag (ADR 0010).

use crate::terms::{NEUTRAL, Scope, TERMS};
use crate::{DIM, Embedding, ImageVectors, MODEL_ID, TextEncoder, VisionError};

/// SigLIP rechnet Ähnlichkeit in „Logits“ um: `SCALE · cos + BIAS`
/// (aus `google/siglip2-base-patch16-224`). 0 entspricht 50 % Treffer.
const SCALE: f32 = 112.680_67;
const BIAS: f32 = -16.771_725;

/// Art des Bildes (nur ganzes Bild) als Stichwort, wenn sie mindestens so
/// gut passt …
const KIND_MIN: f32 = -6.5;
/// … und nicht weiter als das hinter der besten Art liegt.
const KIND_SPREAD: f32 = 2.0;
const KIND_MAX: usize = 2;

/// Dinge im Bild (bester Ausschnitt) als Stichwort: eigene Schwellen, damit
/// „Webshop“ nicht „Mann“ und „Schuh“ verdrängt.
const THING_MIN: f32 = -6.0;
const THING_SPREAD: f32 = 4.0;
const THING_MAX: usize = 6;

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
        feed(&format!("{:?}", term.scope()));
    }
    feed(&format!(
        "{KIND_MIN}/{KIND_SPREAD}/{KIND_MAX}/{THING_MIN}/{THING_SPREAD}/{THING_MAX}"
    ));
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

    /// Stichwörter (Schlüssel, Logit): erst Dinge im Bild, dann die Art des
    /// Bildes, jeweils bester zuerst.
    pub fn tags(&self, image: &ImageVectors) -> Vec<(&'static str, f32)> {
        // Logits je Ansicht (ganzes Bild zuerst) und Begriff.
        let logits: Vec<Vec<f32>> = image
            .all()
            .map(|v| self.terms.iter().map(|t| logit(v, t)).collect())
            .collect();
        let best_kind: Vec<f32> = logits
            .iter()
            .map(|row| {
                TERMS
                    .iter()
                    .zip(row)
                    .filter(|(t, _)| t.scope() == Scope::Kind)
                    .fold(f32::MIN, |m, (_, &l)| m.max(l))
            })
            .collect();
        let (mut kinds, mut things) = (Vec::new(), Vec::new());
        for (i, term) in TERMS.iter().enumerate() {
            let views = logits
                .iter()
                .zip(&best_kind)
                .map(|(row, &kind)| (row[i], kind));
            match term.scope() {
                Scope::Kind => kinds.push((term.key, logits[0][i])),
                Scope::Thing => {
                    things.push((term.key, views.map(|v| v.0).fold(f32::MIN, f32::max)))
                }
                Scope::Device => things.push((
                    term.key,
                    views
                        .filter(|&(l, kind)| l > kind)
                        .fold(f32::MIN, |m, (l, _)| m.max(l)),
                )),
            }
        }
        let mut tags = pick(things, THING_MIN, THING_SPREAD, THING_MAX);
        tags.extend(pick(kinds, KIND_MIN, KIND_SPREAD, KIND_MAX));
        tags
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

/// Die besten Begriffe: mindestens `min`, höchstens `spread` hinter dem
/// besten, höchstens `max` Stück.
fn pick(
    mut scored: Vec<(&'static str, f32)>,
    min: f32,
    spread: f32,
    max: usize,
) -> Vec<(&'static str, f32)> {
    scored.sort_by(|a, b| b.1.total_cmp(&a.1));
    let best = scored.first().map_or(f32::MIN, |s| s.1);
    scored
        .into_iter()
        .take_while(|&(_, l)| l >= min && l >= best - spread)
        .take(max)
        .collect()
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

    fn whole(parts: &[(usize, f32)]) -> ImageVectors {
        ImageVectors {
            whole: image_for(parts),
            parts: Vec::new(),
        }
    }

    fn keys(tg: &Tagger, image: &ImageVectors) -> Vec<&'static str> {
        tg.tags(image).into_iter().map(|t| t.0).collect()
    }

    /// Achse des Begriffs `key`.
    fn axis(key: &str) -> usize {
        10 + TERMS.iter().position(|t| t.key == key).unwrap()
    }

    #[test]
    fn dinge_nur_nahe_am_besten_und_hoechstens_sechs() {
        let tg = tagger();
        let (man, dog, tree, car) = (axis("man"), axis("dog"), axis("tree"), axis("car"));
        let img = whole(&[(man, -2.0), (dog, -4.0), (tree, -5.5), (car, -6.5)]);
        assert_eq!(keys(&tg, &img), vec!["man", "dog", "tree"]);

        let weit = whole(&[(man, 1.0), (dog, -2.5), (tree, -5.0)]);
        assert_eq!(keys(&tg, &weit), vec!["man", "dog"]);

        let viele: Vec<_> = ["man", "dog", "tree", "car", "cat", "bird", "boat", "pizza"]
            .iter()
            .enumerate()
            .map(|(i, k)| (axis(k), -1.0 - i as f32 * 0.1))
            .collect();
        assert_eq!(tg.tags(&whole(&viele)).len(), 6);

        assert!(tg.tags(&whole(&[(man, -7.0)])).is_empty());
    }

    #[test]
    fn dinge_auch_im_ausschnitt_die_art_nur_im_ganzen_bild() {
        let tg = tagger();
        let (shop, code, man) = (axis("shop"), axis("code"), axis("man"));
        // Ganzes Bild: Webshop. Im Ausschnitt: ein Mann, daneben Quellcode.
        let img = ImageVectors {
            whole: image_for(&[(shop, 0.0), (man, -7.5)]),
            parts: vec![image_for(&[(man, -3.0), (code, -1.0)])],
        };
        assert_eq!(keys(&tg, &img), vec!["man", "shop"]);
    }

    #[test]
    fn geraet_nur_wo_es_besser_passt_als_jede_bildart() {
        let tg = tagger();
        let (computer, desktop) = (axis("computer"), axis("desktop"));
        let bildschirmfoto = whole(&[(desktop, -1.0), (computer, -3.0)]);
        assert_eq!(keys(&tg, &bildschirmfoto), vec!["desktop"]);
        let foto = whole(&[(desktop, -8.0), (computer, -3.0)]);
        assert_eq!(keys(&tg, &foto), vec!["computer"]);
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
