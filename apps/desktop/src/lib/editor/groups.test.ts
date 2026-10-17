import { describe, expect, it } from "vitest";
import {
  groupIds,
  groupLayers,
  hitTest,
  keepInPlace,
  moveBlock,
  normalizeAngle,
  pivot,
  rotateAround,
  rotatePoint,
  ungroupLayers,
  visualBounds,
  withBounds,
  type Doc,
  type Layer,
  type LayerKind,
} from "./model";

const stroke = { color: { r: 0, g: 0, b: 0, a: 255 }, width: 2 };
const rect = (id: number, x = 0, group?: number): Layer => ({
  id,
  visible: true,
  locked: false,
  opacity: 1,
  rotation: 0,
  group,
  kind: { type: "rectangle", bounds: { x, y: 0, width: 10, height: 10 }, stroke, fill: stroke.color, corner_radius: 0 },
});
const doc = (layers: Layer[]): Doc => ({
  schema_version: 1,
  pixel_ratio: 1,
  artboard: { rect: { x: 0, y: 0, width: 100, height: 100 }, background: null },
  layers,
});
const order = (d: Doc) => d.layers.map((l) => l.id);
const close = (a: number, b: number) => expect(Math.abs(a - b)).toBeLessThan(1e-6);

describe("Drehen", () => {
  it("Punkt um 90° im Uhrzeigersinn", () => {
    const p = rotatePoint({ x: 10, y: 0 }, 90, { x: 0, y: 0 });
    close(p.x, 0);
    close(p.y, 10);
  });

  it("Winkel bleibt zwischen -180 und 180", () => {
    expect(normalizeAngle(190)).toBe(-170);
    expect(normalizeAngle(-190)).toBe(170);
    expect(normalizeAngle(360)).toBe(0);
  });

  it("Treffer auf gedrehtem Balken", () => {
    const l: Layer = { ...rect(1), kind: { ...(rect(1).kind as LayerKind & { type: "rectangle" }), bounds: { x: 0, y: 45, width: 100, height: 10 } } };
    expect(hitTest(l, { x: 50, y: 10 }, 1)).toBe(false);
    l.rotation = 90;
    expect(hitTest(l, { x: 50, y: 10 }, 1)).toBe(true);
    expect(hitTest(l, { x: 10, y: 50 }, 1)).toBe(false);
  });

  it("Sichtbare Grenzen eines gedrehten Rechtecks", () => {
    const l = rect(1);
    l.kind = { ...(l.kind as LayerKind & { type: "rectangle" }), bounds: { x: 0, y: 0, width: 40, height: 10 } };
    l.rotation = 90;
    const b = visualBounds(l);
    close(b.width, 10);
    close(b.height, 40);
    close(b.x, 15);
  });

  it("Grösse ändern hält die gedrehte Ebene an ihrem Platz", () => {
    const orig: LayerKind = { type: "rectangle", bounds: { x: 0, y: 0, width: 40, height: 20 }, stroke, fill: null, corner_radius: 0 };
    // Rechte Kante im eigenen System um 20 nach rechts – die linke Kante
    // (Ecke oben links auf der Leinwand) darf sich nicht bewegen.
    const changed = keepInPlace(orig, withBounds(orig, { x: 0, y: 0, width: 60, height: 20 }), 90);
    const corner = (k: LayerKind) => {
      const b = (k as { bounds: { x: number; y: number } }).bounds;
      return rotatePoint({ x: b.x, y: b.y }, 90, pivot(k));
    };
    const a = corner(orig);
    const b = corner(changed);
    close(a.x, b.x);
    close(a.y, b.y);
  });

  it("Gruppe drehen dreht Mittelpunkte und Ebenen mit", () => {
    const l = rotateAround(rect(1, 20), 90, { x: 0, y: 5 });
    expect(l.rotation).toBe(90);
    const c = pivot(l.kind);
    close(c.x, 0);
    close(c.y, 30);
    const line = rotateAround({ ...rect(2), kind: { type: "line", from: { x: 0, y: 0 }, to: { x: 10, y: 0 }, stroke } }, 90, { x: 0, y: 0 });
    expect(line.rotation).toBe(0);
    expect(line.kind.type === "line" && Math.round(line.kind.to.y)).toBe(10);
  });
});

describe("Gruppen", () => {
  it("Gruppieren rückt Ebenen zur obersten zusammen", () => {
    const d = doc([rect(1), rect(2), rect(3), rect(4), rect(5)]);
    const g = groupLayers(d, [2, 4]);
    expect(g).toBe(1);
    expect(order(d)).toEqual([1, 3, 2, 4, 5]);
    expect(groupIds(d, 2)).toEqual([2, 4]);
    expect(groupIds(d, 3)).toEqual([3]);
  });

  it("Eine Ebene allein ist keine Gruppe", () => {
    const d = doc([rect(1), rect(2)]);
    expect(groupLayers(d, [1])).toBeNull();
    groupLayers(d, [1, 2]);
    ungroupLayers(d, [1]);
    expect(d.layers.every((l) => l.group == null)).toBe(true);
  });

  it("Block springt über eine Nachbargruppe", () => {
    const d = doc([rect(1), rect(2, 0, 7), rect(3, 0, 7), rect(4)]);
    moveBlock(d, [1], 1);
    expect(order(d)).toEqual([2, 3, 1, 4]);
    moveBlock(d, [2, 3], 1);
    expect(order(d)).toEqual([1, 2, 3, 4]);
    moveBlock(d, [2, 3], 1);
    expect(order(d)).toEqual([1, 4, 2, 3]);
    moveBlock(d, [2, 3], 1);
    expect(order(d)).toEqual([1, 4, 2, 3]);
    moveBlock(d, [2, 3], -1);
    expect(order(d)).toEqual([1, 2, 3, 4]);
    moveBlock(d, [1], 1);
    expect(order(d)).toEqual([2, 3, 1, 4]);
  });
});
