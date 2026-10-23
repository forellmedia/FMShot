// Zustand des Editors: aktuelle Aufnahme, Dokument, Auswahl, Werkzeug, Stil,
// Rückgängig/Wiederholen und Ansicht (Zoom/Verschieben).

import { player } from "./video.svelte";
import { api, on, type AssetInfo, type CaptureInfo, type OcrBlock } from "$lib/api";
import { t } from "$lib/i18n";
import {
  center,
  clone,
  groupIds,
  groupLayers,
  moveBlock,
  newLayer,
  nextGroupId,
  normalizeAngle,
  normalizeGroups,
  rgb,
  rotateAround,
  canRotate,
  translate,
  ungroupLayers,
  union,
  visualBounds,
  type ArrowHead,
  type Color,
  type Corner,
  type Doc,
  type Fill,
  BACKGROUNDS,
  type Layer,
  type LayerKind,
  type LineStyle,
  type Point,
  type TextAlign,
  type VerticalAlign,
  type Rect,
  PALETTE,
  HIGHLIGHT_YELLOW,
} from "./model";
import { findSensitive } from "./redact";
import type { ToolId } from "./tools";

const MAX_HISTORY = 200;
/** Freier Rand neben der Zeichenfläche beim Verschieben (CSS-Pixel). */
const PAN_MARGIN = 40;
const SAVE_DELAY_MS = 400;
/** Länger bleibt der Platzhalter für eine neue Aufnahme nie stehen. */
const LOADING_MAX_MS = 15_000;

/** Stil für neue Ebenen. Grössen in Bildschirmpunkten (× pixel_ratio = Pixel). */
export type Style = {
  color: Color;
  strokeWidth: number;
  /** Linienart für Linie, Pfeil, Rechteck, Ellipse. */
  lineStyle: LineStyle;
  arrowHead: ArrowHead;
  /** Grösse der Pfeilspitze, 1 = normal. */
  arrowHeadSize: number;
  fill: boolean;
  fontSize: number;
  bold: boolean;
  /** Hinterlegung neuer Texte; `null` = ohne. */
  textBackground: Color | null;
  /** Eckenradius der Hinterlegung in Punkten. */
  textRadius: number;
  textAlign: TextAlign;
  textValign: VerticalAlign;
  /** Radius neuer Nummern in Punkten (alle Nummern sind gleich gross). */
  stepRadius: number;
  /** Erste Nummer, wenn noch keine da ist (oder höher als die höchste). */
  stepStart: number;
  /** Zuletzt gewählte Art von „Unkenntlich machen“. */
  obscure: "blur" | "pixelate";
  blurRadius: number;
  pixelBlock: number;
  spotlightDim: number;
  /** Spotlight: Eckenradius und weicher Rand in Punkten. */
  spotlightRadius: number;
  spotlightFeather: number;
  /** Lupengriff; `null` = nur Kreis. */
  magnifierHandle: Corner | null;
  highlightColor: Color;
};

