import { describe, expect, it } from "vitest";
import { LayerCache, type Painter, type Surface } from "./layerCache";

/** Leinwand-Attrappe: merkt sich, was auf jeder Fläche zu sehen ist. */
function fakeCanvas() {
  const shown: Record<Surface, string> = { base: "", active: "", above: "" };
  const painter: Painter = {
    draw(s, from, to, background) {
      shown[s] = `${background ? "bg+" : ""}${from}..${to}`;
    },
    clear(s) {
      shown[s] = "";
    },
  };
  /** Wie `canvas.width = …` im Browser: leert alle Flächen. */
  const resize = () => {
    shown.base = shown.active = shown.above = "";
  };
  return { shown, painter, resize };
}

describe("Zwischenspeicher der Leinwand", () => {
  it("zeichnet beim Ziehen nur die aktive Ebene neu", () => {
    const c = fakeCanvas();
    const cache = new LayerCache(c.painter);
    cache.begin(2, true);
    expect(c.shown).toEqual({ base: "bg+0..2", active: "2..3", above: "3..-1" });

    let draws = 0;
    c.painter.draw = () => void draws++;
    cache.render();
    expect(draws).toBe(1);
  });

  // Fehler aus dem Testvideo: Die neue Ebene wird ausgewählt, die
  // Eigenschaftsleiste wird breiter, die Leinwand ändert ihre Grösse – das
  // leert alle Flächen. Danach fehlte das Bild bis zum Loslassen der Maus.
  it("füllt den Zwischenspeicher nach einer Grössenänderung neu", () => {
    const c = fakeCanvas();
    const cache = new LayerCache(c.painter);
    cache.begin(2, true);

    c.resize();
    cache.invalidate();
    cache.render();

    expect(c.shown).toEqual({ base: "bg+0..2", active: "2..3", above: "3..-1" });
  });

  it("ohne Zieh-Aktion wird alles in die untere Fläche gezeichnet", () => {
    const c = fakeCanvas();
    const cache = new LayerCache(c.painter);
    cache.begin(1, true);
    cache.end();
    cache.render();
    expect(c.shown).toEqual({ base: "bg+0..-1", active: "", above: "" });
  });
});
