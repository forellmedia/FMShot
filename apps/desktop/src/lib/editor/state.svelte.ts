// Zustand des Editors: aktuelle Aufnahme, Dokument, Auswahl, Werkzeug, Stil,
// Rückgängig/Wiederholen und Ansicht (Zoom/Verschieben).

import { api, on, type AssetInfo, type CaptureInfo, type OcrBlock } from "$lib/api";
import { t } from "$lib/i18n";
import {
  bounds,
  clone,
  newLayer,
  rgb,
  translate,
  union,
  type Color,
  type Doc,
  type Fill,
  type Layer,
  type LayerKind,
  type Point,
  type Rect,
  PALETTE,
  HIGHLIGHT_YELLOW,
} from "./model";
import { findSensitive } from "./redact";
import type { ToolId } from "./tools";

const MAX_HISTORY = 200;
const SAVE_DELAY_MS = 400;

/** Stil für neue Ebenen. Grössen in Bildschirmpunkten (× pixel_ratio = Pixel). */
export type Style = {
  color: Color;
  strokeWidth: number;
  fill: boolean;
  fontSize: number;
  bold: boolean;
  textBackground: boolean;
  blurRadius: number;
  pixelBlock: number;
  spotlightDim: number;
  highlightColor: Color;
};

export class Editor {
  captures = $state<CaptureInfo[]>([]);
  current = $state<CaptureInfo | null>(null);
  doc = $state<Doc | null>(null);
  selectedId = $state<number | null>(null);
  editingTextId = $state<number | null>(null);
  tool = $state<ToolId>("arrow");
  style = $state<Style>({
    color: PALETTE[0],
    strokeWidth: 3,
    fill: false,
    fontSize: 18,
    bold: true,
    textBackground: false,
    blurRadius: 8,
    pixelBlock: 8,
    spotlightDim: 0.55,
    highlightColor: HIGHLIGHT_YELLOW,
  });
  /** CSS-Pixel pro Dokument-Pixel. */
  zoom = $state(1);
  /** Dokument-Koordinate oben links im Sichtfenster. */
  panX = $state(0);
  panY = $state(0);
  /** Wird erhöht, wenn sich Vorschaubilder geändert haben. */
  thumbVersion = $state(0);
  message = $state<string | null>(null);
  layersPanel = $state(false);
  /** Erkannte Textblöcke (Werkzeug „Text kopieren“). */
  ocrBlocks = $state<OcrBlock[] | null>(null);
  busy = $state(false);
  /** Suchbegriff der Galerie (Volltext über erkannten Text). */
  query = $state("");
  settingsOpen = $state(false);
  /** Kopierte Ebenen für ⌘V. */
  private layerClipboard: Layer[] = [];
  /** Zählt jede Änderung am Dokument – die Leinwand zeichnet dann neu. */
  revision = $state(0);

  private undoStack: string[] = [];
  private redoStack: string[] = [];
  canUndo = $state(false);
  canRedo = $state(false);
  private saveTimer: ReturnType<typeof setTimeout> | null = null;
  private messageTimer: ReturnType<typeof setTimeout> | null = null;
  /** Sichtfenster-Grösse in CSS-Pixeln (von der Leinwand gesetzt). */
  viewport = { width: 800, height: 600 };

  get ratio(): number {
    return this.doc?.pixel_ratio ?? 1;
  }

  get selected(): Layer | null {
    return this.doc?.layers.find((l) => l.id === this.selectedId) ?? null;
  }

  // --- Laden ----------------------------------------------------------------

  async init() {
    await this.refreshCaptures();
    if (this.captures.length && !this.current) await this.open(this.captures[0].id);
    await on<string>("capture-saved", () => this.refreshCaptures());
    await on("library-changed", () => this.refreshCaptures());
    await on<string>("open-capture", async (id) => {
      await this.refreshCaptures();
      await this.open(id);
    });
    await on("open-settings", () => (this.settingsOpen = true));
  }

  async refreshCaptures() {
    try {
      this.captures = this.query.trim()
        ? await api.searchCaptures(this.query)
        : await api.listCaptures();
    } catch (e) {
      this.flash(String(e));
    }
  }