export class Editor {
  captures = $state<CaptureInfo[]>([]);
  current = $state<CaptureInfo | null>(null);
  doc = $state<Doc | null>(null);
  /** Ausgewählte Ebenen (bei einer Gruppe alle Mitglieder). */
  selectedIds = $state<number[]>([]);
  editingTextId = $state<number | null>(null);
  tool = $state<ToolId>("arrow");
  style = $state<Style>({
    color: PALETTE[0],
    strokeWidth: 3,
    lineStyle: "solid",
    arrowHead: "filled",
    arrowHeadSize: 1.6,
    fill: false,
    fontSize: 18,
    bold: true,
    textBackground: null,
    textRadius: 6,
    textAlign: "left",
    textValign: "top",
    stepRadius: 14,
    stepStart: 1,
    obscure: "blur",
    blurRadius: 8,
    pixelBlock: 8,
    spotlightDim: 0.55,
    spotlightRadius: 0,
    spotlightFeather: 0,
    magnifierHandle: "bottom_right",
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
  /** Galerie im Vollbild offen. */
  galleryOpen = $state(false);
  /** Neue Aufnahme wird geladen: Die Leinwand zeigt einen Platzhalter statt
   *  des vorigen Bildes, bis die neue gezeichnet ist. */
  loadingCapture = $state<string | null>(null);
  private loadingTimer: ReturnType<typeof setTimeout> | null = null;
  /** Anzeige der globalen Kürzel je Aktion (vom Backend, je Plattform). */
  hotkeyLabels = $state<Record<string, string>>({});
  /** Offenes Kontextmenü einer Aufnahme (Leiste unten oder Leinwand). */
  /** Kontextmenü einer Aufnahme; `capture` aus der Galerie (steht evtl. nicht in der Leiste). */
  captureMenu = $state<{ x: number; y: number; id: string; capture?: CaptureInfo } | null>(null);
  /** Aufnahme, die die Galerie beim Öffnen zeigen soll („In Galerie zeigen“). */
  galleryFocus = $state<string | null>(null);
  /** Zählt Änderungen an Aufnahmen aus dem Kontextmenü – die Galerie lädt dann neu. */
  galleryRevision = $state(0);
  /** Auswahl des Formats für „Speichern als …“ (⇧⌘S) offen. */
  saveAsOpen = $state(false);
  /** Kopierte Ebenen für ⌘V. */
  private layerClipboard: Layer[] = [];
  /** Änderungszähler der Zwischenablage, als die Ebenen kopiert wurden. */
  private layerClipboardStamp: number | null = null;
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
  /** Ansicht ist eingepasst (nicht von Hand gezoomt): Ändert sich die Grösse
   *  des Fensters, etwa weil die Video-Zeitleiste erscheint, wird neu
   *  eingepasst statt abgeschnitten. */
  fitted = false;
  /** Dialog „Speicher aufräumen“ offen. */
  cleanupOpen = $state(false);

  get ratio(): number {
    return this.doc?.pixel_ratio ?? 1;
  }

  /** Die eine ausgewählte Ebene – null bei keiner oder mehreren. */
  get selectedId(): number | null {
    return this.selectedIds.length === 1 ? this.selectedIds[0] : null;
  }

  /** Genau diese Ebene auswählen (ohne ihre Gruppe), `null` = nichts. */
  set selectedId(id: number | null) {
    this.selectedIds = id == null ? [] : [id];
  }

  get selected(): Layer | null {
    const id = this.selectedId;
    return id == null ? null : (this.doc?.layers.find((l) => l.id === id) ?? null);
  }

  /** Alle ausgewählten Ebenen in Zeichenreihenfolge. */
  get selectedLayers(): Layer[] {
    const set = new Set(this.selectedIds);
    return this.doc?.layers.filter((l) => set.has(l.id)) ?? [];
  }

  /** Ausgewählte Ebenen, die verändert werden dürfen. */
  get editableSelection(): Layer[] {
    return this.selectedLayers.filter((l) => !l.locked);
  }

  /** Ist die Auswahl genau eine ganze Gruppe? */
  get selectedGroup(): number | null {
    const layers = this.selectedLayers;
    const g = layers[0]?.group;
    if (layers.length < 2 || g == null || layers.some((l) => l.group !== g)) return null;
    return this.doc!.layers.filter((l) => l.group === g).length === layers.length ? g : null;
  }

  get canGroup(): boolean {
    return this.editableSelection.length >= 2 && this.selectedGroup == null;
  }

  get canUngroup(): boolean {
    return this.selectedLayers.some((l) => l.group != null);
  }

  /**
   * Ebene auswählen. Gruppen werden als Ganzes gewählt, ausser `single`.
   * `add` = zur Auswahl hinzufügen bzw. wieder entfernen (Umschalttaste).
   */
  select(id: number, opts: { add?: boolean; single?: boolean } = {}) {
    if (!this.doc) return;
    const ids = opts.single ? [id] : groupIds(this.doc, id);
    if (!opts.add) {
      this.selectedIds = ids;
      return;
    }
    const has = this.selectedIds.includes(id);
    this.selectedIds = has
      ? this.selectedIds.filter((x) => !ids.includes(x))
      : [...this.selectedIds, ...ids.filter((x) => !this.selectedIds.includes(x))];
  }

  selectAll() {
    this.selectedIds = this.doc?.layers.filter((l) => l.visible && !l.locked).map((l) => l.id) ?? [];
  }

  // --- Laden ----------------------------------------------------------------

  async init() {
    await this.refreshCaptures();
    if (this.captures.length && !this.current) await this.open(this.captures[0].id);
    await on<string>("capture-saved", () => this.refreshCaptures());
    await on("library-changed", () => this.refreshCaptures());
    // Nach der Aufnahme („Editor öffnen“) oder aus der Vorschau: immer der
    // Editor, auch wenn gerade die Galerie offen ist.
    await on<string>("open-capture", async (id) => {
      this.showLoading(id);
      this.galleryOpen = false;
      await this.refreshCaptures();
      await this.openFromGallery(id);
    });
    // Video gerade aufgenommen: sofort im Editor (spielt die Spuren direkt
    // ab), auf Wunsch danach fertig zusammensetzen und kopieren.
    await on<{ id: string; copy: boolean }>("open-video", async ({ id, copy }) => {
      this.galleryOpen = false;
      await this.refreshCaptures();
      await this.open(id);
      if (!copy || this.current?.id !== id) return;
      await player.load(this.current);
      await player.copy();
    });
    await on("open-gallery", () => (this.galleryOpen = true));
    await on("open-editor", () => (this.galleryOpen = false));
    // Kürzel können in den Einstellungen geändert worden sein.
    void this.loadHotkeys();
    window.addEventListener("focus", () => void this.loadHotkeys());
  }

  async loadHotkeys() {
    try {
      const list = await api.getHotkeys();
      this.hotkeyLabels = Object.fromEntries(list.map((h) => [h.action, h.label]));
    } catch {
      // Ohne Kürzel: Anzeige bleibt leer.
    }
  }

  /** Anzeige des globalen Kürzels einer Aktion, z. B. `capture_region`. */
  hotkey(action: string): string {
    return this.hotkeyLabels[action] ?? "";
  }

  async refreshCaptures() {
    try {
      // Suche wie in der Galerie: erkannter Text und Bildinhalt.
      this.captures = this.query.trim()
        ? (
            await api.queryCaptures({
              text: this.query,
              fromMs: null,
              toMs: null,
              mode: null,
              starredOnly: false,
              oldestFirst: false,
              limit: 500,
              group: null,
            })
          ).items.map((item) => item.capture)
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
      this.selectedIds = [];
      this.editingTextId = null;
      this.ocrBlocks = null;
      this.undoStack = [];
      this.redoStack = [];
      this.updateHistoryFlags();
      this.fit();
      this.revision++;
    } catch (e) {
      this.loadingCapture = null;
      this.flash(String(e));
    }
  }

  /** Platzhalter für `id` zeigen; die Leinwand nimmt ihn weg, sobald das Bild
   *  steht – spätestens nach ein paar Sekunden, falls etwas schiefgeht. */
  showLoading(id: string) {
    this.loadingCapture = id;
    if (this.loadingTimer) clearTimeout(this.loadingTimer);
    this.loadingTimer = setTimeout(() => {
      if (this.loadingCapture === id) this.loadingCapture = null;
    }, LOADING_MAX_MS);
  }

  /** Das Bild von `id` ist gezeichnet. */
  loaded(id: string) {
    if (this.loadingCapture !== id) return;
    this.loadingCapture = null;
    if (this.loadingTimer) clearTimeout(this.loadingTimer);
    this.loadingTimer = null;
  }

  // --- Ansicht --------------------------------------------------------------

  /**
   * Ganze Zeichenfläche ins Fenster einpassen. Bilder nie grösser als 100 %;
   * Videos füllen den Platz ganz aus, wie in einem Abspielprogramm.
   */
  fit() {
    if (!this.doc) return;
    const r = this.doc.artboard.rect;
    const isVideo = !!(this.doc as { video?: unknown }).video;
    const margin = isVideo ? 16 : 48;
    const z = Math.min(
      (this.viewport.width - margin) / r.width,
      (this.viewport.height - margin) / r.height,
      isVideo ? Infinity : 1 / this.ratio,
    );
    this.zoom = Math.max(0.02, z);
    this.fitted = true;
    this.panX = r.x + r.width / 2 - this.viewport.width / 2 / this.zoom;
    this.panY = r.y + r.height / 2 - this.viewport.height / 2 / this.zoom;
  }

  /** Zoom um einen Punkt im Sichtfenster (CSS-Pixel). */
  zoomAt(factor: number, sx = this.viewport.width / 2, sy = this.viewport.height / 2) {
    const z = Math.min(32, Math.max(0.02, this.zoom * factor));
    const dx = this.panX + sx / this.zoom;
    const dy = this.panY + sy / this.zoom;
    this.zoom = z;
    this.fitted = false;
    this.setPan(dx - sx / z, dy - sy / z);
  }

  /**
   * Ansicht verschieben – begrenzt, damit die Zeichenfläche nie aus dem
   * Fenster rutscht: Passt sie ganz hinein, bleibt sie mittig; sonst bleibt
   * neben ihr höchstens ein schmaler Rand frei.
   */
  setPan(x: number, y: number) {
    const r = this.doc?.artboard.rect;
    if (!r) {
      this.panX = x;
      this.panY = y;
      return;
    }
    const margin = PAN_MARGIN / this.zoom;
    const clamp = (pan: number, start: number, size: number, view: number) =>
      size + 2 * margin <= view
        ? start + size / 2 - view / 2
        : Math.min(Math.max(pan, start - margin), start + size + margin - view);
    this.panX = clamp(x, r.x, r.width, this.viewport.width / this.zoom);
    this.panY = clamp(y, r.y, r.height, this.viewport.height / this.zoom);
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
    const ids = new Set(this.doc?.layers.map((l) => l.id));
    this.selectedIds = this.selectedIds.filter((id) => ids.has(id));
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
    // Snapshot statt structuredClone: `l.kind` ist ein `$state`-Proxy, den
    // structuredClone nicht kopieren kann.
    this.updateLayer(id, (l) => (l.kind = patch($state.snapshot(l.kind) as LayerKind)));
  }

  deleteSelected() {
    const ids = new Set(this.editableSelection.map((l) => l.id));
    if (!ids.size) return;
    this.commit((d) => {
      d.layers = d.layers.filter((l) => !ids.has(l.id));
      normalizeGroups(d);
    });
    this.selectedIds = [];
  }

  /**
   * Ebene in der Reihenfolge verschieben (+1 = nach oben). Innerhalb einer
   * Gruppe bleibt sie in der Gruppe; eine einzelne Ebene springt über
   * Nachbargruppen hinweg.
   */
  moveLayer(id: number, delta: 1 | -1) {
    this.commit((d) => {
      const i = d.layers.findIndex((l) => l.id === id);
      if (i < 0) return;
      const g = d.layers[i].group;
      if (g == null) {
        moveBlock(d, [id], delta);
        return;
      }
      const j = i + delta;
      if (d.layers[j]?.group !== g) return;
      [d.layers[i], d.layers[j]] = [d.layers[j], d.layers[i]];
    });
  }

  /** Ganze Gruppe in der Reihenfolge verschieben. */
  moveGroup(group: number, delta: 1 | -1) {
    this.commit((d) => moveBlock(d, d.layers.filter((l) => l.group === group).map((l) => l.id), delta));
  }

  // --- Gruppen, Drehen, Verschieben ------------------------------------------------

  /** ⌘G: Auswahl zu einer Gruppe zusammenfassen. */
  groupSelection() {
    if (!this.canGroup) return;
    const ids = this.editableSelection.map((l) => l.id);
    this.commit((d) => groupLayers(d, ids));
    if (this.doc) this.selectedIds = groupIds(this.doc, ids[0]);
  }

  /** ⇧⌘G: Gruppe der Auswahl auflösen (Ebenen bleiben ausgewählt). */
  ungroupSelection() {
    if (!this.canUngroup) return;
    const ids = this.selectedLayers.map((l) => l.id);
    this.commit((d) => ungroupLayers(d, ids));
  }

  /** Auswahl verschieben (Pfeiltasten). */
  nudgeSelection(dx: number, dy: number) {
    const ids = new Set(this.editableSelection.map((l) => l.id));
    if (!ids.size) return;
    this.commit((d) => {
      for (const l of d.layers) if (ids.has(l.id)) l.kind = translate(l.kind, dx, dy);
    });
  }

  /** Umschliessendes Rechteck der Auswahl (nach Drehung). */
  selectionBounds(layers = this.selectedLayers): Rect | null {
    return layers.length ? layers.map(visualBounds).reduce(union) : null;
  }

  /**
   * Drehung setzen: eine Ebene auf `deg` Grad; mehrere Ebenen bzw. eine
   * Gruppe werden gemeinsam um `deg` weitergedreht (um ihre Mitte).
   */
  setRotation(deg: number) {
    const sel = this.selected;
    if (sel) {
      if (!sel.locked && canRotate(sel.kind)) this.updateLayer(sel.id, (l) => (l.rotation = normalizeAngle(deg)));
      return;
    }
    this.rotateSelectionBy(deg);
  }

  rotateSelectionBy(deg: number) {
    const layers = this.editableSelection;
    const box = this.selectionBounds(layers);
    if (!box || !deg) return;
    const c = center(box);
    const ids = new Set(layers.map((l) => l.id));
    this.commit((d) => {
      d.layers = d.layers.map((l) => (ids.has(l.id) ? rotateAround(l, deg, c) : l));
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
  /** Werkzeug wählen. „Präsentation“ legt gleich einen Hintergrund an. */
  setTool(id: ToolId) {
    this.tool = id;
    if (id === "blur" || id === "pixelate") this.style.obscure = id;
    if (id !== "select") this.selectedId = null;
    if (id === "background" && this.doc && !this.doc.artboard.background) this.applyBackground(BACKGROUNDS[0]);
  }

  /** Grösse aller Nummern (Pixel) – Nummern sind immer gleich gross. */
  setStepRadius(radius: number) {
    this.commit((d) => {
      for (const l of d.layers) if (l.kind.type === "step" && !l.locked) l.kind.radius = radius;
    });
  }

  applyBackground(fill: Fill, paddingPt = 48) {
    this.commit((d) => {
      const pad = paddingPt * d.pixel_ratio;
      const visible = d.layers.filter((l) => l.visible);
      const content = visible.length ? visible.map(visualBounds).reduce(union) : d.artboard.rect;
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
      if (visible.length) d.artboard.rect = visible.map(visualBounds).reduce(union);
    });
  }

  // --- Ebenen kopieren -------------------------------------------------------

  copyLayer(): boolean {
    const layers = this.editableSelection;
    if (!layers.length) return false;
    this.layerClipboard = layers.map((l) => clone($state.snapshot(l) as Layer));
    return true;
  }

  /** Kopierte Ebenen leicht versetzt einfügen (Gruppen bleiben Gruppen). */
  pasteLayer(): boolean {
    if (!this.doc || !this.layerClipboard.length) return false;
    const offset = 12 * this.ratio;
    const ids: number[] = [];
    this.commit((d) => {
      const groups = new Map<number, number>();
      for (const l of this.layerClipboard) {
        const copy = newLayer(d, translate(clone(l.kind), offset, offset));
        copy.opacity = l.opacity;
        copy.rotation = l.rotation;
        if (l.group != null) {
          if (!groups.has(l.group)) groups.set(l.group, nextGroupId(d));
          copy.group = groups.get(l.group);
        }
        d.layers.push(copy);
        ids.push(copy.id);
      }
      normalizeGroups(d);
    });
    // Nächstes Einfügen wieder ein Stück weiter.
    this.layerClipboard = this.layerClipboard.map((l) => ({ ...l, kind: translate(l.kind, offset, offset) }));
    this.selectedIds = ids;
    return true;
  }

  /**
   * ⌘C: immer das fertige Bild in die Zwischenablage – auch mit Auswahl
   * (z. B. direkt nach dem Zeichnen eines Pfeils). Die ausgewählten Ebenen
   * merkt sich der Editor dazu: ⌘V fügt sie ein, solange seither nichts
   * anderes kopiert wurde.
   */
  async copyShortcut() {
    const layers = this.copyLayer();
    this.layerClipboardStamp = null;
    await this.copy();
    if (layers) this.layerClipboardStamp = await api.clipboardChangeCount();
  }

  /** Liegt noch die Kopie aus `copyShortcut` mit Ebenen in der Zwischenablage? */
  async clipboardHoldsLayers(): Promise<boolean> {
    if (!this.layerClipboard.length || this.layerClipboardStamp == null) return false;
    return (await api.clipboardChangeCount()) === this.layerClipboardStamp;
  }

  get hasLayerClipboard() {
    return this.layerClipboard.length > 0;
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
    // Video: das fertige Video als Datei kopieren.
    if (this.current.videoPath) return player.copy();
    try {
      await api.copyDocument(this.current.id, $state.snapshot(this.doc) as Doc);
      this.flash(t("editor.copied"));
    } catch (e) {
      this.flash(String(e));
    }
  }

  /** Speichern im Format aus den Einstellungen (⌘S); ⇧⌘S fragt danach. */
  async save() {
    if (this.current?.videoPath) return player.saveAs();
    const format = (await api.getSettings()).save_format ?? "png";
    await this.exportAs(format);
  }

  /** ⌘S speichert gleich, ⇧⌘S fragt zuerst nach dem Format (Video: immer MP4). */
  saveShortcut(askFormat: boolean) {
    if (askFormat && this.doc && !this.current?.videoPath) this.saveAsOpen = true;
    else void this.save();
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

  /** Löscht eine Aufnahme. War sie offen, kommt die nächste in der Leiste
   * (sonst die vorige); ohne weitere Aufnahmen bleibt der Editor leer. */
  async deleteCapture(id: string) {
    const index = this.captures.findIndex((c) => c.id === id);
    await api.deleteCapture(id);
    const wasCurrent = this.current?.id === id;
    await this.refreshCaptures();
    if (wasCurrent) {
      this.current = null;
      this.doc = null;
      this.selectedIds = [];
      const next = this.captures[Math.min(Math.max(index, 0), this.captures.length - 1)];
      if (next) await this.open(next.id);
    }
  }

  /** Nach dem Aufräumen: Liste neu laden; war die offene Aufnahme dabei,
   *  die nächste zeigen. */
  async afterCleanup(deleted: string[]) {
    const index = this.captures.findIndex((c) => c.id === this.current?.id);
    const wasCurrent = !!this.current && deleted.includes(this.current.id);
    await this.refreshCaptures();
    if (wasCurrent) {
      this.current = null;
      this.doc = null;
      this.selectedIds = [];
      const next = this.captures[Math.min(Math.max(index, 0), this.captures.length - 1)];
      if (next) await this.open(next.id);
    }
  }

  /** Kontextmenü der Aufnahme am Mauszeiger statt des Menüs des WebViews. */
  openCaptureMenu(e: MouseEvent, id: string, capture?: CaptureInfo) {
    e.preventDefault();
    e.stopPropagation();
    this.captureMenu = { x: e.clientX, y: e.clientY, id, capture };
  }

  /** Aufnahme aus der Galerie öffnen. Steht sie wegen der Suche nicht in
   *  der Leiste unten, die Suche dort leeren – sonst fehlt sie dort. */
  async openFromGallery(id: string) {
    this.galleryOpen = false;
    if (!this.captures.some((x) => x.id === id) && this.query) {
      this.query = "";
      await this.refreshCaptures();
    }
    await this.open(id);
  }

  /** Galerie öffnen und dort die Aufnahme zeigen. */
  showInGallery(id: string) {
    this.galleryFocus = id;
    this.galleryOpen = true;
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
