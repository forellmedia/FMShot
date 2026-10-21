// Video im Editor (ADR 0011, Etappe 4–6): Wiedergabe, Zeitleiste und
// Bearbeiten. Die Rechnung (Lage, Zeitachse, Schnitte, Zoom, Zeiger) macht
// `grabmark-video` als WASM (`VideoPreview`) – genau wie beim Export.
//
// Alles im Dokument steht in Zeit der Aufnahme („Quelle“); Zeitleiste und
// Wiedergabe zeigen das fertige Video ohne Schnitte („Ausgabe“).

import { api, fileUrl, on, type CaptureInfo } from "$lib/api";
import type { VideoDoc, VideoTracks, WebcamMode, ZoomSection } from "$lib/bindings";
import { t } from "$lib/i18n";
import { VideoPreview } from "$lib/wasm/grabmark_render";
import { wasmModule } from "./renderer";
import { editor } from "./state.svelte";
import { staleLayerTimes } from "./videoLayers";

/** Weicht eine Spur weiter ab, wird sie nachgezogen (Sekunden). */
const MAX_DRIFT_S = 0.15;
/** Normale Zeigerhöhe in Bildschirmpunkten (wie `CURSOR_POINTS` in Rust). */
const CURSOR_POINTS = 24;
/** Neue Pfeile, Texte usw. sind ab der aktuellen Stelle so lange zu sehen. */
export const LAYER_DEFAULT_MS = 4000;

export type SceneLayer = {
  source: "screen" | "webcam" | "cursor" | "click" | "overlay";
  rect: [number, number, number, number];
  crop: [number, number, number, number];
  radius: number;
  mirror: boolean;
  border: { width: number; color: [number, number, number, number] } | null;
  shadow: number;
  opacity: number;
};

export type Scene = { sourceMs: number; layers: SceneLayer[] };

export type EditOp =
  | { op: "cut"; start: number; end: number }
  | { op: "trim"; start: number; end: number }
  | { op: "uncut" }
  | { op: "webcamMode"; start: number; end: number; mode: WebcamMode }
  | { op: "addZoom"; t: number; x: number; y: number }
  | { op: "setZoom"; index: number; zoom: ZoomSection | null }
  | { op: "autoZoom" }
  | { op: "hideClick"; source: number }
  | { op: "showClicks" }
  | { op: "layerTime"; layer: number; range: [number, number] | null };

type Urls = { screen: string | null; webcam: string | null; system: string | null; mic: string | null };

export class VideoPlayer {
  /** Aufnahme, die gerade geladen ist. */
  id = $state<string | null>(null);
  urls = $state<Urls | null>(null);
  playing = $state(false);
  /** Stelle im fertigen Video (ms). */
  outputMs = $state(0);
  durationMs = $state(0);
  /** Was gerade zu sehen ist (für Bühne und Leinwand). */
  scene = $state<Scene>({ sourceMs: 0, layers: [] });
  /** Ausgewählter Bereich auf der Zeitleiste (Ausgabe-Zeit). */
  selection = $state<{ start: number; end: number } | null>(null);
  /**
   * Fortschritt beim Zusammensetzen (0–1) oder `null`. GIF: -1, solange der
   * Speichern-Dialog offen ist (erst dann beginnt die Arbeit).
   */
  exporting = $state<number | null>(null);
  /** Was gerade zusammengesetzt wird (Anzeige über dem Video). */
  exportKind = $state<"video" | "gif">("video");
  /** Sichtbare Klicks in Ausgabe-Zeit (Zeitleiste). */
  clicks = $state<number[]>([]);
  /** Dieselben Klicks in Zeit der Aufnahme (zum Löschen). */
  clickSources = $state<number[]>([]);
  hasInput = $state(false);
  /** Grössen in Pixeln, sobald die Videos geladen sind. */
  screenSize = $state<[number, number] | null>(null);
  cameraSize = $state<[number, number] | null>(null);

  private preview: VideoPreview | null = null;
  private media: { screen?: HTMLVideoElement; webcam?: HTMLVideoElement; system?: HTMLAudioElement; mic?: HTMLAudioElement } = {};
  private frame = 0;
  private docJson = "";
  /** Zählt die Exporte; nur der neueste darf den Zustand setzen. */
  private exportToken = 0;
  /** Ebenen beim letzten Abgleich – neue bekommen einen Zeitbereich. */
  private knownLayers = new Set<number>();

