// Ebenen im Video (Etappe 6): Die Aufnahme selbst ist im Dokument eine
// Bild-Ebene; im Editor liegt dort das Video. Andere Ebenen sind nur in
// ihrem Zeitbereich zu sehen (gleiche Regel wie `edit::layer_visible`).

import type { Doc, Layer } from "./model";

type LayerTime = { layer: number; start_ms: number; end_ms: number };

/** Ist das die Aufnahme (unterste Bild-Ebene mit dem Original)? */
export function isBaseLayer(layer: Layer): boolean {
  return layer.kind.type === "image" && layer.kind.asset.source === "original";
}

function times(doc: Doc): LayerTime[] {
  return (doc as { video?: { layer_times?: LayerTime[] } | null }).video?.layer_times ?? [];
}

/** Zeitbereich einer Ebene (Quellzeit) oder `null` = immer sichtbar. */
export function layerTime(doc: Doc, id: number): LayerTime | null {
  return times(doc).find((t) => t.layer === id) ?? null;
}

/** IDs der Ebenen, die zur Quellzeit zu sehen sind (ohne die Aufnahme). */
export function visibleVideoLayers(doc: Doc, sourceMs: number): number[] {
  const all = times(doc);
  return doc.layers
    .filter((l) => {
      if (!l.visible || isBaseLayer(l)) return false;
      const t = all.find((x) => x.layer === l.id);
      return !t || (sourceMs >= t.start_ms && sourceMs < t.end_ms);
    })
    .map((l) => l.id);
}

/** Kopie zum Zeichnen: nur die genannten Ebenen sichtbar. */
export function withVisible(doc: Doc, ids: number[]): Doc {
  const set = new Set(ids);
  return { ...doc, layers: doc.layers.map((l) => ({ ...l, visible: set.has(l.id) })) };
}
