// Zugriff aufs Backend. Läuft das Frontend im normalen Browser (Entwicklung
// ohne Tauri), liefert eine Attrappe Beispieldaten – so lässt sich die
// Oberfläche mit `npm run dev` ansehen.

import { commands, type CaptureInfo, type CaptureMode, type ExportFormat } from "$lib/bindings";
import { fromBackend, type Doc } from "$lib/editor/model";

export type { CaptureInfo };

export const isTauri =
  typeof window !== "undefined" && "__TAURI_INTERNALS__" in (window as object);

async function unwrap<T>(p: Promise<{ status: "ok"; data: T } | { status: "error"; error: string }>) {
  const r = await p;
  if (r.status === "error") throw new Error(r.error);
  return r.data;
}

/** URL für eine Datei der Bibliothek (Asset-Protokoll). */
export async function fileUrl(path: string, version = 0): Promise<string> {
  if (!isTauri) return mock.fileUrl(path);
  const { convertFileSrc } = await import("@tauri-apps/api/core");
  return convertFileSrc(path) + (version ? `?v=${version}` : "");
}

export async function fileBytes(path: string): Promise<Uint8Array> {
  const res = await fetch(await fileUrl(path));
  if (!res.ok) throw new Error(`${path}: ${res.status}`);
  return new Uint8Array(await res.arrayBuffer());
}

export const api = {
  listCaptures: (limit = 500): Promise<CaptureInfo[]> =>
    isTauri ? unwrap(commands.listCaptures(limit)) : mock.listCaptures(),
  getCapture: (id: string): Promise<CaptureInfo> =>
    isTauri ? unwrap(commands.getCapture(id)) : mock.getCapture(id),
  loadDocument: async (id: string): Promise<Doc> =>
    fromBackend(isTauri ? await unwrap(commands.loadDocument(id)) : await mock.loadDocument(id)),
  saveDocument: (id: string, doc: Doc): Promise<null> =>
    isTauri ? unwrap(commands.saveDocument(id, doc)) : Promise.resolve(null),
  copyDocument: (id: string, doc: Doc): Promise<null> =>
    isTauri ? unwrap(commands.copyDocument(id, doc)) : Promise.resolve(null),
  exportDocument: (id: string, doc: Doc, format: ExportFormat): Promise<string | null> =>
    isTauri ? unwrap(commands.exportDocument(id, doc, format)) : Promise.resolve(null),
  addAsset: (id: string, png: Uint8Array): Promise<string> =>
    isTauri ? unwrap(commands.addAsset(id, Array.from(png))) : Promise.resolve("mock"),
  deleteCapture: (id: string): Promise<null> =>
    isTauri ? unwrap(commands.deleteCapture(id)) : mock.deleteCapture(id),
  setStarred: (id: string, starred: boolean): Promise<null> =>
    isTauri ? unwrap(commands.setStarred(id, starred)) : mock.setStarred(id, starred),
  revealCapture: (id: string): Promise<null> =>
    isTauri ? unwrap(commands.revealCapture(id)) : Promise.resolve(null),
  startCapture: (mode: CaptureMode): Promise<void> =>
    isTauri ? commands.startCapture(mode) : Promise.resolve(),
};

/** Ereignisse aus dem Backend (z. B. neue Aufnahme gespeichert). */
export async function onCaptureSaved(handler: (id: string) => void): Promise<() => void> {
  if (!isTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<string>("capture-saved", (e) => handler(e.payload));
}

// --- Attrappe für den Browser ---------------------------------------------------

const mock = (() => {
  const captures: CaptureInfo[] = [];
  const images = new Map<string, string>();

  /** Erzeugt ein Beispielbild, das wie ein Fenster mit Text aussieht. */
  function sample(i: number, w: number, h: number): string {
    const c = document.createElement("canvas");
    c.width = w;
    c.height = h;
    const g = c.getContext("2d")!;
    const hue = (i * 67) % 360;
    g.fillStyle = `hsl(${hue} 30% 96%)`;
    g.fillRect(0, 0, w, h);
    g.fillStyle = `hsl(${hue} 40% 40%)`;
    g.fillRect(0, 0, w, 56);
    g.fillStyle = "#fff";
    g.font = "bold 28px sans-serif";
    g.fillText(`Beispiel-Aufnahme ${i + 1}`, 24, 38);
    g.fillStyle = "#333";
    g.font = "22px sans-serif";
    for (let l = 0; l < 12; l++) g.fillText(`Zeile ${l + 1}: Lorem ipsum dolor sit amet, Fehler 0x${(l * 4099).toString(16)}`, 24, 110 + l * 40);
    return c.toDataURL("image/png");
  }

  function ensure() {
    if (captures.length) return;
    const sizes = [
      [1600, 1000],
      [1200, 800],
      [900, 1200],
      [1800, 700],
    ];
    sizes.forEach(([w, h], i) => {
      const id = `20261016-22${String(10 + i).padStart(2, "0")}00-000-${i}`;
      const path = `mock://${id}`;
      images.set(path, sample(i, w, h));
      captures.unshift({
        id,
        createdAt: Date.now() - (sizes.length - i) * 60000,
        mode: "region",
        width: w,
        height: h,
        scaleFactor: 2,
        starred: i === 1,
        originalPath: path,
        thumbnailPath: path,
        dir: "mock",
      });
    });
  }

  return {
    fileUrl: (path: string) => images.get(path) ?? path,
    async listCaptures() {
      ensure();
      return captures.slice();
    },
    async getCapture(id: string) {
      ensure();
      const c = captures.find((c) => c.id === id);
      if (!c) throw new Error("nicht gefunden");
      return c;
    },
    async loadDocument(id: string): Promise<Doc> {
      const c = await this.getCapture(id);
      const r = { x: 0, y: 0, width: c.width, height: c.height };
      return {
        schema_version: 1,
        pixel_ratio: 2,
        artboard: { rect: r, background: null },
        layers: [
          {
            id: 1,
            visible: true,
            locked: true,
            opacity: 1,
            rotation: 0,
            kind: {
              type: "image",
              asset: { source: "original" },
              bounds: r,
              crop: null,
              source_size: { width: c.width, height: c.height },
              corner_radius: 0,
              shadow: null,
            },
          },
        ],
      };
    },
    async deleteCapture(id: string) {
      const i = captures.findIndex((c) => c.id === id);
      if (i >= 0) captures.splice(i, 1);
      return null;
    },
    async setStarred(id: string, starred: boolean) {
      const c = captures.find((c) => c.id === id);
      if (c) c.starred = starred;
      return null;
    },
  };
})();