  constructor() {
    void on<{ id: string; progress: number }>("video-export-progress", (p) => {
      if (p.id === this.id && this.exporting != null) this.exporting = p.progress;
    });
  }

  get video(): VideoDoc | null {
    return (editor.doc as { video?: VideoDoc | null } | null)?.video ?? null;
  }

  /** Neue Aufnahme im Editor: Spuren und Mausspur laden. */
  async load(capture: CaptureInfo) {
    if (this.id === capture.id) return;
    this.pause();
    this.id = capture.id;
    this.urls = null;
    this.outputMs = 0;
    this.selection = null;
    this.exporting = null;
    this.screenSize = null;
    this.cameraSize = null;
    this.media = {};
    this.docJson = "";
    this.knownLayers = new Set();
    await wasmModule();
    this.preview ??= new VideoPreview();
    let tracks: VideoTracks;
    try {
      tracks = await api.videoTracks(capture.id);
    } catch (e) {
      editor.flash(String(e));
      return;
    }
    if (this.id !== capture.id) return;
    const url = (p: string | null) => (p ? fileUrl(p) : Promise.resolve(null));
    const [screen, webcam, system, mic] = await Promise.all([
      url(tracks.screen),
      url(tracks.webcam),
      url(tracks.systemAudio),
      url(tracks.microphone),
    ]);
    const text = async (p: string | null) => {
      if (!p) return "";
      try {
        return await (await fetch(await fileUrl(p))).text();
      } catch {
        return "";
      }
    };
    const [input, faces] = await Promise.all([text(tracks.input), text(tracks.faces)]);
    if (this.id !== capture.id) return;
    try {
      this.preview.setInput(input);
    } catch {
      this.preview.setInput("");
    }
    try {
      this.preview.setFaces(faces);
    } catch {
      this.preview.setFaces("");
    }
    this.hasInput = this.preview.hasInput();
    this.urls = { screen, webcam, system, mic };
    this.syncDoc();
  }

  unload() {
    this.pause();
    this.id = null;
    this.urls = null;
    this.docJson = "";
    this.knownLayers = new Set();
  }

  /** Zeit der Aufnahme → Zeit im fertigen Video. */
  outputTime(sourceMs: number): number {
    return this.preview?.outputTime(sourceMs) ?? sourceMs;
  }

  /** Zeit im fertigen Video → Zeit der Aufnahme. */
  sourceTime(outputMs: number): number {
    return this.preview?.sourceTime(outputMs) ?? outputMs;
  }

  /**
   * Neu gezeichnete Ebenen (Pfeil, Text …) erscheinen ab der aktuellen
   * Stelle für einige Sekunden – gehört zum selben Schritt wie das Zeichnen.
   */
  private timeNewLayers() {
    const doc = editor.doc;
    const video = doc?.video;
    if (!doc || !video || !this.preview) return;
    const ids = doc.layers.map((l) => l.id);
    // Zeitbereiche gelöschter Ebenen weg – sonst erbte eine neue Ebene mit
    // derselben Nummer ihren Bereich.
    const stale = staleLayerTimes(doc);
    const fresh = this.knownLayers.size
      ? ids.filter((id) => !this.knownLayers.has(id) && !(video.layer_times ?? []).some((t) => t.layer === id))
      : [];
    this.knownLayers = new Set(ids);
    if (stale.length) {
      editor.preview((d) => {
        const v = d.video!;
        v.layer_times = (v.layer_times ?? []).filter((t) => !stale.includes(t.layer));
      });
    }
    if (!fresh.length) return;
    // Abrunden: Die Ebene soll an der aktuellen Stelle schon zu sehen sein.
    const start = Math.floor(this.preview.sourceTime(this.outputMs));
    const end = Math.ceil(this.preview.sourceTime(Math.min(this.durationMs, this.outputMs + LAYER_DEFAULT_MS)));
    if (end <= start) return;
    editor.preview((d) => {
      const v = d.video!;
      v.layer_times = [...(v.layer_times ?? []), ...fresh.map((layer) => ({ layer, start_ms: start, end_ms: end }))];
    });
  }

