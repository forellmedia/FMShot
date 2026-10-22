// Wellenform der Tonspuren für die Zeitleiste: Spitzenwerte je kurzem
// Abschnitt der Aufnahme, gezeichnet in Zeit des fertigen Videos.

/** Spitzenwerte (0–1) je `bucketMs` der Aufnahme. */
export type Peaks = { data: Float32Array; bucketMs: number };

export const PEAK_BUCKET_MS = 20;

/** Lauteste Stelle je Abschnitt über alle Kanäle. */
export function computePeaks(channels: Float32Array[], sampleRate: number, bucketMs = PEAK_BUCKET_MS): Peaks {
  const length = channels[0]?.length ?? 0;
  const per = Math.max(1, Math.round((sampleRate * bucketMs) / 1000));
  const data = new Float32Array(Math.ceil(length / per));
  for (const ch of channels) {
    for (let i = 0; i < length; i++) {
      const v = Math.abs(ch[i]);
      const b = (i / per) | 0;
      if (v > data[b]) data[b] = v;
    }
  }
  return { data, bucketMs };
}

/** Grösster Spitzenwert zwischen zwei Zeiten der Aufnahme. */
export function peakBetween(peaks: Peaks, fromMs: number, toMs: number): number {
  const a = Math.max(0, Math.floor(Math.min(fromMs, toMs) / peaks.bucketMs));
  const b = Math.min(peaks.data.length - 1, Math.floor(Math.max(fromMs, toMs) / peaks.bucketMs));
  let max = 0;
  for (let i = a; i <= b; i++) if (peaks.data[i] > max) max = peaks.data[i];
  return max;
}

/**
 * Umriss der Wellenform als SVG-Pfad (mittig, nach oben und unten gespiegelt).
 * `sourceAt` rechnet Ausgabe- in Aufnahme-Zeit um; Schnitte fallen so weg.
 */
export function wavePath(
  peaks: Peaks,
  width: number,
  height: number,
  durationMs: number,
  sourceAt: (outputMs: number) => number,
  step = 2,
): string {
  if (width <= 0 || durationMs <= 0) return "";
  const mid = height / 2;
  const top: string[] = [];
  const bottom: string[] = [];
  for (let x = 0; x <= width; x += step) {
    const t0 = (x / width) * durationMs;
    const t1 = (Math.min(width, x + step) / width) * durationMs;
    // Lautes Material sichtbar machen, leises nicht ganz verschwinden lassen.
    const p = Math.sqrt(peakBetween(peaks, sourceAt(t0), sourceAt(t1)));
    const h = Math.max(0.5, p * mid);
    top.push(`${x.toFixed(1)},${(mid - h).toFixed(1)}`);
    bottom.push(`${x.toFixed(1)},${(mid + h).toFixed(1)}`);
  }
  return `M${top.join("L")}L${bottom.reverse().join("L")}Z`;
}
