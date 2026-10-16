// Zustand des Editors: aktuelle Aufnahme, Dokument, Auswahl, Werkzeug, Stil,
// Rückgängig/Wiederholen und Ansicht (Zoom/Verschieben).

import { api, onCaptureSaved, type CaptureInfo } from "$lib/api";
import { t } from "$lib/i18n";
import {
  clone,
  type Color,
  type Doc,
  type Layer,
  type LayerKind,
  PALETTE,
  HIGHLIGHT_YELLOW,
} from "./model";
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
    await onCaptureSaved(async (id) => {
      await this.refreshCaptures();
      await this.open(id);
    });
  }

  async refreshCaptures() {
    try {
      this.captures = await api.listCaptures();
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
