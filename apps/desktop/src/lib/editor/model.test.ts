import { describe, expect, it } from "vitest";
import { bounds, hitTest, rectFromPoints, translate, withBounds, type Layer, type LayerKind } from "./model";

const stroke = { color: { r: 0, g: 0, b: 0, a: 255 }, width: 4 };
const layer = (kind: LayerKind): Layer => ({ id: 1, visible: true, locked: false, opacity: 1, rotation: 0, kind });

describe("Modell", () => {
  it("Rechteck aus zwei Punkten in jede Richtung", () => {
    expect(rectFromPoints({ x: 50, y: 40 }, { x: 10, y: 20 })).toEqual({ x: 10, y: 20, width: 40, height: 20 });
  });

  it("Grenzen von Pfeil und Schritt", () => {
    expect(bounds({ type: "arrow", from: { x: 10, y: 50 }, to: { x: 30, y: 5 }, stroke })).toEqual({
      x: 10,
      y: 5,
      width: 20,
      height: 45,
    });
    const step: LayerKind = { type: "step", center: { x: 20, y: 20 }, radius: 5, number: 1, fill: stroke.color, text_color: stroke.color };
    expect(bounds(step)).toEqual({ x: 15, y: 15, width: 10, height: 10 });
  });

  it("Verschieben verschiebt alle Punkte", () => {
    const k = translate({ type: "pen", points: [{ x: 0, y: 0 }, { x: 5, y: 5 }], stroke }, 10, -2);
    expect(k.type === "pen" && k.points).toEqual([{ x: 10, y: -2 }, { x: 15, y: 3 }]);
  });

  it("Linie trifft man nahe der Strecke", () => {
    const l = layer({ type: "line", from: { x: 0, y: 0 }, to: { x: 100, y: 0 }, stroke });
    expect(hitTest(l, { x: 50, y: 3 }, 2)).toBe(true);
    expect(hitTest(l, { x: 50, y: 10 }, 2)).toBe(false);
  });

  it("Ungefülltes Rechteck nur am Rand greifbar", () => {
    const l = layer({ type: "rectangle", bounds: { x: 0, y: 0, width: 100, height: 100 }, stroke, fill: null, corner_radius: 0 });
    expect(hitTest(l, { x: 1, y: 50 }, 2)).toBe(true);
    expect(hitTest(l, { x: 50, y: 50 }, 2)).toBe(false);
  });

  it("Lupe behält die Vergrösserung beim Ändern der Grösse", () => {
    const k: LayerKind = {
      type: "magnifier",
      bounds: { x: 0, y: 0, width: 100, height: 100 },
      source: { x: 25, y: 25, width: 50, height: 50 },
      stroke,
    };
    const r = withBounds(k, { x: 0, y: 0, width: 200, height: 200 });
    expect(r.type === "magnifier" && r.source.width).toBe(100);
  });
});
