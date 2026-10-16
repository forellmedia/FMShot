//! Setzt die Bildfolge einer Scroll-Aufnahme zu einem langen Bild zusammen
//! (ADR 0006). Plattformunabhängig und ohne Bildschirm testbar.
//!
//! Vorgehen pro neuem Bild:
//! 1. Feste Kopf- und Fusszeilen erkennen (Zeilen, die zwischen zwei Bildern
//!    an derselben Stelle gleich bleiben – z. B. eine fixierte Navigation).
//! 2. Den Versatz suchen, um den der Inhalt dazwischen nach oben gewandert
//!    ist: Zeilen werden gehasht, jede Übereinstimmung stimmt für einen
//!    Versatz. Einfarbige Zeilen (Leerraum) stimmen nicht mit, weil sie überall
//!    passen würden.
//! 3. Nur die neu sichtbaren Zeilen anhängen.

use std::collections::HashMap;

use fmshot_core::RgbaImage;

/// Mindestanteil übereinstimmender Zeilen, damit ein Versatz gilt.
const MIN_AGREEMENT: f32 = 0.5;
/// Mindestzahl „aussagekräftiger“ Zeilen für einen Versatz.
const MIN_VOTES: usize = 8;

/// Ergebnis eines neuen Bildes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Push {
    /// Erstes Bild.
    Started,
    /// Inhalt ist um so viele Zeilen weitergewandert; angehängt.
    Appended(u32),
    /// Nichts hat sich bewegt (oder nach oben gescrollt).
    Unchanged,
    /// Kein passender Versatz gefunden (zu schnell gescrollt?).
    NoMatch,
}

pub struct Stitcher {
    width: u32,
    height: u32,
    /// Bisheriges Ergebnis ohne Fusszeile.
    body: Vec<u8>,
    /// Letztes Bild – Vergleichsbasis.
    last: Option<Frame>,
    header: u32,
    footer: u32,
    max_height: u32,
}

struct Frame {
    image: RgbaImage,
    hashes: Vec<u64>,
    /// Zeile hat Inhalt (nicht einfarbig)?
    textured: Vec<bool>,
}

impl Frame {
    fn new(image: RgbaImage) -> Self {
        let (hashes, textured) = (0..image.height())
            .map(|y| row_signature(&image, y))
            .unzip();
        Self {
            image,
            hashes,
            textured,
        }
    }
}