  /** Dokument geändert (Bearbeiten, Rückgängig): Rechnung nachführen. */
  syncDoc() {
    this.timeNewLayers();
    const video = this.video;
    if (!this.preview || !video) return;
    const json = JSON.stringify(video);
    if (json === this.docJson) {
      this.updateScene();
      return;
    }
    const first = this.docJson === "";
    this.docJson = json;
    this.preview.setDoc(json);
    this.durationMs = this.preview.duration();
    this.clicks = Array.from(this.preview.clickTimes());
    this.clickSources = Array.from(this.preview.clickSources());
    this.outputMs = Math.min(this.outputMs, this.durationMs);
    this.updateScene();
  }

  attach(kind: "screen" | "webcam" | "system" | "mic", el: HTMLMediaElement | null) {
    if (el) (this.media as Record<string, HTMLMediaElement>)[kind] = el;
    else delete (this.media as Record<string, HTMLMediaElement>)[kind];
  }

  setSizes(screen: [number, number] | null, camera: [number, number] | null) {
    if (screen) this.screenSize = screen;
    if (camera) this.cameraSize = camera;
    this.updateScene();
  }

  /** Lage aller Ebenen zur aktuellen Stelle neu rechnen. */
  updateScene() {
    const doc = editor.doc;
    if (!this.preview || !doc) return;
    if (!(this.video?.camera_only ? this.cameraSize : this.screenSize)) {
      // Videos noch nicht geladen: wenigstens die Zeit stimmt (Ebenen).
      this.scene = { sourceMs: this.preview.sourceTime(this.outputMs), layers: [] };
      return;
    }
    const frame = doc.artboard.rect;
    const cam = this.cameraSize ?? [0, 0];
    // Nur Webcam: Es gibt keinen Bildschirm, die Kamera gibt den Takt.
    const screen = this.screenSize ?? [0, 0];
    this.scene = JSON.parse(
      this.preview.scene(
        this.outputMs,
        frame.width,
        frame.height,
        screen[0],
        screen[1],
        cam[0],
        cam[1],
        CURSOR_POINTS * (doc.pixel_ratio || 1),
      ),
    );
  }

  // --- Wiedergabe -------------------------------------------------------------

  /** Spur, nach der sich die anderen richten: Bildschirm, sonst Webcam. */
  private get master(): HTMLVideoElement | undefined {
    return this.media.screen ?? this.media.webcam;
  }

  play() {
    if (!this.master || !this.preview) return;
    if (this.outputMs >= this.durationMs - 30) this.seek(0);
    this.playing = true;
    for (const el of this.elements()) void el.play().catch(() => {});
    const loop = () => {
      if (!this.playing) return;
      this.tick();
      this.frame = requestAnimationFrame(loop);
    };
    this.frame = requestAnimationFrame(loop);
  }

  pause() {
    this.playing = false;
    cancelAnimationFrame(this.frame);
    for (const el of this.elements()) el.pause();
  }

  toggle() {
    if (this.playing) this.pause();
    else this.play();
  }

  /** Zur Stelle `ms` im fertigen Video springen. */
  seek(ms: number) {
    if (!this.preview) return;
    this.outputMs = Math.max(0, Math.min(ms, this.durationMs));
    const source = this.preview.sourceTime(this.outputMs) / 1000;
    for (const el of this.elements()) {
      if (Number.isFinite(source)) el.currentTime = Math.min(source, el.duration || source);
    }
    this.updateScene();
  }

  private elements(): HTMLMediaElement[] {
    const { screen, webcam, system, mic } = this.media;
    return [screen, webcam, system, mic].filter((e): e is HTMLMediaElement => !!e);
  }

