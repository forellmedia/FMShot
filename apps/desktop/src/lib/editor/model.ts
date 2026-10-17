// Dokumentmodell im Frontend – entspricht fmshot-core (Rust).
//
// Die erzeugten Bindings (bindings.ts) erlauben `null` für jede Zahl (JSON
// kennt kein NaN). Im Editor arbeiten wir mit strengen Typen und prüfen die
// Verträglichkeit unten zur Compile-Zeit.

import type { Document_Deserialize } from "$lib/bindings";

export type Color = { r: number; g: number; b: number; a: number };
export type Point = { x: number; y: number };
export type Rect = { x: number; y: number; width: number; height: number };
export type Size = { width: number; height: number };
export type Stroke = { color: Color; width: number };
export type Shadow = { color: Color; blur: number; offset_y: number };
export type TextStyle = { size: number; color: Color; bold: boolean; background: Color | null };
export type Fill =
  | { type: "solid"; color: Color }
  | { type: "linear_gradient"; from: Color; to: Color; angle: number };
export type AssetRef = { source: "original" } | { source: "asset"; hash: string };

export type LayerKind =
  | {
      type: "image";
      asset: AssetRef;
      bounds: Rect;
      crop: Rect | null;
      source_size: Size;
      corner_radius: number;
      shadow: Shadow | null;
    }
  | { type: "rectangle"; bounds: Rect; stroke: Stroke; fill: Color | null; corner_radius: number }
  | { type: "ellipse"; bounds: Rect; stroke: Stroke; fill: Color | null }
  | { type: "line"; from: Point; to: Point; stroke: Stroke }
  | { type: "arrow"; from: Point; to: Point; stroke: Stroke }
  | { type: "pen"; points: Point[]; stroke: Stroke }
  | { type: "highlight"; bounds: Rect; color: Color }
  | { type: "text"; bounds: Rect; text: string; style: TextStyle }
  | { type: "callout"; bounds: Rect; tail: Point; text: string; style: TextStyle; fill: Color }
  | { type: "step"; center: Point; radius: number; number: number; fill: Color; text_color: Color }
  | { type: "blur"; bounds: Rect; radius: number }
  | { type: "pixelate"; bounds: Rect; block: number }
  | { type: "spotlight"; bounds: Rect; dim: number }
  | { type: "magnifier"; bounds: Rect; source: Rect; stroke: Stroke };

export type LayerType = LayerKind["type"];

export type Layer = {
  id: number;
  name?: string | null;
  visible: boolean;
  locked: boolean;
  opacity: number;
  rotation: number;
  /** Gruppe (Ebenen liegen direkt übereinander), siehe fmshot-core. */
  group?: number | null;
  kind: LayerKind;
};

export type Doc = {
  schema_version: number;
  pixel_ratio: number;
  artboard: { rect: Rect; background: Fill | null };
  layers: Layer[];
};

// Compile-Zeit-Prüfung: Doc passt in den Typ, den Rust erwartet.
const _compatible = (d: Doc): Document_Deserialize => d;
void _compatible;

/** Dokument aus dem Backend (Zahlen können laut Typ `null` sein). */
export function fromBackend(raw: unknown): Doc {
  const doc = raw as Doc;
  doc.pixel_ratio ||= 1;
  for (const l of doc.layers) {
    l.rotation ||= 0;
    l.opacity ??= 1;
  }
  return doc;
}

// --- Farben -----------------------------------------------------------------

export const rgb = (r: number, g: number, b: number, a = 255): Color => ({ r, g, b, a });
export const PALETTE: Color[] = [
  rgb(0xe5, 0x39, 0x35),
  rgb(0xfb, 0x8c, 0x00),
  rgb(0xfd, 0xd8, 0x35),
  rgb(0x43, 0xa0, 0x47),
  rgb(0x1e, 0x88, 0xe5),
  rgb(0x8e, 0x24, 0xaa),
  rgb(0x21, 0x21, 0x21),
  rgb(0xff, 0xff, 0xff),
];
export const HIGHLIGHT_YELLOW = rgb(0xff, 0xeb, 0x3b);

