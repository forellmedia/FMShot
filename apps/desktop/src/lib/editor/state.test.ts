import { describe, expect, it } from "vitest";
import { fromBackend, nextStepNumber, type Doc, type Layer } from "./model";
import { Editor } from "./state.svelte";

const red = { r: 229, g: 57, b: 53, a: 255 };
const blue = { r: 30, g: 136, b: 229, a: 255 };

const arrow: Layer = {
  id: 1,
  visible: true,
  locked: false,
  opacity: 1,
  rotation: 0,
  kind: { type: "arrow", from: { x: 0, y: 0 }, to: { x: 50, y: 20 }, stroke: { color: red, width: 6, style: "solid" }, head: "filled", head_size: 1, control: null },
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
    e.updateKind(1, (k) => ("stroke" in k ? { ...k, stroke: { color: blue, width: 12, style: "solid" } } : k));
    const k = e.doc!.layers[0].kind;
    expect(k.type === "arrow" && k.stroke).toEqual({ color: blue, width: 12, style: "solid" });
  });

  it("ein Schritt rückgängig stellt den alten Stil wieder her", () => {
    const e = editorWith([structuredClone(arrow)]);
    e.updateKind(1, (k) => ("stroke" in k ? { ...k, stroke: { ...k.stroke, color: blue } } : k));
    e.undo();
    const k = e.doc!.layers[0].kind;
    expect(k.type === "arrow" && k.stroke.color).toEqual(red);
  });
});

const step = (id: number, number: number, radius = 28): Layer => ({
  id,
  visible: true,
  locked: false,
  opacity: 1,
  rotation: 0,
  kind: { type: "step", center: { x: id * 10, y: 10 }, radius, number, fill: red, text_color: blue },
});

const radii = (e: Editor) => e.doc!.layers.map((l) => (l.kind.type === "step" ? l.kind.radius : null));

describe("Nummern", () => {
  it("Startwert gilt, solange er über der höchsten Nummer liegt", () => {
    const doc = editorWith([step(1, 1), step(2, 2)]).doc!;
    expect(nextStepNumber(doc)).toBe(3);
    expect(nextStepNumber(doc, 2)).toBe(3);
    expect(nextStepNumber(doc, 7)).toBe(7);
    expect(nextStepNumber(editorWith([]).doc!, 2)).toBe(2);
  });

  it("Grösse ändert immer alle Nummern", () => {
    const e = editorWith([step(1, 1), step(2, 2, 10)]);
    e.setStepRadius(40);
    expect(radii(e)).toEqual([40, 40]);
  });
});

describe("Werkzeug wählen", () => {
  it("Präsentation legt sofort einen Hintergrund an", () => {
    const e = editorWith([]);
    e.setTool("background");
    expect(e.doc!.artboard.background).not.toBeNull();
  });

  it("Unkenntlich machen merkt sich die Art", () => {
    const e = editorWith([]);
    e.setTool("pixelate");
    expect(e.style.obscure).toBe("pixelate");
  });
});

describe("Ältere Aufnahmen", () => {
  it("bekommen die neuen Felder mit Standardwerten", () => {
    const doc = fromBackend({
      schema_version: 1,
      pixel_ratio: 1,
      artboard: { rect: { x: 0, y: 0, width: 10, height: 10 }, background: null },
      layers: [
        {
          id: 1,
          visible: true,
          locked: false,
          opacity: 1,
          kind: { type: "arrow", from: { x: 0, y: 0 }, to: { x: 5, y: 5 }, stroke: { color: red, width: 2 } },
        },
      ],
    });
    const k = doc.layers[0].kind;
    expect(k.type === "arrow" && [k.stroke.style, k.head, k.head_size]).toEqual(["solid", "filled", 1]);
  });
});

describe("Ansicht verschieben", () => {
  const view = () => {
    const e = editorWith([]);
    e.doc!.artboard.rect = { x: 0, y: 0, width: 1000, height: 500 };
    e.viewport = { width: 800, height: 600 };
    return e;
  };

  it("passt das Bild hinein, bleibt es mittig", () => {
    const e = view();
    e.zoom = 0.5; // 500 × 250 Pixel im 800 × 600-Fenster
    e.setPan(-5000, 9000);
    expect(e.panX).toBe(500 - 800);
    expect(e.panY).toBe(250 - 600);
  });

  it("grösser als das Fenster: höchstens 40 px Rand", () => {
    const e = view();
    e.zoom = 2; // 2000 × 1000 Pixel
    e.setPan(-1000, -1000);
    expect([e.panX, e.panY]).toEqual([-20, -20]);
    e.setPan(5000, 5000);
    expect([e.panX, e.panY]).toEqual([1000 + 20 - 400, 500 + 20 - 300]);
  });
});
