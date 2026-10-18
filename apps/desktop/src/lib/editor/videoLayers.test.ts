import { describe, expect, it } from "vitest";
import type { Doc, Layer } from "./model";
import { isBaseLayer, visibleVideoLayers, withVisible } from "./videoLayers";

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
