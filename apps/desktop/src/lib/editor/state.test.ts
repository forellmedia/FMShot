import { describe, expect, it } from "vitest";
import type { Doc, Layer } from "./model";
import { Editor } from "./state.svelte";

const red = { r: 229, g: 57, b: 53, a: 255 };
const blue = { r: 30, g: 136, b: 229, a: 255 };

const arrow: Layer = {
  id: 1,
  visible: true,
  locked: false,
  opacity: 1,
  rotation: 0,
  kind: { type: "arrow", from: { x: 0, y: 0 }, to: { x: 50, y: 20 }, stroke: { color: red, width: 6 } },
};

/**
 * In der App sind Dokument und Ebenen reaktive `$state`-Proxys. In Vitest
 * übersetzt Svelte für den Server ohne Proxys – darum hier von Hand.
 */
const reactive = <T extends object>(v: T): T => new Proxy(v, {});

function editorWith(layers: Layer[]): Editor {
  const e = new Editor();
  const doc: Doc = {
    schema_version: 1,
    pixel_ratio: 2,
    artboard: { rect: { x: 0, y: 0, width: 100, height: 100 }, background: null },
    layers: layers.map((l) => reactive({ ...l, kind: reactive(l.kind) })),
  };
  e.doc = doc;
  return e;
}

describe("Eigenschaften nachträglich ändern", () => {
  // Fehler aus dem Test vom 17.10.: Farbe/Stärke einer gezeichneten Ebene
  // liessen sich nicht ändern – structuredClone warf bei der reaktiven Ebene.
  it("Farbe und Stärke einer ausgewählten Ebene ändern", () => {
    const e = editorWith([structuredClone(arrow)]);
    e.updateKind(1, (k) => ("stroke" in k ? { ...k, stroke: { color: blue, width: 12 } } : k));
    const k = e.doc!.layers[0].kind;
    expect(k.type === "arrow" && k.stroke).toEqual({ color: blue, width: 12 });
  });

  it("ein Schritt rückgängig stellt den alten Stil wieder her", () => {
    const e = editorWith([structuredClone(arrow)]);
    e.updateKind(1, (k) => ("stroke" in k ? { ...k, stroke: { ...k.stroke, color: blue } } : k));
    e.undo();
    const k = e.doc!.layers[0].kind;
    expect(k.type === "arrow" && k.stroke.color).toEqual(red);
  });
});
