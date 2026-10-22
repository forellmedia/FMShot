import { describe, expect, it } from "vitest";
import { FPS_CHOICES, nextFps } from "./fps";

describe("nextFps", () => {
  it("geht der Reihe nach durch und beginnt wieder vorne", () => {
    expect(nextFps(24)).toBe(25);
    expect(nextFps(30)).toBe(50);
    expect(nextFps(60)).toBe(24);
  });

  it("startet bei einem unbekannten Wert mit der ersten Bildrate", () => {
    expect(nextFps(48)).toBe(FPS_CHOICES[0]);
  });
});