export function toCss(c: Color): string {
  return `rgba(${c.r}, ${c.g}, ${c.b}, ${c.a / 255})`;
}

export function toHex(c: Color): string {
  return "#" + [c.r, c.g, c.b].map((v) => v.toString(16).padStart(2, "0")).join("");
}

export function fromHex(hex: string, a = 255): Color {
  const v = parseInt(hex.replace("#", ""), 16);
  return rgb((v >> 16) & 255, (v >> 8) & 255, v & 255, a);
}

export function sameColor(a: Color, b: Color): boolean {
  return a.r === b.r && a.g === b.g && a.b === b.b && a.a === b.a;
}

// --- Geometrie ----------------------------------------------------------------

export function rectFromPoints(a: Point, b: Point): Rect {
  return {
    x: Math.min(a.x, b.x),
    y: Math.min(a.y, b.y),
    width: Math.abs(a.x - b.x),
    height: Math.abs(a.y - b.y),
  };
}

export function union(a: Rect, b: Rect): Rect {
  const x = Math.min(a.x, b.x);
  const y = Math.min(a.y, b.y);
  return {
    x,
    y,
    width: Math.max(a.x + a.width, b.x + b.width) - x,
    height: Math.max(a.y + a.height, b.y + b.height) - y,
  };
}

export function inflate(r: Rect, d: number): Rect {
  return { x: r.x - d, y: r.y - d, width: r.width + 2 * d, height: r.height + 2 * d };
}

export function containsPoint(r: Rect, p: Point): boolean {
  return p.x >= r.x && p.y >= r.y && p.x <= r.x + r.width && p.y <= r.y + r.height;
}

function distToSegment(p: Point, a: Point, b: Point): number {
  const dx = b.x - a.x;
  const dy = b.y - a.y;
  const len2 = dx * dx + dy * dy;
  const t = len2 === 0 ? 0 : Math.max(0, Math.min(1, ((p.x - a.x) * dx + (p.y - a.y) * dy) / len2));
  return Math.hypot(p.x - (a.x + t * dx), p.y - (a.y + t * dy));
}

/** Umschliessendes Rechteck (ohne Strichbreite) – wie `LayerKind::bounds`. */
export function bounds(kind: LayerKind): Rect {
  switch (kind.type) {
    case "line":
    case "arrow":
      return rectFromPoints(kind.from, kind.to);
    case "pen": {
      if (kind.points.length === 0) return { x: 0, y: 0, width: 0, height: 0 };
      let r: Rect = { x: kind.points[0].x, y: kind.points[0].y, width: 0, height: 0 };
      for (const p of kind.points) r = union(r, { x: p.x, y: p.y, width: 0, height: 0 });
      return r;
    }
    case "step":
      return {
        x: kind.center.x - kind.radius,
        y: kind.center.y - kind.radius,
        width: kind.radius * 2,
        height: kind.radius * 2,
      };
    case "callout":
      return union(kind.bounds, { x: kind.tail.x, y: kind.tail.y, width: 0, height: 0 });
    default:
      return kind.bounds;
  }
}

export function center(r: Rect): Point {
  return { x: r.x + r.width / 2, y: r.y + r.height / 2 };
}

/** Dreht `p` um `deg` Grad im Uhrzeigersinn (y nach unten) um `c`. */
export function rotatePoint(p: Point, deg: number, c: Point): Point {
  if (!deg) return p;
  const a = (deg * Math.PI) / 180;
  const cos = Math.cos(a);
  const sin = Math.sin(a);
  const dx = p.x - c.x;
  const dy = p.y - c.y;
  return { x: c.x + dx * cos - dy * sin, y: c.y + dx * sin + dy * cos };
}

/** Winkel auf -180 bis 180 Grad bringen. */
export function normalizeAngle(deg: number): number {
  const a = (((deg + 180) % 360) + 360) % 360 - 180;
  return Math.abs(a) < 1e-9 ? 0 : a;
}

