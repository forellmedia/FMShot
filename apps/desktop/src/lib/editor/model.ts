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

/** Liegt `p` auf der Ebene? `tol` in Dokument-Pixeln. */
export function hitTest(layer: Layer, p: Point, tol: number): boolean {
  const k = layer.kind;
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