  /** Ein Bild der Wiedergabe: Schnitte überspringen, Spuren angleichen. */
  private tick() {
    const master = this.master;
    if (!master || !this.preview) return;
    let source = master.currentTime * 1000;
    const skip = this.preview.skipCut(source);
    if (skip >= 0) {
      source = skip;
      master.currentTime = skip / 1000;
    }
    this.outputMs = this.preview.outputTime(source);
    if (this.outputMs >= this.durationMs - 5 || master.ended) {
      this.outputMs = this.durationMs;
      this.pause();
    }
    for (const el of [this.media.webcam, this.media.system, this.media.mic]) {
      if (el && el !== master && Math.abs(el.currentTime - source / 1000) > MAX_DRIFT_S) el.currentTime = source / 1000;
    }
    const video = this.video;
    if (this.media.system) this.media.system.volume = Math.min(1, video?.audio?.system ?? 1);
    if (this.media.mic) this.media.mic.volume = Math.min(1, video?.audio?.microphone ?? 1);
    this.updateScene();
  }

  // --- Bearbeiten -------------------------------------------------------------

  /** Bearbeitungsschritt als ein Schritt fürs Rückgängigmachen. */
  edit(op: EditOp) {
    if (!this.preview || !this.video) return;
    const next = JSON.parse(this.preview.edit(JSON.stringify(op))) as VideoDoc;
    editor.commit((d) => {
      (d as { video?: VideoDoc | null }).video = next;
    });
  }

  /** Lautstärke oder Zeiger ändern (direkt im Dokument). */
  update(patch: (v: VideoDoc) => void) {
    editor.commit((d) => {
      const v = (d as { video?: VideoDoc | null }).video;
      if (v) patch(v);
    });
  }

  /**
   * Das fertige Video entsteht erst auf Wunsch (Kopieren, Sichern, GIF,
   * Einzelbild) – nicht nach jeder Änderung. `true`, wenn es zum Dokument
   * passt; `false` bei Abbruch oder Fehler.
   */
  async ready(): Promise<boolean> {
    const id = this.id;
    if (!id) return false;
    await editor.flushSave();
    if (await api.videoIsCurrent(id)) return true;
    const token = ++this.exportToken;
    const latest = () => this.id === id && token === this.exportToken;
    this.exportKind = "video";
    this.exporting = 0;
    try {
      const info = await api.exportVideo(id);
      if (latest()) {
        editor.current = info;
        editor.thumbVersion++;
      }
      return latest();
    } catch (e) {
      // Abgebrochen (Knopf oder neuer Auftrag): kein Fehler.
      if (latest() && !String(e).toLowerCase().includes("abgebrochen")) editor.flash(String(e));
      return false;
    } finally {
      if (latest()) this.exporting = null;
    }
  }

  /** Laufendes Zusammensetzen abbrechen. */
  cancelExport() {
    if (this.id) void api.cancelVideoExport(this.id);
  }

  // --- Aktionen ---------------------------------------------------------------

  async copy() {
    if (!(await this.ready()) || !this.id) return;
    try {
      await api.copyVideo(this.id);
      editor.flash(t("video.copied"));
    } catch (e) {
      editor.flash(String(e));
    }
  }

  async saveAs() {
    if (!(await this.ready()) || !this.id) return;
    try {
      const path = await api.saveVideoAs(this.id);
      if (path) editor.flash(t("video.saved", { path }));
    } catch (e) {
      editor.flash(String(e));
    }
  }

  async gif() {
    if (!this.id) return;
    await editor.flushSave();
    this.exportKind = "gif";
    this.exporting = -1;
    try {
      const path = await api.exportGif(this.id);
      if (path) editor.flash(t("video.saved", { path }));
    } catch (e) {
      if (!String(e).toLowerCase().includes("abgebrochen")) editor.flash(String(e));
    } finally {
      this.exporting = null;
    }
  }

  async snapshot() {
    if (!(await this.ready()) || !this.id) return;
    try {
      await api.videoSnapshot(this.id, this.outputMs);
      editor.flash(t("video.snapshot_saved"));
    } catch (e) {
      editor.flash(String(e));
    }
  }
}

export const player = new VideoPlayer();

/** „1:23“ bzw. „1:23.4“ mit Zehnteln. */
export function formatTime(ms: number, tenths = false): string {
  const total = Math.max(0, ms) / 1000;
  const m = Math.floor(total / 60);
  const s = total - m * 60;
  const sec = tenths ? s.toFixed(1).padStart(4, "0") : String(Math.floor(s)).padStart(2, "0");
  return `${m}:${sec}`;
}