  async open(id: string) {
    await this.flushSave();
    try {
      const [info, doc] = await Promise.all([api.getCapture(id), api.loadDocument(id)]);
      this.current = info;
      this.doc = doc;
      this.selectedId = null;
      this.editingTextId = null;
      this.ocrBlocks = null;
      this.undoStack = [];
      this.redoStack = [];
      this.updateHistoryFlags();
      this.fit();
      this.revision++;
    } catch (e) {
      this.flash(String(e));
    }
  }

  // --- Ansicht --------------------------------------------------------------

  /** Ganze Zeichenfläche ins Fenster einpassen (nie grösser als 100 %). */
  fit() {
    if (!this.doc) return;
    const r = this.doc.artboard.rect;
    const margin = 48;
    const z = Math.min(
      (this.viewport.width - margin) / r.width,
      (this.viewport.height - margin) / r.height,
      1 / this.ratio,
    );
    this.zoom = Math.max(0.02, z);
    this.panX = r.x + r.width / 2 - this.viewport.width / 2 / this.zoom;
    this.panY = r.y + r.height / 2 - this.viewport.height / 2 / this.zoom;
  }

  /** Zoom um einen Punkt im Sichtfenster (CSS-Pixel). */
  zoomAt(factor: number, sx = this.viewport.width / 2, sy = this.viewport.height / 2) {
    const z = Math.min(32, Math.max(0.02, this.zoom * factor));
    const dx = this.panX + sx / this.zoom;
    const dy = this.panY + sy / this.zoom;
    this.zoom = z;
    this.panX = dx - sx / z;
    this.panY = dy - sy / z;
  }

  /** 100 % = ein Bildschirmpunkt pro Punkt der Aufnahme. */
  actualSize() {
    this.zoomAt(1 / this.ratio / this.zoom);
  }

  // --- Ändern ---------------------------------------------------------------

  /**
   * Ändert das Dokument als ein Schritt fürs Rückgängigmachen.
   * `before` = Zustand vor einer Zieh-Aktion (sonst der aktuelle).
   */
  commit(mutate: (doc: Doc) => void, before?: string) {
    if (!this.doc) return;
    this.undoStack.push(before ?? JSON.stringify(this.doc));
    if (this.undoStack.length > MAX_HISTORY) this.undoStack.shift();
    this.redoStack = [];
    mutate(this.doc);
    this.changed();
  }

  /** Vorschau während des Ziehens – ohne Verlaufseintrag und ohne Speichern. */
  preview(mutate: (doc: Doc) => void) {
    if (!this.doc) return;
    mutate(this.doc);
    this.revision++;
  }

  /** Nach einer Vorschau-Folge: als einen Schritt festhalten. */
  finishPreview(before: string) {
    if (!this.doc || before === JSON.stringify(this.doc)) return;
    this.undoStack.push(before);
    if (this.undoStack.length > MAX_HISTORY) this.undoStack.shift();
    this.redoStack = [];
    this.changed();
  }

  snapshot(): string {
    return JSON.stringify(this.doc);
  }

  private changed() {
    this.revision++;
    this.updateHistoryFlags();
    this.scheduleSave();
  }

  undo() {
    if (!this.doc || !this.undoStack.length) return;
    this.redoStack.push(JSON.stringify(this.doc));
    this.doc = JSON.parse(this.undoStack.pop()!);
    this.afterHistoryJump();
  }

  redo() {
    if (!this.doc || !this.redoStack.length) return;
    this.undoStack.push(JSON.stringify(this.doc));
    this.doc = JSON.parse(this.redoStack.pop()!);
    this.afterHistoryJump();
  }

  private afterHistoryJump() {
    if (this.selectedId && !this.selected) this.selectedId = null;
    this.editingTextId = null;
    this.changed();
  }

  private updateHistoryFlags() {
    this.canUndo = this.undoStack.length > 0;
    this.canRedo = this.redoStack.length > 0;
  }

  addLayer(layer: Layer, select = true) {
    this.commit((d) => d.layers.push(layer));
    if (select) this.selectedId = layer.id;
  }

  updateLayer(id: number, patch: (l: Layer) => void) {
    this.commit((d) => {
      const l = d.layers.find((l) => l.id === id);
      if (l) patch(l);
    });
  }

  updateKind(id: number, patch: (k: LayerKind) => LayerKind) {
    this.updateLayer(id, (l) => (l.kind = patch(clone(l.kind))));
  }

