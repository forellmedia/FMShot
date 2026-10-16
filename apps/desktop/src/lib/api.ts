// Zugriff aufs Backend. Läuft das Frontend im normalen Browser (Entwicklung
// ohne Tauri), liefert eine Attrappe Beispieldaten – so lässt sich die
// Oberfläche mit `npm run dev` ansehen.

import {
  commands,
  type AssetInfo,
  type CaptureInfo,
  type CaptureMode,
  type Diagnostics,
  type ExportFormat,
  type HotkeyInfo,
  type Settings,
} from "$lib/bindings";
import { fromBackend, type Doc } from "$lib/editor/model";

export type { AssetInfo, CaptureInfo, Diagnostics, HotkeyInfo, Settings };

/** Erkannter Textblock in Pixeln der Originalaufnahme. */
export type OcrBlock = { text: string; x: number; y: number; width: number; height: number };

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
  addAsset: (id: string, bytes: Uint8Array): Promise<AssetInfo> =>
    isTauri ? unwrap(commands.addAsset(id, Array.from(bytes))) : mock.addAsset(bytes),
  addAssetFile: (id: string, path: string): Promise<AssetInfo> =>
    isTauri ? unwrap(commands.addAssetFile(id, path)) : Promise.reject(new Error("nur in der App")),
  addAssetFromCapture: (id: string, sourceId: string): Promise<AssetInfo> =>
    isTauri ? unwrap(commands.addAssetFromCapture(id, sourceId)) : mock.addAssetFromCapture(sourceId),
  recognizeText: async (id: string): Promise<OcrBlock[]> =>
    isTauri ? ((await unwrap(commands.recognizeText(id))) as OcrBlock[]) : mock.recognizeText(id),
  copyText: (text: string): Promise<null> =>
    isTauri ? unwrap(commands.copyText(text)) : navigator.clipboard.writeText(text).then(() => null),
  deleteCapture: (id: string): Promise<null> =>
    isTauri ? unwrap(commands.deleteCapture(id)) : mock.deleteCapture(id),
  setStarred: (id: string, starred: boolean): Promise<null> =>
    isTauri ? unwrap(commands.setStarred(id, starred)) : mock.setStarred(id, starred),
  revealCapture: (id: string): Promise<null> =>
    isTauri ? unwrap(commands.revealCapture(id)) : Promise.resolve(null),
  startCapture: (mode: CaptureMode): Promise<void> =>
    isTauri ? commands.startCapture(mode) : Promise.resolve(),
  searchCaptures: (query: string): Promise<CaptureInfo[]> =>
    isTauri ? unwrap(commands.searchCaptures(query)) : mock.search(query),
  getSettings: (): Promise<Settings> =>
    isTauri ? commands.getSettings() : Promise.resolve(mock.settings),
  setSettings: (settings: Settings): Promise<null> =>
    isTauri ? unwrap(commands.setSettings(settings)) : Promise.resolve((mock.settings = settings, null)),
  getHotkeys: (): Promise<HotkeyInfo[]> => (isTauri ? commands.getHotkeys() : Promise.resolve(mock.hotkeys)),
  shortcutLabel: (shortcut: string): Promise<string> =>
    isTauri ? unwrap(commands.shortcutLabel(shortcut)) : Promise.resolve(shortcut),
  pauseHotkeys: (paused: boolean): Promise<void> =>
    isTauri ? commands.pauseHotkeys(paused) : Promise.resolve(),
  chooseFolder: (): Promise<string | null> =>
    isTauri ? commands.chooseFolder() : Promise.resolve("/Users/beispiel/Desktop/Screenshots"),
  diagnostics: (): Promise<Diagnostics> =>
    isTauri
      ? commands.getDiagnostics()
      : Promise.resolve({ version: "0.1.0", platform: "browser", library: "-", logFile: "-", timings: ["Taste → Overlay sichtbar: 18 ms"] }),
  reportBug: (): Promise<null> => (isTauri ? unwrap(commands.reportBug()) : Promise.resolve(null)),
  appInfo: () => (isTauri ? commands.appInfo() : Promise.resolve({ version: "0.1.0", platform: "browser", arch: "" })),
};

/** Ereignis aus dem Backend abonnieren (in der Attrappe: nie). */
export async function on<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
  if (!isTauri) return () => {};
  const { listen } = await import("@tauri-apps/api/event");
  return listen<T>(event, (e) => handler(e.payload));
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
    settings: {
      language: null,
      after_capture: "preview",
      preview_position: "mouse",
      copy_to_clipboard: true,
      scroll_mode: "manual",
      hotkeys: {},
      replace_mac_system_shortcuts: false,
      auto_export: { enabled: false, folder: null, pattern: "FMShot {date} {time}", format: "png" },
      retention_days: null,
    } as Settings,
    hotkeys: [
      { action: "capture_region", shortcut: "Primary+Shift+2", label: "⇧⌘2", conflict: false },
      { action: "capture_screen", shortcut: "Primary+Shift+1", label: "⇧⌘1", conflict: false },
      { action: "capture_window", shortcut: "Primary+Alt+3", label: "⌥⌘3", conflict: false },
      { action: "capture_scroll", shortcut: "Primary+Alt+4", label: "⌥⌘4", conflict: false },
      { action: "capture_text", shortcut: "Primary+Alt+5", label: "⌥⌘5", conflict: false },
      { action: "open_gallery", shortcut: "Primary+Alt+G", label: "⌥⌘G", conflict: false },
    ] as HotkeyInfo[],
    async search(query: string) {
      ensure();
      return captures.filter((c) => !query || c.id.includes(query) || "lorem fehler".includes(query.toLowerCase()));
    },
    fileUrl: (path: string) => {
      const asset = /_asset_(.+)\.png$/.exec(path);
      return (asset ? images.get(`mock/${asset[1]}`) : images.get(path)) ?? path;
    },
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
    async addAsset(bytes: Uint8Array) {
      const hash = `mock${images.size}`;
      const blob = new Blob([bytes as BlobPart]);
      const bmp = await createImageBitmap(blob);
      images.set(`mock/${hash}`, URL.createObjectURL(blob));
      return { hash, width: bmp.width, height: bmp.height };
    },
    async addAssetFromCapture(sourceId: string) {
      const c = await this.getCapture(sourceId);
      const hash = `cap${images.size}`;
      images.set(`mock/${hash}`, images.get(c.originalPath)!);
      return { hash, width: c.width, height: c.height };
    },
    async recognizeText(id: string) {
      const c = await this.getCapture(id);
      return Array.from({ length: 12 }, (_, l) => ({
        text: `Zeile ${l + 1}: Lorem ipsum dolor sit amet, Fehler 0x${(l * 4099).toString(16)}`,
        x: 24,
        y: 110 + l * 40 - 18,
        width: Math.min(c.width - 48, 640),
        height: 24,
      })).concat([{ text: "Kontakt: max.muster@example.com, IBAN CH93 0076 2011 6238 5295 7", x: 24, y: 620, width: 700, height: 24 }]);
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
