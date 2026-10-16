// Auto-Schwärzen: erkennt sensible Stellen in OCR-Text und liefert die
// Rechtecke dazu (Dokument-Pixel der Originalaufnahme).

import type { OcrBlock } from "$lib/api";
import type { Rect } from "./model";

const PATTERNS: RegExp[] = [
  // E-Mail
  /[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}/g,
  // IBAN (mit oder ohne Leerzeichen)
  /\b[A-Z]{2}\d{2}(?:[ ]?[A-Z0-9]{4}){2,7}(?:[ ]?[A-Z0-9]{1,3})?\b/g,
  // Kreditkarte (13–19 Ziffern, Gruppen)
  /\b(?:\d[ -]?){13,19}\b/g,
  // Telefon (international oder mit Vorwahl)
  /(?:\+|00)\d{1,3}[ /-]?(?:\(?\d{1,4}\)?[ /-]?){2,5}\d{2,4}/g,
  /\b0\d{2,3}[ /-]?\d{3}[ /-]?\d{2}[ /-]?\d{2}\b/g,
  // Zugangsschlüssel / Tokens (bekannte Präfixe und lange Zufallsfolgen)
  /\b(?:sk|pk|ghp|gho|github_pat|xox[abp]|AKIA)[A-Za-z0-9_-]{8,}\b/g,
  /\b[A-Za-z0-9_\-]{32,}\b/g,
];

/** Bereiche aller Treffer – horizontal anteilig aus der Zeichenposition geschätzt. */
export function findSensitive(blocks: OcrBlock[]): Rect[] {
  const rects: Rect[] = [];
  for (const b of blocks) {
    const n = Math.max(1, b.text.length);
    const taken: [number, number][] = [];
    for (const re of PATTERNS) {
      re.lastIndex = 0;
      for (let m = re.exec(b.text); m; m = re.exec(b.text)) {
        const start = m.index;
        const end = start + m[0].length;
        if (m[0].replace(/\D/g, "").length < 6 && /\d/.test(m[0]) && !m[0].includes("@")) continue;
        if (taken.some(([s, e]) => start < e && end > s)) continue;
        taken.push([start, end]);
        const pad = b.height * 0.15;
        rects.push({
          x: b.x + (b.width * start) / n - pad,
          y: b.y - pad,
          width: (b.width * (end - start)) / n + 2 * pad,
          height: b.height + 2 * pad,
        });
      }
    }
  }
  return rects;
}