/** Wirkt die Ebene auf das darunter (wie `LayerKind::is_effect`)? */
export function isEffect(kind: LayerKind): boolean {
  return ["blur", "pixelate", "spotlight", "magnifier"].includes(kind.type);
}

/** Kann die Ebene gedreht werden? Linien und Pfeile drehen über ihre Enden. */
export function canRotate(kind: LayerKind): boolean {
  return !isEffect(kind) && kind.type !== "line" && kind.type !== "arrow";
}

/** Wirksame Drehung (Effekte drehen nie mit – wie im Renderer). */
export function rotationOf(layer: Layer): number {
  return isEffect(layer.kind) ? 0 : layer.rotation || 0;
}

/** Drehpunkt der Ebene: Mitte von `bounds`. */
export function pivot(kind: LayerKind): Point {
  return center(bounds(kind));
}

/** Punkt der Leinwand → ungedrehtes Koordinatensystem der Ebene. */
export function toLocal(layer: Layer, p: Point): Point {
  const r = rotationOf(layer);
  return r ? rotatePoint(p, -r, pivot(layer.kind)) : p;
}

/** Umschliessendes Rechteck nach der Drehung – wie `Layer::visual_bounds`. */
export function visualBounds(layer: Layer): Rect {
  const b = bounds(layer.kind);
  const r = rotationOf(layer);
  if (!r) return b;
  const a = (r * Math.PI) / 180;
  const cos = Math.abs(Math.cos(a));
  const sin = Math.abs(Math.sin(a));
  const hw = (b.width * cos + b.height * sin) / 2;
  const hh = (b.width * sin + b.height * cos) / 2;
  const c = center(b);
  return { x: c.x - hw, y: c.y - hh, width: 2 * hw, height: 2 * hh };
}

/** Überschneiden sich zwei Rechtecke? */
export function intersects(a: Rect, b: Rect): boolean {
  return a.x <= b.x + b.width && b.x <= a.x + a.width && a.y <= b.y + b.height && b.y <= a.y + a.height;
}

/**
 * Nach dem Ändern einer gedrehten Ebene in ihrem eigenen Koordinatensystem:
 * so verschieben, dass sie auf der Leinwand an derselben Stelle bleibt
 * (der Drehpunkt wandert mit der Grösse mit).
 */
export function keepInPlace(orig: LayerKind, changed: LayerKind, rotation: number): LayerKind {
  if (!rotation) return changed;
  const c = pivot(changed);
  const target = rotatePoint(c, rotation, pivot(orig));
  return translate(changed, target.x - c.x, target.y - c.y);
}

/** Dreht eine Ebene um `deg` Grad um den Punkt `c` (Gruppe drehen). */
export function rotateAround(layer: Layer, deg: number, c: Point): Layer {
  const k = layer.kind;
  if (k.type === "line" || k.type === "arrow") {
    return { ...layer, kind: { ...k, from: rotatePoint(k.from, deg, c), to: rotatePoint(k.to, deg, c) } };
  }
  const p = pivot(k);
  const q = rotatePoint(p, deg, c);
  const kind = translate(k, q.x - p.x, q.y - p.y);
  if (isEffect(k)) return { ...layer, kind };
  return { ...layer, kind, rotation: normalizeAngle((layer.rotation || 0) + deg) };
}

