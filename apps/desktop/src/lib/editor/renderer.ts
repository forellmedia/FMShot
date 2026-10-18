// Brücke zum WASM-Renderer (fmshot-render). Lädt das Modul einmal und hält die
// Bilder der aktuellen Aufnahme.

import init, { WasmRenderer } from "$lib/wasm/fmshot_render";
import type { Doc, Rect } from "./model";

let module: Promise<unknown> | null = null;
let ready: Promise<WasmRenderer> | null = null;

/**
 * Lädt das WASM-Modul genau einmal. Zwei gleichzeitige `init()` legen zwei
 * Instanzen an – Objekte der ersten zeigen dann in fremden Speicher.
 */
export function wasmModule(): Promise<unknown> {
  module ??= init();
  return module;
}

export function renderer(): Promise<WasmRenderer> {
  ready ??= wasmModule().then(() => new WasmRenderer());
  return ready;
}

export type RenderJob = {
  doc: Doc;
  docJson?: string;
  region: Rect;
  scale: number;
  from?: number;
  to?: number;
  background?: boolean;
  skip?: number;
};

/** Zeichnet in ein ImageData (oder `null`, wenn die Fläche leer ist). */
export function render(r: WasmRenderer, job: RenderJob): ImageData | null {
  const { region, scale } = job;
  if (region.width <= 0 || region.height <= 0 || scale <= 0) return null;
  const json = job.docJson ?? JSON.stringify(job.doc);
  const img = r.render(
    json,
    region.x,
    region.y,
    region.width,
    region.height,
    scale,
    job.from ?? 0,
    job.to ?? -1,
    job.background ?? true,
    job.skip ?? 0,
  );
  const w = img.width;
  const h = img.height;
  const data = img.takeData();
  img.free();
  if (w === 0 || h === 0) return null;
  // wasm-bindgen kopiert die Pixel in einen eigenen ArrayBuffer – ohne weitere Kopie übernehmen.
  return new ImageData(new Uint8ClampedArray(data.buffer as ArrayBuffer, data.byteOffset, data.length), w, h);
}

export type TextSize = { width: number; height: number; lineHeight: number };

export function measureText(
  r: WasmRenderer,
  text: string,
  size: number,
  bold: boolean,
  maxWidth = 0,
): TextSize {
  const [width, height, lineHeight] = r.measureText(text || " ", size, bold, maxWidth);
  return { width, height, lineHeight };
}
