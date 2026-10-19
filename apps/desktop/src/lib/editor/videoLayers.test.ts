import { describe, expect, it } from "vitest";
import type { Doc, Layer } from "./model";
import { dragRange, isBaseLayer, layerLanes, staleLayerTimes, visibleVideoLayers, withVisible } from "./videoLayers";

const r = { x: 0, y: 0, width: 10, height: 10 };

function layer(id: number, base = false): Layer {
  return {
    id,
    visible: true,
    locked: base,
    opacity: 1,
    rotation: 0,
    kind: base
      ? {
          type: "image",
          asset: { source: "original" },
          bounds: r,
          crop: null,
          source_size: { width: 10, height: 10 },
          corner_radius: 0,
          shadow: null,
        }
      : { type: "rectangle", bounds: r, stroke: { color: { r: 0, g: 0, b: 0, a: 255 }, width: 2, style: "solid" }, fill: null, corner_radius: 0 },
  } as Layer;
}

const doc = {
  schema_version: 1,
  pixel_ratio: 2,
  artboard: { rect: r, background: null },
  layers: [layer(1, true), layer(2), layer(3)],
  video: { duration_ms: 5000, layer_times: [{ layer: 3, start_ms: 1000, end_ms: 2000 }] },
} as unknown as Doc;

describe("Ebenen im Video", () => {
  it("die Aufnahme ist nie dabei", () => {
    expect(isBaseLayer(doc.layers[0])).toBe(true);
    expect(visibleVideoLayers(doc, 0)).toEqual([2]);
  });

  it("Ebenen nur in ihrem Zeitbereich", () => {
    expect(visibleVideoLayers(doc, 1500)).toEqual([2, 3]);
    expect(visibleVideoLayers(doc, 2000)).toEqual([2]);
  });

  it("Kopie blendet den Rest aus", () => {
    const copy = withVisible(doc, [3]);
    expect(copy.layers.map((l) => l.visible)).toEqual([false, false, true]);
    expect(doc.layers[1].visible).toBe(true);
  });
});

describe("Zeitleiste: Zeilen je Werkzeug", () => {
  const stroke = { color: { r: 229, g: 57, b: 53, a: 255 }, width: 3, style: "solid" };
  const arrow = (id: number) =>
    ({ id, visible: true, locked: false, opacity: 1, rotation: 0, kind: { type: "arrow", from: { x: 0, y: 0 }, to: { x: 5, y: 5 }, stroke, head: "filled", head_size: 1, control: null } }) as Layer;
  const step = (id: number, number: number) =>
    ({ id, visible: true, locked: false, opacity: 1, rotation: 0, kind: { type: "step", center: { x: 0, y: 0 }, radius: 4, number, fill: stroke.color, text_color: stroke.color } }) as Layer;
  const d = {
    ...doc,
    layers: [layer(1, true), arrow(2), arrow(3), arrow(4), step(5, 1), layer(6)],
    video: {
      duration_ms: 10_000,
      layer_times: [
        { layer: 2, start_ms: 0, end_ms: 3000 },
        { layer: 3, start_ms: 1000, end_ms: 2000 },
        { layer: 4, start_ms: 3000, end_ms: 5000 },
        { layer: 5, start_ms: 4000, end_ms: 6000 },
        { layer: 99, start_ms: 0, end_ms: 1 },
      ],
    },
  } as unknown as Doc;

  it("überlappende Pfeile bekommen eine zweite Zeile", () => {
    const lanes = layerLanes(d, 10_000, (ms) => ms);
    expect(lanes.map((l) => [l.group, l.first, l.blocks.map((b) => b.layer)])).toEqual([
      ["arrow", true, [2, 4]],
      ["arrow", false, [3]],
      ["rectangle", true, [6]],
      ["step", true, [5]],
    ]);
    const s = lanes[3].blocks[0];
    expect(s.label).toBe("1");
    expect(s.color).toBe("rgba(229, 57, 53, 1)");
    // Ohne Zeitbereich: über die ganze Länge
    expect(lanes[2].blocks[0]).toMatchObject({ start: 0, end: 10_000, always: true });
  });

  it("Zeit des fertigen Videos", () => {
    const lanes = layerLanes(d, 5000, (ms) => ms / 2);
    expect(lanes[0].blocks[1]).toMatchObject({ layer: 4, start: 1500, end: 2500 });
  });

  it("Ziehen bleibt im Video und behält eine Mindestlänge", () => {
    const b = { start: 1000, end: 3000 };
    expect(dragRange(b, "move", 500, 10_000)).toEqual({ start: 1500, end: 3500 });
    expect(dragRange(b, "move", -5000, 10_000)).toEqual({ start: 0, end: 2000 });
    expect(dragRange(b, "move", 9000, 10_000)).toEqual({ start: 8000, end: 10_000 });
    expect(dragRange(b, "start", 5000, 10_000)).toEqual({ start: 2900, end: 3000 });
    expect(dragRange(b, "end", -5000, 10_000)).toEqual({ start: 1000, end: 1100 });
    expect(dragRange(b, "end", 99_000, 10_000)).toEqual({ start: 1000, end: 10_000 });
  });

  it("verwaiste Zeitbereiche werden erkannt", () => {
    expect(staleLayerTimes(d)).toEqual([99]);
  });
});