/** Liegt `p` auf der Ebene? `tol` in Dokument-Pixeln. */
export function hitTest(layer: Layer, point: Point, tol: number): boolean {
  const k = layer.kind;
  const p = toLocal(layer, point);
  const sw = "stroke" in k ? k.stroke.width / 2 : 0;
  switch (k.type) {
    case "line":
    case "arrow":
      return distToSegment(p, k.from, k.to) <= tol + sw;
    case "pen":
      for (let i = 1; i < k.points.length; i++) {
        if (distToSegment(p, k.points[i - 1], k.points[i]) <= tol + sw) return true;
      }
      return k.points.length === 1 && Math.hypot(p.x - k.points[0].x, p.y - k.points[0].y) <= tol + sw;
    case "rectangle":
    case "ellipse":
      // Ohne Füllung nur am Rand greifbar – sonst verdeckt ein Rahmen alles darunter.
      if (k.fill) return containsPoint(inflate(k.bounds, tol + sw), p);
      return (
        containsPoint(inflate(k.bounds, tol + sw), p) &&
        !containsPoint(inflate(k.bounds, -(tol + sw)), p)
      );
    case "step":
      return Math.hypot(p.x - k.center.x, p.y - k.center.y) <= k.radius + tol;
    case "magnifier":
      return containsPoint(inflate(k.bounds, tol), p);
    default:
      return containsPoint(inflate(bounds(k), tol), p);
  }
}

/** Verschiebt eine Ebene um (dx, dy). Gibt eine neue Art zurück. */
export function translate(kind: LayerKind, dx: number, dy: number): LayerKind {
  const mv = (p: Point): Point => ({ x: p.x + dx, y: p.y + dy });
  const mr = (r: Rect): Rect => ({ ...r, x: r.x + dx, y: r.y + dy });
  switch (kind.type) {
    case "line":
    case "arrow":
      return { ...kind, from: mv(kind.from), to: mv(kind.to) };
    case "pen":
      return { ...kind, points: kind.points.map(mv) };
    case "step":
      return { ...kind, center: mv(kind.center) };
    case "callout":
      return { ...kind, bounds: mr(kind.bounds), tail: mv(kind.tail) };
    case "magnifier":
      return { ...kind, bounds: mr(kind.bounds), source: mr(kind.source) };
    default:
      return { ...kind, bounds: mr(kind.bounds) } as LayerKind;
  }
}

/** Setzt das Rechteck einer rechteckigen Ebene (Griffe ziehen). */
export function withBounds(kind: LayerKind, r: Rect): LayerKind {
  switch (kind.type) {
    case "line":
    case "arrow":
    case "pen": {
      // Proportional in das neue Rechteck abbilden.
      const old = bounds(kind);
      const sx = old.width ? r.width / old.width : 1;
      const sy = old.height ? r.height / old.height : 1;
      const map = (p: Point): Point => ({ x: r.x + (p.x - old.x) * sx, y: r.y + (p.y - old.y) * sy });
      if (kind.type === "pen") return { ...kind, points: kind.points.map(map) };
      return { ...kind, from: map(kind.from), to: map(kind.to) };
    }
    case "step": {
      const radius = Math.max(4, Math.min(r.width, r.height) / 2);
      return { ...kind, radius, center: { x: r.x + r.width / 2, y: r.y + r.height / 2 } };
    }
    case "magnifier": {
      // Lupe: Ausschnitt bleibt mittig, Vergrösserung bleibt gleich.
      const zoom = kind.bounds.width / Math.max(1, kind.source.width);
      const cx = r.x + r.width / 2;
      const cy = r.y + r.height / 2;
      const sw = r.width / zoom;
      const sh = r.height / zoom;
      return { ...kind, bounds: r, source: { x: cx - sw / 2, y: cy - sh / 2, width: sw, height: sh } };
    }
    default:
      return { ...kind, bounds: r } as LayerKind;
  }
}

/** Hat die Ebene ein veränderbares Rechteck mit 8 Griffen? */
export function hasBoxHandles(kind: LayerKind): boolean {
  return !["line", "arrow", "pen"].includes(kind.type);
}

export function nextLayerId(doc: Doc): number {
  return doc.layers.reduce((m, l) => Math.max(m, l.id), 0) + 1;
}

export function nextStepNumber(doc: Doc): number {
  return doc.layers.reduce((m, l) => (l.kind.type === "step" ? Math.max(m, l.kind.number) : m), 0) + 1;
}

// --- Gruppen -------------------------------------------------------------------

