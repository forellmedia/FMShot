// Zugriff aufs Backend. Läuft das Frontend im normalen Browser (Entwicklung
// ohne Tauri), liefert eine Attrappe Beispieldaten – so lässt sich die
// Oberfläche mit `npm run dev` ansehen.

import {
  commands,
  type AssetInfo,
  type Camera,
  type CaptureInfo,
  type CaptureMode,
  type ContentSearchStatus,
  type Diagnostics,
  type ExportFormat,
  type GalleryItem,
  type GalleryResult,
  type GalleryQuery,
  type HotkeyInfo,
  type MainView,
  type Microphone,
  type Settings,
  type VideoSettings,
  type VideoTracks,
} from "$lib/bindings";
import { fromBackend, type Doc } from "$lib/editor/model";

export type { AssetInfo, Camera, CaptureInfo, ContentSearchStatus, Diagnostics, GalleryItem, GalleryQuery, GalleryResult, HotkeyInfo, Microphone, Settings, VideoSettings };

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
  clipboardChangeCount: (): Promise<number | null> =>
    isTauri ? commands.clipboardChangeCount() : Promise.resolve(null),
  deleteCapture: (id: string): Promise<null> =>
    isTauri ? unwrap(commands.deleteCapture(id)) : mock.deleteCapture(id),
  setStarred: (id: string, starred: boolean): Promise<null> =>
    isTauri ? unwrap(commands.setStarred(id, starred)) : mock.setStarred(id, starred),
  revealCapture: (id: string): Promise<null> =>
    isTauri ? unwrap(commands.revealCapture(id)) : Promise.resolve(null),
  startCapture: (mode: CaptureMode): Promise<void> =>
    isTauri ? commands.startCapture(mode) : Promise.resolve(),
  queryCaptures: (query: GalleryQuery): Promise<GalleryResult> =>
    isTauri ? unwrap(commands.queryCaptures(query)) : mock.query(query),
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
  systemShortcutsActive: (): Promise<boolean | null> =>
    isTauri ? commands.systemShortcutsActive() : Promise.resolve(true),
  openShortcutSettings: (): Promise<void> =>
    isTauri ? unwrap(commands.openShortcutSettings()).then(() => {}) : Promise.resolve(),
  chooseFolder: (): Promise<string | null> =>
    isTauri ? commands.chooseFolder() : Promise.resolve("/Users/beispiel/Desktop/Screenshots"),
  diagnostics: (): Promise<Diagnostics> =>
    isTauri
      ? commands.getDiagnostics()
      : Promise.resolve({ version: "0.1.0", platform: "browser", library: "-", logFile: "-", timings: ["Taste → Overlay sichtbar: 18 ms"] }),
  contentSearchStatus: (): Promise<ContentSearchStatus> =>
    isTauri ? unwrap(commands.contentSearchStatus()) : Promise.resolve({ available: true, done: 3, total: 4 }),
  reportBug: (): Promise<null> => (isTauri ? unwrap(commands.reportBug()) : Promise.resolve(null)),
  videoTracks: (id: string): Promise<VideoTracks> =>
    isTauri
      ? unwrap(commands.videoTracks(id))
      : Promise.resolve({ screen: `mock://${id}`, webcam: null, systemAudio: null, microphone: null, input: null, faces: null }),
  exportVideo: (id: string): Promise<CaptureInfo> =>
    isTauri ? unwrap(commands.exportVideo(id)) : mock.getCapture(id),
  videoIsCurrent: (id: string): Promise<boolean> => (isTauri ? commands.videoIsCurrent(id) : Promise.resolve(true)),
  cancelVideoExport: (id: string): Promise<void> => (isTauri ? commands.cancelVideoExport(id) : Promise.resolve()),
  exportGif: (id: string): Promise<string | null> =>
    isTauri ? unwrap(commands.exportGif(id)) : Promise.resolve(null),
  videoSnapshot: (id: string, outputMs: number): Promise<CaptureInfo> =>
    isTauri ? unwrap(commands.videoSnapshot(id, outputMs)) : mock.getCapture(id),
  copyVideo: (id: string): Promise<null> => (isTauri ? unwrap(commands.copyVideo(id)) : Promise.resolve(null)),
  saveVideoAs: (id: string): Promise<string | null> =>
    isTauri ? unwrap(commands.saveVideoAs(id)) : Promise.resolve(null),
  openMain: (view: MainView): Promise<void> => (isTauri ? commands.openMain(view) : Promise.resolve()),
  quitApp: (): Promise<void> => (isTauri ? commands.quitApp() : Promise.resolve()),
  listMicrophones: (): Promise<Microphone[]> =>
    isTauri
      ? commands.listMicrophones()
      : Promise.resolve([
          { id: "builtin", name: "MacBook Pro-Mikrofon" },
          { id: "usb", name: "USB-Headset" },
        ]),
  listCameras: (): Promise<Camera[]> =>
    isTauri
      ? commands.listCameras()
      : Promise.resolve([
          { id: "facetime", name: "FaceTime HD-Kamera" },
          { id: "iphone", name: "iPhone-Kamera" },
        ]),
  getLaunchAtLogin: (): Promise<boolean> =>
    isTauri ? commands.getLaunchAtLogin() : Promise.resolve(mock.launchAtLogin),
  setLaunchAtLogin: (enabled: boolean): Promise<null> =>
    isTauri ? unwrap(commands.setLaunchAtLogin(enabled)) : Promise.resolve((mock.launchAtLogin = enabled, null)),
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

  // Stichwörter der Beispiel-Aufnahmen (Schlüssel wie im Backend).
  const MOCK_TAGS = [["code", "terminal"], ["chart", "table"], ["forest", "snow"], ["chat"]];
  const MOCK_GROUPS: Record<string, string> = {
    code: "code",
    terminal: "code",
    chart: "charts",
    table: "charts",
    forest: "nature",
    snow: "nature",
    chat: "apps",
  };

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
        // Über mehrere Tage verteilt, damit die Galerie Tage gruppiert.
        createdAt: Date.now() - (sizes.length - i) * 31 * 3_600_000,
        mode: ["video", "window", "screen", "scroll"][i],
        width: w,
        height: h,
        scaleFactor: 2,
        starred: i === 1,
        originalPath: path,
        thumbnailPath: path,
        dir: "mock",
        durationMs: i === 0 ? 83_000 : null,
        videoPath: i === 0 ? path : null,
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
      save_format: "png",
      content_search: true,
      video: {
        target: "region",
        microphone: true,
        microphone_id: null,
        system_audio: true,
        countdown: true,
        fps: 30,
        webcam: true,
        camera_id: null,
        webcam_layout: {
          position: { type: "bottom_right" },
          size: 0.2,
          shape: "circle",
          corner_radius: 0.12,
          border: { width: 0.025, color: { r: 255, g: 255, b: 255, a: 255 } },
          shadow: true,
          mirror: true,
        },
        camera: { format: "landscape", framing: "full", follow: true, background: "original", mirror: true },
        encoding: { resolution: "original", codec: "h264", quality: "standard" },
      },
    } as Settings,
    launchAtLogin: false,
    hotkeys: [
      { action: "capture_region", shortcut: "Primary+Shift+2", label: "⇧⌘2", conflict: false },
      { action: "capture_screen", shortcut: "Primary+Shift+1", label: "⇧⌘1", conflict: false },
      { action: "capture_window", shortcut: "Primary+Alt+3", label: "⌥⌘3", conflict: false },
      { action: "capture_scroll", shortcut: "Primary+Alt+4", label: "⌥⌘4", conflict: false },
      { action: "capture_text", shortcut: "Primary+Alt+5", label: "⌥⌘5", conflict: false },
      { action: "capture_video", shortcut: "Primary+Shift+6", label: "⇧⌘6", conflict: false },
      { action: "capture_webcam", shortcut: "Primary+Shift+7", label: "⇧⌘7", conflict: false },
      { action: "record_draw", shortcut: "Primary+Shift+8", label: "⇧⌘8", conflict: false },
      { action: "record_pause", shortcut: "Primary+Shift+9", label: "⇧⌘9", conflict: false },
      { action: "open_gallery", shortcut: "Primary+Alt+G", label: "⌥⌘G", conflict: false },
    ] as HotkeyInfo[],
    async query(q: GalleryQuery): Promise<GalleryResult> {
      ensure();
      const text = q.text.trim().toLowerCase();
      const items = captures.map((c, i) => {
        const tags = MOCK_TAGS[i % MOCK_TAGS.length];
        const matched: string[] = [];
        if (text && "lorem fehler zeile".includes(text)) matched.push("text");
        matched.push(...tags.filter((tag) => text && tag.startsWith(text)));
        return { capture: c, tags, matched };
      });
      const filtered = items.filter(
        ({ capture: c, matched }) =>
          (!text || matched.length > 0) &&
          (q.fromMs == null || (c.createdAt ?? 0) >= q.fromMs) &&
          (q.toMs == null || (c.createdAt ?? 0) < q.toMs) &&
          (!q.mode || c.mode === q.mode) &&
          (!q.starredOnly || c.starred),
      );
      // Schnellfilter, die in den Treffern vorkommen (ohne den gewählten).
      const counts = new Map<string, number>();
      for (const item of filtered) {
        for (const g of new Set(item.tags.map((tag) => MOCK_GROUPS[tag]))) counts.set(g, (counts.get(g) ?? 0) + 1);
      }
      const order = ["people", "animals", "nature", "food", "vehicles", "buildings", "documents", "code", "charts", "apps"];
      const groups = order.filter((g) => counts.has(g)).map((group) => ({ group, count: counts.get(group)! }));
      const hits = filtered.filter(({ tags }) => !q.group || tags.some((tag) => MOCK_GROUPS[tag] === q.group));
      hits.sort((a, b) => ((a.capture.createdAt ?? 0) - (b.capture.createdAt ?? 0)) * (q.oldestFirst ? 1 : -1));
      return { items: hits.slice(0, q.limit), groups };
    },
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
      const isVideo = c.mode === "video";
      // Beispiel-Zeichnungen im Video: zwei Pfeile überlappen, eine Nummer, ein Textmarker.
      const red = { r: 229, g: 57, b: 53, a: 255 };
      const stroke = { color: red, width: 6, style: "solid" };
      const drawings = isVideo
        ? [
            { type: "arrow", from: { x: 200, y: 200 }, to: { x: 500, y: 400 }, stroke, head: "filled", head_size: 1.6, control: null },
            { type: "arrow", from: { x: 900, y: 200 }, to: { x: 700, y: 500 }, stroke, head: "filled", head_size: 1.6, control: null },
            { type: "step", center: { x: 300, y: 600 }, radius: 28, number: 1, fill: red, text_color: { r: 255, g: 255, b: 255, a: 255 } },
            { type: "highlight", bounds: { x: 600, y: 650, width: 400, height: 60 }, color: { r: 255, g: 235, b: 59, a: 255 } },
          ]
        : [];
      const video = isVideo
        ? {
            duration_ms: c.durationMs ?? 10_000,
            fps: 30,
            cuts: [{ start_ms: 20_000, end_ms: 25_000 }],
            zooms: [{ start_ms: 5000, end_ms: 9000, x: 0.3, y: 0.3, scale: 2 }],
            layer_times: [
              { layer: 2, start_ms: 2000, end_ms: 8000 },
              { layer: 3, start_ms: 6000, end_ms: 12_000 },
              { layer: 4, start_ms: 30_000, end_ms: 34_000 },
              { layer: 5, start_ms: 40_000, end_ms: 50_000 },
            ],
          }
        : null;
      const extra = drawings.map((kind, i) => ({
        id: i + 2,
        visible: true,
        locked: false,
        opacity: kind.type === "highlight" ? 0.45 : 1,
        rotation: 0,
        kind,
      }));
      return {
        video,
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
          ...extra,
        ],
      } as Doc;
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