  deleteSelected() {
    const sel = this.selected;
    if (!sel || sel.locked) return;
    this.commit((d) => (d.layers = d.layers.filter((l) => l.id !== sel.id)));
    this.selectedId = null;
  }

  /** Ebene in der Reihenfolge verschieben (+1 = nach oben). */
  moveLayer(id: number, delta: number) {
    this.commit((d) => {
      const i = d.layers.findIndex((l) => l.id === id);
      const j = Math.max(0, Math.min(d.layers.length - 1, i + delta));
      if (i < 0 || i === j) return;
      const [l] = d.layers.splice(i, 1);
      d.layers.splice(j, 0, l);
    });
  }

  // --- Bilder, Text, Schwärzen, Hintergrund ---------------------------------------

  /** Bild als Ebene einfügen – zentriert auf `at` (oder die Zeichenfläche). */
  insertImage(info: AssetInfo, at?: Point) {
    if (!this.doc) return;
    const board = this.doc.artboard.rect;
    // Höchstens 60 % der Zeichenfläche, nie grösser als das Original.
    const scale = Math.min(1, (board.width * 0.6) / info.width, (board.height * 0.6) / info.height);
    const w = info.width * scale;
    const h = info.height * scale;
    const c = at ?? { x: board.x + board.width / 2, y: board.y + board.height / 2 };
    const layer = newLayer(this.doc, {
      type: "image",
      asset: { source: "asset", hash: info.hash },
      bounds: { x: c.x - w / 2, y: c.y - h / 2, width: w, height: h },
      crop: null,
      source_size: { width: info.width, height: info.height },
      corner_radius: 0,
      shadow: null,
    });
    this.addLayer(layer);
    this.tool = "select";
  }

  async addImageBytes(bytes: Uint8Array, at?: Point) {
    if (!this.current) return;
    try {
      this.insertImage(await api.addAsset(this.current.id, bytes), at);
    } catch (e) {
      this.flash(String(e));
    }
  }

  async addImageFile(path: string, at?: Point) {
    if (!this.current) return;
    try {
      this.insertImage(await api.addAssetFile(this.current.id, path), at);
    } catch (e) {
      this.flash(String(e));
    }
  }

  async addCaptureAsImage(sourceId: string, at?: Point) {
    if (!this.current || sourceId === this.current.id) return;
    try {
      this.insertImage(await api.addAssetFromCapture(this.current.id, sourceId), at);
    } catch (e) {
      this.flash(String(e));
    }
  }

  private async ocr(): Promise<OcrBlock[]> {
    if (!this.current) return [];
    this.busy = true;
    try {
      return await api.recognizeText(this.current.id);
    } finally {
      this.busy = false;
    }
  }

  /** „Text kopieren“: Blöcke anzeigen und alles kopieren. */
  async showText() {
    if (this.ocrBlocks) {
      this.ocrBlocks = null;
      return;
    }
    try {
      const blocks = await this.ocr();
      if (!blocks.length) {
        this.flash(t("ocr.nothing"));
        return;
      }
      this.ocrBlocks = blocks;
      await this.copyText(blocks.map((b) => b.text).join("\n"));
    } catch (e) {
      this.flash(String(e));
    }
  }

  async copyText(text: string) {
    await api.copyText(text);
    this.flash(t("ocr.copied", { count: String(text.length) }));
  }

  /** Auto-Schwärzen: sensible Stellen finden und verpixeln. */
  async autoRedact() {
    try {
      const blocks = await this.ocr();
      const hits = findSensitive(blocks);
      if (!hits.length) {
        this.flash(t("redact.nothing"));
        return;
      }
      const block = 8 * this.ratio;
      this.commit((d) => {
        for (const r of hits) {
          d.layers.push(newLayer(d, { type: "pixelate", bounds: r, block }));
        }
      });
      this.flash(t("redact.done", { count: String(hits.length) }));
    } catch (e) {
      this.flash(String(e));
    }
  }

  /** Bild der Originalaufnahme (unterste Ebene). */
  private originalLayer(doc: Doc): Layer | undefined {
    return doc.layers.find((l) => l.kind.type === "image" && l.kind.asset.source === "original");
  }

