import { describe, expect, it } from "vitest";
import { computePeaks, peakBetween, wavePath } from "./audioWave";

describe("computePeaks", () => {
  it("nimmt je Abschnitt den lautesten Wert aller Kanäle", () => {
    // 1000 Hz, 10 ms je Abschnitt = 10 Werte
    const left = new Float32Array(30);
    const right = new Float32Array(30);
    left[3] = 0.5;
    right[4] = -0.8;
    left[25] = 0.2;
    const peaks = computePeaks([left, right], 1000, 10);
    expect(Array.from(peaks.data)).toEqual([expect.closeTo(0.8), 0, expect.closeTo(0.2)]);
  });
});

describe("peakBetween", () => {
  it("sucht über alle Abschnitte im Bereich, auch rückwärts", () => {
    const peaks = { data: new Float32Array([0.1, 0.9, 0.3]), bucketMs: 10 };
    expect(peakBetween(peaks, 0, 5)).toBeCloseTo(0.1);
    expect(peakBetween(peaks, 25, 0)).toBeCloseTo(0.9);
    expect(peakBetween(peaks, 500, 900)).toBe(0);
  });
});

describe("wavePath", () => {
  it("ergibt einen geschlossenen Pfad", () => {
    const peaks = { data: new Float32Array([1, 1, 1]), bucketMs: 10 };
    const d = wavePath(peaks, 10, 20, 30, (t) => t);
    expect(d.startsWith("M0.0,0.0")).toBe(true);
    expect(d.endsWith("Z")).toBe(true);
  });

  it("leer ohne Breite", () => {
    expect(wavePath({ data: new Float32Array(1), bucketMs: 10 }, 0, 20, 30, (t) => t)).toBe("");
  });
});
