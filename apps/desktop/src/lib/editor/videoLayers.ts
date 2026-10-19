// Ebenen im Video (Etappe 6): Die Aufnahme selbst ist im Dokument eine
// Bild-Ebene; im Editor liegt dort das Video. Andere Ebenen sind nur in
// ihrem Zeitbereich zu sehen (gleiche Regel wie `edit::layer_visible`).

import { toCss, type Color, type Doc, type Layer, type LayerKind } from "./model";

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

// --- Zeitleiste: Zeilen je Werkzeug ---------------------------------------------

/** Zeilen der Zeitleiste in dieser Reihenfolge. */
export const LANE_GROUPS = [
  "arrow",
  "line",
  "rectangle",
  "ellipse",
  "pen",
  "step",
  "highlight",
  "text",
  "callout",
  "image",
  "other",
] as const;
export type LaneGroup = (typeof LANE_GROUPS)[number];

/** Zu welcher Zeile eine Ebene gehört. */
export function laneGroup(kind: LayerKind): LaneGroup {
  return (LANE_GROUPS as readonly string[]).includes(kind.type) ? (kind.type as LaneGroup) : "other";
}

/** Eine Ebene als Block (Ausgabe-Zeit in ms). */
export type TimeBlock = {
  layer: number;
  start: number;
  end: number;
  /** Ohne Zeitbereich: immer zu sehen. */
  always: boolean;
  /** Farbe der Ebene (CSS) oder `null`. */
  color: string | null;
  /** Kurzer Text im Block, z. B. die Nummer. */
  label: string | null;
  /** Im Ebenen-Panel ausgeblendet. */
  hidden: boolean;
};

export type Lane = {
  group: LaneGroup;
  /** Erste Zeile der Gruppe (trägt den Namen). */
  first: boolean;
  blocks: TimeBlock[];
};

function kindColor(kind: LayerKind): Color | null {
  switch (kind.type) {
    case "step":
    case "callout":
      return kind.fill;
    case "highlight":
      return kind.color;
    case "text":
      return kind.style.color;
    default:
      return "stroke" in kind ? kind.stroke.color : null;
  }
}

function kindLabel(kind: LayerKind): string | null {
  if (kind.type === "step") return String(kind.number);
  if (kind.type === "text" || kind.type === "callout") {
    const s = kind.text.trim().split("\n")[0];
    return s ? s.slice(0, 24) : null;
  }
  return null;
}

/**
 * Ebenen als Blöcke, je Werkzeug eine Zeile. Überlappen sich Blöcke, kommen
 * sie in weitere Zeilen derselben Gruppe. `toOutput` rechnet Zeit der
 * Aufnahme in Zeit des fertigen Videos.
 */
export function layerLanes(doc: Doc, durationMs: number, toOutput: (sourceMs: number) => number): Lane[] {
  const all = times(doc);
  const groups = new Map<LaneGroup, TimeBlock[]>();
  for (const l of doc.layers) {
    if (isBaseLayer(l)) continue;
    const t = all.find((x) => x.layer === l.id);
    const color = kindColor(l.kind);
    const block: TimeBlock = {
      layer: l.id,
      start: t ? toOutput(t.start_ms) : 0,
      end: t ? toOutput(t.end_ms) : durationMs,
      always: !t,
      color: color ? toCss({ ...color, a: 255 }) : null,
      label: kindLabel(l.kind),
      hidden: !l.visible,
    };
    const g = laneGroup(l.kind);
    groups.set(g, [...(groups.get(g) ?? []), block]);
  }
  const lanes: Lane[] = [];
  for (const group of LANE_GROUPS) {
    const blocks = groups.get(group);
    if (!blocks) continue;
    // Erste Zeile, in die der Block passt (Berühren ist erlaubt).
    const rows: TimeBlock[][] = [];
    for (const b of [...blocks].sort((a, c) => a.start - c.start || a.end - c.end)) {
      const row = rows.find((r) => r[r.length - 1].end <= b.start);
      if (row) row.push(b);
      else rows.push([b]);
    }
    rows.forEach((r, i) => lanes.push({ group, first: i === 0, blocks: r }));
  }
  return lanes;
}

/** Neuer Zeitbereich beim Ziehen eines Blocks (Ausgabe-Zeit). */
export function dragRange(
  block: { start: number; end: number },
  mode: "move" | "start" | "end",
  deltaMs: number,
  durationMs: number,
  minMs = 100,
): { start: number; end: number } {
  const len = block.end - block.start;
  if (mode === "move") {
    const start = Math.max(0, Math.min(durationMs - len, block.start + deltaMs));
    return { start, end: start + len };
  }
  if (mode === "start") {
    return { start: Math.max(0, Math.min(block.end - minMs, block.start + deltaMs)), end: block.end };
  }
  return { start: block.start, end: Math.min(durationMs, Math.max(block.start + minMs, block.end + deltaMs)) };
}

/** Zeitbereiche von Ebenen, die es nicht mehr gibt. */
export function staleLayerTimes(doc: Doc): number[] {
  const ids = new Set(doc.layers.map((l) => l.id));
  return times(doc)
    .map((t) => t.layer)
    .filter((id) => !ids.has(id));
}