  /** Hintergrund/Rahmen: Rand, Verlauf, runde Ecken und Schatten. */
  applyBackground(fill: Fill, paddingPt = 48) {
    this.commit((d) => {
      const pad = paddingPt * d.pixel_ratio;
      const content = d.layers.filter((l) => l.visible).map((l) => bounds(l.kind)).reduce(union);
      d.artboard.rect = {
        x: content.x - pad,
        y: content.y - pad,
        width: content.width + 2 * pad,
        height: content.height + 2 * pad,
      };
      d.artboard.background = fill;
      const img = this.originalLayer(d);
      if (img?.kind.type === "image") {
        img.kind.corner_radius = 10 * d.pixel_ratio;
        img.kind.shadow = { color: rgb(0, 0, 0, 90), blur: 24 * d.pixel_ratio, offset_y: 8 * d.pixel_ratio };
      }
    });
  }

  removeBackground() {
    this.commit((d) => {
      d.artboard.background = null;
      const img = this.originalLayer(d);
      if (img?.kind.type === "image") {
        img.kind.corner_radius = 0;
        img.kind.shadow = null;
        d.artboard.rect = { ...img.kind.bounds };
      }
    });
  }

  /** „An Inhalt anpassen“. */
  fitArtboardToContent() {
    this.commit((d) => {
      const visible = d.layers.filter((l) => l.visible);
      if (visible.length) d.artboard.rect = visible.map((l) => bounds(l.kind)).reduce(union);
    });
  }

  // --- Ebenen kopieren -------------------------------------------------------

  copyLayer(): boolean {
    const sel = this.selected;
    if (!sel || sel.locked) return false;
    this.layerClipboard = [clone($state.snapshot(sel) as Layer)];
    return true;
  }

  /** Kopierte Ebene leicht versetzt einfügen. */
  pasteLayer(): boolean {
    if (!this.doc || !this.layerClipboard.length) return false;
    const offset = 12 * this.ratio;
    let lastId = 0;
    this.commit((d) => {
      for (const l of this.layerClipboard) {
        const copy = newLayer(d, translate(clone(l.kind), offset, offset));
        copy.opacity = l.opacity;
        d.layers.push(copy);
        lastId = copy.id;
      }
    });
    // Nächstes Einfügen wieder ein Stück weiter.
    this.layerClipboard = this.layerClipboard.map((l) => ({ ...l, kind: translate(l.kind, offset, offset) }));
    this.selectedId = lastId;
    return true;
  }

  duplicate() {
    if (this.copyLayer()) this.pasteLayer();
  }

  // --- Speichern & Ausgeben ---------------------------------------------------

  private scheduleSave() {
    if (this.saveTimer) clearTimeout(this.saveTimer);
    this.saveTimer = setTimeout(() => this.flushSave(), SAVE_DELAY_MS);
  }

  async flushSave() {
    if (this.saveTimer) clearTimeout(this.saveTimer);
    this.saveTimer = null;
    if (!this.current || !this.doc) return;
    try {
      await api.saveDocument(this.current.id, $state.snapshot(this.doc) as Doc);
      // Vorschaubild wird im Hintergrund neu gezeichnet.
      setTimeout(() => this.thumbVersion++, 800);
    } catch (e) {
      this.flash(t("editor.save_failed", { error: String(e) }));
    }
  }

  async copy() {
    if (!this.current || !this.doc) return;
    try {
      await api.copyDocument(this.current.id, $state.snapshot(this.doc) as Doc);
      this.flash(t("editor.copied"));
    } catch (e) {
      this.flash(String(e));
    }
  }

  async exportAs(format: "png" | "jpg") {
    if (!this.current || !this.doc) return;
    try {
      const path = await api.exportDocument(this.current.id, $state.snapshot(this.doc) as Doc, format);
      if (path) this.flash(t("editor.exported", { path }));
    } catch (e) {
      this.flash(String(e));
    }
  }

  async deleteCapture(id: string) {
    await api.deleteCapture(id);
    const wasCurrent = this.current?.id === id;
    await this.refreshCaptures();
    if (wasCurrent) {
      this.current = null;
      this.doc = null;
      if (this.captures.length) await this.open(this.captures[0].id);
    }
  }

  async toggleStar(c: CaptureInfo) {
    await api.setStarred(c.id, !c.starred);
    await this.refreshCaptures();
  }

  flash(text: string) {
    this.message = text;
    if (this.messageTimer) clearTimeout(this.messageTimer);
    this.messageTimer = setTimeout(() => (this.message = null), 3000);
  }
}

export const editor = new Editor();
