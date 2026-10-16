// Ausrichtungshilfen: Beim Verschieben an Kanten und Mitten einrasten.

import type { Rect } from "./model";

export type Guides = { x: number[]; y: number[] };

/** Kanten und Mitte eines Rechtecks entlang einer Achse. */
function stops(r: Rect, axis: "x" | "y"): number[] {
  return axis === "x" ? [r.x, r.x + r.width / 2, r.x + r.width] : [r.y, r.y + r.height / 2, r.y + r.height];
}

/**
 * Wie weit muss `moving` verschoben werden, damit es an `targets` einrastet?
 * `tol` in Dokument-Pixeln. Liefert Korrektur und die Hilfslinien.
 */
export function snap(moving: Rect, targets: Rect[], tol: number): { dx: number; dy: number; guides: Guides } {
  const result = { dx: 0, dy: 0, guides: { x: [] as number[], y: [] as number[] } };
  for (const axis of ["x", "y"] as const) {
    const mine = stops(moving, axis);
    let best: { delta: number; at: number } | null = null;
    for (const t of targets) {
      for (const s of stops(t, axis)) {
        for (const m of mine) {
          const delta = s - m;
          if (Math.abs(delta) <= tol && (!best || Math.abs(delta) < Math.abs(best.delta))) {
            best = { delta, at: s };
          }
        }
      }
    }
    if (best) {
      if (axis === "x") result.dx = best.delta;
      else result.dy = best.delta;
      result.guides[axis].push(best.at);
    }
  }
  return result;
}