/// Hash einer Zeile auf leicht vergröberten Helligkeiten (verzeiht minimale
/// Farbunterschiede) und ob die Zeile Inhalt hat.
fn row_signature(image: &RgbaImage, y: u32) -> (u64, bool) {
    let w = image.width() as usize;
    let row = &image.data()[y as usize * w * 4..(y as usize + 1) * w * 4];
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    let (mut min, mut max) = (u8::MAX, 0u8);
    for px in row.as_chunks::<4>().0 {
        let l = ((px[0] as u32 * 54 + px[1] as u32 * 183 + px[2] as u32 * 19) >> 8) as u8;
        let q = l >> 2;
        min = min.min(q);
        max = max.max(q);
        h ^= q as u64;
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    (h, max.saturating_sub(min) > 2)
}

impl Stitcher {
    /// `max_height`: Abbruchgrenze in Pixeln, damit der Speicher nicht überläuft.
    pub fn new(max_height: u32) -> Self {
        Self {
            width: 0,
            height: 0,
            body: Vec::new(),
            last: None,
            header: 0,
            footer: 0,
            max_height,
        }
    }

    /// Bisherige Höhe des Ergebnisses in Pixeln.
    pub fn height(&self) -> u32 {
        let rows = self.body.len() / (self.width as usize * 4).max(1);
        rows as u32 + self.footer
    }

    pub fn is_full(&self) -> bool {
        self.height() >= self.max_height
    }

    pub fn push(&mut self, image: RgbaImage) -> Push {
        let frame = Frame::new(image);
        let Some(last) = self.last.take() else {
            self.width = frame.image.width();
            self.height = frame.image.height();
            self.body = frame.image.data().to_vec();
            self.last = Some(frame);
            return Push::Started;
        };
        if frame.image.width() != self.width || frame.image.height() != self.height {
            self.last = Some(last);
            return Push::NoMatch;
        }

        // Feste Kopf-/Fusszeilen: gleiche Zeilen an gleicher Stelle. Nur wenn
        // sich dazwischen etwas bewegt hat – sonst ist alles „fest“.
        let h = self.height as usize;
        let same_top = (0..h)
            .take_while(|&y| last.hashes[y] == frame.hashes[y])
            .count();
        if same_top == h {
            self.last = Some(last);
            return Push::Unchanged;
        }
        let same_bottom = (0..h)
            .rev()
            .take_while(|&y| last.hashes[y] == frame.hashes[y])
            .count();
        // Kopf/Fuss nur übernehmen, wenn sie klein genug sind (≤ 40 %).
        let header = if same_top * 5 <= h * 2 { same_top } else { 0 };
        let footer = if same_bottom * 5 <= h * 2 {
            same_bottom
        } else {
            0
        };
        if self.header == 0 {
            self.header = header as u32;
        }
        let footer = footer.max(self.footer as usize);

        let Some(offset) = find_offset(&last, &frame, self.header as usize, footer) else {
            self.last = Some(last);
            return Push::NoMatch;
        };
        if offset == 0 {
            self.last = Some(last);
            return Push::Unchanged;
        }

        // Bisherige Fusszeile entfernen, neue Zeilen anhängen, Fusszeile merken.
        let row_bytes = self.width as usize * 4;
        if footer as u32 > self.footer {
            let cut = (footer - self.footer as usize) * row_bytes;
            let len = self.body.len().saturating_sub(cut);
            self.body.truncate(len);
            self.footer = footer as u32;
        }
        let content_end = h - footer;
        let new_from = content_end - offset;
        let data = frame.image.data();
        self.body
            .extend_from_slice(&data[new_from * row_bytes..content_end * row_bytes]);
        self.last = Some(frame);
        Push::Appended(offset as u32)
    }

    /// Fertiges Bild (mit Fusszeile des letzten Bildes).
    pub fn finish(self) -> Option<RgbaImage> {
        let last = self.last?;
        let row_bytes = self.width as usize * 4;
        let mut data = self.body;
        let h = self.height as usize;
        let footer = self.footer as usize;
        data.extend_from_slice(&last.image.data()[(h - footer) * row_bytes..h * row_bytes]);
        let rows = (data.len() / row_bytes) as u32;
        RgbaImage::new(self.width, rows, data).ok()
    }
}

/// Um wie viele Zeilen ist der Inhalt von `a` nach `b` nach oben gewandert?
fn find_offset(a: &Frame, b: &Frame, header: usize, footer: usize) -> Option<usize> {
    let h = a.hashes.len();
    let (top, bottom) = (header, h - footer);
    if bottom <= top + MIN_VOTES {
        return None;
    }
    // Position(en) jeder Inhalts-Zeile in `a`.
    let mut positions: HashMap<u64, Vec<usize>> = HashMap::new();
    for y in top..bottom {
        if a.textured[y] {
            positions.entry(a.hashes[y]).or_default().push(y);
        }
    }
    // Jede Zeile von `b` stimmt für Versätze, an denen sie in `a` vorkam.
    let mut votes: HashMap<usize, usize> = HashMap::new();
    for y in top..bottom {
        if !b.textured[y] {
            continue;
        }
        if let Some(ps) = positions.get(&b.hashes[y]) {
            // Häufige Zeilen (z. B. Trennlinien) sind wenig aussagekräftig.
            if ps.len() > 3 {
                continue;
            }
            for &py in ps {
                if py >= y {
                    *votes.entry(py - y).or_default() += 1;
                }
            }
        }
    }
    // Kandidaten nach Stimmen; dann den mit der besten Übereinstimmung wählen.
    let mut candidates: Vec<(usize, usize)> = votes.into_iter().collect();
    candidates.sort_by(|x, y| y.1.cmp(&x.1).then(x.0.cmp(&y.0)));
    candidates.into_iter().take(5).find_map(|(offset, n)| {
        if n < MIN_VOTES.min((bottom - top) / 4) {
            return None;
        }
        // Prüfung: Anteil gleicher Inhalts-Zeilen im überlappenden Bereich.
        let mut checked = 0;
        let mut equal = 0;
        for y in top..bottom.saturating_sub(offset) {
            if a.textured[y + offset] || b.textured[y] {
                checked += 1;
                if a.hashes[y + offset] == b.hashes[y] {
                    equal += 1;
                }
            }
        }
        (checked > 0 && equal as f32 / checked as f32 >= MIN_AGREEMENT).then_some(offset)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Lange „Webseite“: Zeilen mit unterschiedlichen Mustern, ab und zu Leerraum.
    fn page(width: u32, height: u32) -> RgbaImage {
        let mut data = Vec::with_capacity((width * height * 4) as usize);
        let mut seed: u32 = 12345;
        for y in 0..height {
            let blank = (y / 40) % 5 == 4; // Leerraum-Blöcke
            for x in 0..width {
                let v = if blank {
                    250
                } else {
                    seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12345);
                    ((seed >> 16) as u8 & 0x7f) + (x as u8 & 0x3f)
                };
                data.extend_from_slice(&[v, v.wrapping_add(y as u8), 200, 255]);
            }
        }
        RgbaImage::new(width, height, data).unwrap()
    }

    /// Ausschnitt ab `scroll` mit fester Kopf- und Fusszeile.
    fn viewport(page: &RgbaImage, scroll: u32, h: u32, header: u32, footer: u32) -> RgbaImage {
        let w = page.width();
        let mut data = Vec::new();
        for y in 0..h {
            if y < header {
                data.extend(
                    std::iter::repeat_n([30u8, 60, 90, 255], w as usize)
                        .flatten()
                        .enumerate()
                        .map(|(i, v)| if i % 8 < 4 { v } else { v / 2 }),
                );
            } else if y >= h - footer {
                data.extend(
                    std::iter::repeat_n([90u8, 30, 60, 255], w as usize)
                        .flatten()
                        .enumerate()
                        .map(|(i, v)| if i % 12 < 4 { v } else { v / 3 }),
                );
            } else {
                let src = (scroll + y) as usize * w as usize * 4;
                data.extend_from_slice(&page.data()[src..src + w as usize * 4]);
            }
        }
        RgbaImage::new(w, h, data).unwrap()
    }

    #[test]
    fn setzt_ohne_kopfzeile_zusammen() {
        let p = page(64, 1200);
        let mut s = Stitcher::new(10_000);
        for scroll in [0, 150, 310, 470, 600] {
            s.push(viewport(&p, scroll, 300, 0, 0));
        }
        let out = s.finish().unwrap();
        assert_eq!(out.height(), 900);
        assert_eq!(out.data(), &p.data()[..900 * 64 * 4]);
    }

    #[test]
    fn feste_kopf_und_fusszeile_erscheinen_nur_einmal() {
        let p = page(64, 2000);
        let (h, header, footer) = (400, 40, 30);
        let mut s = Stitcher::new(10_000);
        let scrolls = [0, 200, 420, 610, 800];
        for scroll in scrolls {
            s.push(viewport(&p, scroll, h, header, footer));
        }
        let out = s.finish().unwrap();
        let content = h - header - footer;
        let expected_h = header + (800 + content) + footer;
        assert_eq!(out.height(), expected_h);
        // Inhalt in der Mitte entspricht der Seite.
        let row = |img: &RgbaImage, y: u32| {
            img.data()[(y * 64 * 4) as usize..((y + 1) * 64 * 4) as usize].to_vec()
        };
        for y in [0, 123, 500, 900] {
            assert_eq!(row(&out, header + y), row(&p, header + y), "Zeile {y}");
        }
    }

    #[test]
    fn stillstand_und_rueckwaerts_aendern_nichts() {
        let p = page(64, 1000);
        let mut s = Stitcher::new(10_000);
        assert_eq!(s.push(viewport(&p, 100, 300, 0, 0)), Push::Started);
        assert_eq!(s.push(viewport(&p, 100, 300, 0, 0)), Push::Unchanged);
        assert_eq!(s.push(viewport(&p, 200, 300, 0, 0)), Push::Appended(100));
        assert!(matches!(
            s.push(viewport(&p, 120, 300, 0, 0)),
            Push::NoMatch | Push::Unchanged
        ));
        assert_eq!(s.finish().unwrap().height(), 400);
    }

    #[test]
    fn zu_weit_gescrollt_wird_erkannt() {
        let p = page(64, 2000);
        let mut s = Stitcher::new(10_000);
        s.push(viewport(&p, 0, 300, 0, 0));
        assert_eq!(s.push(viewport(&p, 900, 300, 0, 0)), Push::NoMatch);
    }

    #[test]
    fn hoehengrenze() {
        let p = page(32, 3000);
        let mut s = Stitcher::new(500);
        s.push(viewport(&p, 0, 300, 0, 0));
        s.push(viewport(&p, 250, 300, 0, 0));
        assert!(s.is_full());
    }
}