/** IDs aller Ebenen, die mit `id` zusammen ausgewählt werden (ihre Gruppe). */
export function groupIds(doc: Doc, id: number): number[] {
  const l = doc.layers.find((x) => x.id === id);
  if (!l) return [];
  if (l.group == null) return [id];
  return doc.layers.filter((x) => x.group === l.group).map((x) => x.id);
}

export function nextGroupId(doc: Doc): number {
  return doc.layers.reduce((m, l) => Math.max(m, l.group ?? 0), 0) + 1;
}

/**
 * Fasst Ebenen zu einer Gruppe zusammen. Sie rücken dabei in der Reihenfolge
 * an die Stelle der obersten von ihnen (Reihenfolge untereinander bleibt).
 * Gibt die neue Gruppen-ID zurück (oder null bei weniger als zwei Ebenen).
 */
export function groupLayers(doc: Doc, ids: number[]): number | null {
  const set = new Set(ids);
  const members = doc.layers.filter((l) => set.has(l.id));
  if (members.length < 2) return null;
  const group = nextGroupId(doc);
  const top = Math.max(...members.map((m) => doc.layers.indexOf(m)));
  const rest = doc.layers.filter((l) => !set.has(l.id));
  // Wie viele Nicht-Mitglieder liegen unter der obersten?
  const below = doc.layers.slice(0, top).filter((l) => !set.has(l.id)).length;
  for (const m of members) m.group = group;
  doc.layers = [...rest.slice(0, below), ...members, ...rest.slice(below)];
  normalizeGroups(doc);
  return group;
}

export function ungroupLayers(doc: Doc, ids: number[]) {
  const set = new Set(ids);
  for (const l of doc.layers) if (set.has(l.id)) l.group = null;
  normalizeGroups(doc);
}

/** Gruppen mit nur noch einer Ebene auflösen. */
export function normalizeGroups(doc: Doc) {
  const count = new Map<number, number>();
  for (const l of doc.layers) if (l.group != null) count.set(l.group, (count.get(l.group) ?? 0) + 1);
  for (const l of doc.layers) {
    if (l.group != null && (count.get(l.group) ?? 0) < 2) l.group = null;
    if (l.group == null) delete l.group;
  }
}

/**
 * Verschiebt einen zusammenhängenden Block von Ebenen um eine Stufe nach oben
 * (+1) oder unten (-1). Eine benachbarte Gruppe wird als Ganzes übersprungen.
 */
export function moveBlock(doc: Doc, ids: number[], delta: 1 | -1) {
  const set = new Set(ids);
  const idx = doc.layers.map((l, i) => (set.has(l.id) ? i : -1)).filter((i) => i >= 0);
  if (!idx.length) return;
  const first = idx[0];
  const last = idx[idx.length - 1];
  if (last - first + 1 !== idx.length) return; // nicht zusammenhängend
  const block = doc.layers.slice(first, last + 1);
  const rest = [...doc.layers.slice(0, first), ...doc.layers.slice(last + 1)];
  // Nachbar-Block (Gruppe oder einzelne Ebene) in Bewegungsrichtung.
  let at = first;
  const step = (i: number) => {
    const g = rest[i]?.group;
    let j = i;
    if (g != null) while (rest[j + delta]?.group === g) j += delta;
    return j;
  };
  if (delta > 0) {
    if (first >= rest.length) return;
    at = step(first) + 1;
  } else {
    if (first === 0) return;
    at = step(first - 1);
  }
  doc.layers = [...rest.slice(0, at), ...block, ...rest.slice(at)];
}

export function newLayer(doc: Doc, kind: LayerKind): Layer {
  return { id: nextLayerId(doc), visible: true, locked: false, opacity: 1, rotation: 0, kind };
}

export function clone<T>(v: T): T {
  return structuredClone(v);
}

/** Text-Innenabstand – identisch zu `TextEngine::padding` in Rust. */
export function textPadding(style: TextStyle): number {
  return style.background ? style.size * 0.35 : 0;
}

export const LINE_HEIGHT = 1.25;
