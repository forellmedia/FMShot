<script lang="ts">
  // Leinwand: zeichnet das Dokument mit dem WASM-Renderer und setzt die
  // Werkzeuge um. Beim Ziehen werden die Ebenen unter und über der aktiven
  // Ebene einmal gezeichnet und zwischengespeichert; pro Mausbewegung wird
  // nur die aktive Ebene neu gezeichnet (ADR 0002).

  import { onMount, untrack } from "svelte";
  import type { WasmRenderer } from "$lib/wasm/grabmark_render";
  import { fileBytes, isTauri } from "$lib/api";
  import { editor } from "./state.svelte";
  import {
    bounds,
    canRotate,
    center,
    clone,
    groupIds,
    hasBoxHandles,
    arrowMid,
    controlThrough,
    hitTest,
    intersects,
    keepInPlace,
    newLayer,
    nextStepNumber,
    normalizeAngle,
    pivot,
    rectFromPoints,
    rotateAround,
    rotatePoint,
    rotationOf,
    textPadding,
    toCss,
    translate,
    union,
    visualBounds,
    withBounds,
    LINE_HEIGHT,
    type Doc,
    type Layer,
    type LayerKind,
    type Point,
    type Rect,
  } from "./model";
  import { measureText, render, renderer } from "./renderer";
  import { LayerCache, type Surface } from "./layerCache";
  import { player } from "./video.svelte";
  import { visibleVideoLayers, withVisible } from "./videoLayers";
  import VideoStage from "./VideoStage.svelte";
  import { snap, type Guides } from "./snap";

  let container: HTMLDivElement;
  let baseCanvas: HTMLCanvasElement;
  let activeCanvas: HTMLCanvasElement;
  let aboveCanvas: HTMLCanvasElement;
  let textarea = $state<HTMLTextAreaElement | null>(null);
  let wasm: WasmRenderer | null = null;
  let loadedAssetsFor = "";
  let width = $state(800);
  let height = $state(600);
  let dpr = 1;
  let spaceDown = $state(false);
  let cropRect = $state<Rect | null>(null);
  let hoverHandle = $state<string | null>(null);
  /** Zeichenwerkzeug über einer verschiebbaren Ebene (Mauszeiger „verschieben“). */
  let hoverMovable = $state(false);
  /** Hilfslinien beim Einrasten (Dokument-Koordinaten). */
  let guides = $state<Guides | null>(null);

  /** Rahmen beim Aufziehen einer Auswahl (Dokument-Koordinaten). */
  let marquee = $state<Rect | null>(null);

  /** Laufende Zieh-Aktion. */
  type Drag =
    | { kind: "create"; id: number; start: Point; before: string }
    | { kind: "move"; origs: Layer[]; start: Point; box: Rect; before: string }
    | { kind: "handle"; id: number; handle: string; orig: LayerKind; rotation: number; before: string }
    | { kind: "rotate"; origs: Layer[]; center: Point; startAngle: number; before: string }
    | { kind: "pen"; id: number; before: string }
    | { kind: "pan"; start: Point; panX: number; panY: number }
    | { kind: "crop"; start: Point }
    | { kind: "textbox"; start: Point }
    | { kind: "marquee"; start: Point; add: boolean };
  let drag: Drag | null = null;
  let frame = 0;
  let editBefore = "";

  // --- Koordinaten ------------------------------------------------------------

  function toDoc(e: { clientX: number; clientY: number }): Point {
    const r = container.getBoundingClientRect();
    return {
      x: editor.panX + (e.clientX - r.left) / editor.zoom,
      y: editor.panY + (e.clientY - r.top) / editor.zoom,
    };
  }

  const sx = (x: number) => (x - editor.panX) * editor.zoom;
  const sy = (y: number) => (y - editor.panY) * editor.zoom;
  /** Pixel des Dokuments für `pt` Bildschirmpunkte (Strichstärken usw.). */
  const px = (pt: number) => pt * editor.ratio;

  function visibleRegion(): Rect {
    return { x: editor.panX, y: editor.panY, width: width / editor.zoom, height: height / editor.zoom };
  }

  // --- Zeichnen ---------------------------------------------------------------

  /** Video: Die Aufnahme liegt als Video darunter; Ebenen nur in ihrer Zeit. */
  const isVideo = $derived(!!editor.current?.videoPath);
  const videoLayers = $derived(isVideo && editor.doc ? visibleVideoLayers(editor.doc, player.scene.sourceMs) : null);

  /** Dokument zum Zeichnen: Text in Bearbeitung wird ausgeblendet. */
  function renderDoc(): { doc: Doc; skip: number } {
    if (videoLayers) return { doc: withVisible(editor.doc!, videoLayers), skip: editor.editingTextId ?? 0 };
    const doc = editor.doc!;
    const editing = editor.editingTextId;
    if (!editing) return { doc, skip: 0 };
    const layer = doc.layers.find((l) => l.id === editing);
    if (layer?.kind.type === "callout") {
      const copy = clone($state.snapshot(doc) as Doc);
      const l = copy.layers.find((l) => l.id === editing)!;
      if (l.kind.type === "callout") l.kind.text = "";
      return { doc: copy, skip: 0 };
    }
    return { doc, skip: editing };
  }

  function draw(canvas: HTMLCanvasElement | undefined, from: number, to: number, background: boolean) {
    if (!canvas) return;
    const ctx = canvas.getContext("2d")!;
    ctx.clearRect(0, 0, canvas.width, canvas.height);
    if (!wasm || !editor.doc || from >= (to < 0 ? editor.doc.layers.length : to)) return;
    const { doc, skip } = renderDoc();
    try {
      const img = render(wasm, {
        doc,
        docJson: JSON.stringify(doc),
        region: visibleRegion(),
        scale: editor.zoom * dpr,
        from,
        to,
        background: background && !isVideo,
        skip,
      });
      if (img) ctx.putImageData(img, 0, 0);
    } catch (e) {
      console.error(e);
    }
  }

  function clear(canvas: HTMLCanvasElement | undefined) {
    canvas?.getContext("2d")!.clearRect(0, 0, canvas.width, canvas.height);
  }

  const surfaces = (): Record<Surface, HTMLCanvasElement> => ({
    base: baseCanvas,
    active: activeCanvas,
    above: aboveCanvas,
  });
  const cache = new LayerCache({
    draw: (s, from, to, background) => draw(surfaces()[s], from, to, background),
    clear: (s) => clear(surfaces()[s]),
  });

  /** Beginn einer Zieh-Aktion an Ebene `index`: Zwischenspeicher füllen. */
  function beginSession(index: number) {
    if (!wasm || !editor.doc) return;
    cache.begin(index, !wasm.hasEffectAbove(JSON.stringify(editor.doc), index));
  }

  /** Zieh-Aktion über mehrere Ebenen: jedes Mal alles neu zeichnen. */
  function beginSlowSession() {
    cache.beginSlow();
  }

  function endSession() {
    cache.end();
    scheduleRender();
  }

  function scheduleRender() {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      cache.render();
    });
  }

  /** Sofort zeichnen (z. B. nach neuer Grösse, sonst bleibt ein Bild lang leer). */
  function renderNow() {
    cancelAnimationFrame(frame);
    frame = 0;
    cache.render();
  }

  // Neu zeichnen, wenn sich das Dokument ändert.
  $effect(() => {
    void editor.revision;
    void editor.editingTextId;
    scheduleRender();
  });

  // Video: andere Ebenen sichtbar (Zeit läuft) – alles neu zeichnen.
  $effect(() => {
    void videoLayers?.join(",");
    untrack(() => {
      cache.invalidate();
      scheduleRender();
    });
  });

  // Video: Dokument geändert → Vorschau nachführen.
  $effect(() => {
    void editor.revision;
    if (isVideo) untrack(() => player.syncDoc());
  });

  // Andere Ansicht: auch die zwischengespeicherten Flächen neu zeichnen.
  $effect(() => {
    void editor.zoom;
    void editor.panX;
    void editor.panY;
    cache.invalidate();
    scheduleRender();
  });

  // Zeichenfläche geändert (Zuschneiden, Präsentation …): Ansicht neu begrenzen.
  $effect(() => {
    void editor.doc?.artboard.rect;
    void editor.revision;
    untrack(() => editor.setPan(editor.panX, editor.panY));
  });

  // Bilder der Aufnahme in den Renderer laden.
  $effect(() => {
    void editor.revision;
    const cap = editor.current;
    const doc = editor.doc;
    if (!cap || !doc) return;
    void loadAssets(cap.id, cap.originalPath, cap.dir, doc);
  });

  async function loadAssets(id: string, original: string, dir: string, doc: Doc) {
    const r = await renderer();
    if (loadedAssetsFor !== id) {
      r.clearAssets();
      loadedAssetsFor = id;
    }
    if (!r.hasAsset("")) r.setAssetPng("", await fileBytes(original));
    for (const l of doc.layers) {
      if (l.kind.type === "image" && l.kind.asset.source === "asset") {
        const hash = l.kind.asset.hash;
        if (!r.hasAsset(hash)) r.setAssetPng(hash, await fileBytes(`${dir}/${id}_asset_${hash}.png`));
      }
    }
    wasm = r;
    scheduleRender();
  }

  // --- Grösse -----------------------------------------------------------------

  onMount(() => {
    renderer().then((r) => (wasm = r));
    const ro = new ResizeObserver(() => {
      dpr = window.devicePixelRatio || 1;
      const first = width === 800 && height === 600;
      width = container.clientWidth;
      height = container.clientHeight;
      // Neue Grösse leert jede Canvas – auch mitten in einer Zieh-Aktion, etwa
      // wenn die Eigenschaftsleiste für die neue Ebene breiter wird.
      for (const c of [baseCanvas, activeCanvas, aboveCanvas]) {
        c.width = Math.round(width * dpr);
        c.height = Math.round(height * dpr);
      }
      cache.invalidate();
      editor.viewport = { width, height };
      if (first) editor.fit();
      else editor.setPan(editor.panX, editor.panY);
      renderNow();
    });
    ro.observe(container);
    return () => ro.disconnect();
  });

  // Nach dem Laden einer Aufnahme einpassen.
  let lastFitFor = "";
  $effect(() => {
    const id = editor.current?.id ?? "";
    if (id && id !== lastFitFor && width > 0) {
      lastFitFor = id;
      editor.fit();
    }
  });

  // --- Griffe -----------------------------------------------------------------

  type Handle = { name: string; x: number; y: number };

  /** Abstand des Drehgriffs über der Auswahl (Bildschirmpunkte). */
  const ROTATE_GAP = 26;

  /** Griffe im eigenen (ungedrehten) Koordinatensystem der Ebene. */
  function handlesOf(layer: Layer): Handle[] {
    const k = layer.kind;
    if (k.type === "line") {
      return [
        { name: "from", x: k.from.x, y: k.from.y },
        { name: "to", x: k.to.x, y: k.to.y },
      ];
    }
    if (k.type === "arrow") {
      const mid = arrowMid(k);
      return [
        { name: "from", x: k.from.x, y: k.from.y },
        { name: "mid", x: mid.x, y: mid.y },
        { name: "to", x: k.to.x, y: k.to.y },
      ];
    }
    const hs: Handle[] = [];
    if (hasBoxHandles(k)) {
      const b = k.type === "callout" ? k.bounds : bounds(k);
      const xs = [b.x, b.x + b.width / 2, b.x + b.width];
      const ys = [b.y, b.y + b.height / 2, b.y + b.height];
      const names = [
        ["nw", "n", "ne"],
        ["w", "", "e"],
        ["sw", "s", "se"],
      ];
      for (let row = 0; row < 3; row++)
        for (let col = 0; col < 3; col++)
          if (names[row][col]) hs.push({ name: names[row][col], x: xs[col], y: ys[row] });
    }
    if (k.type === "callout") hs.push({ name: "tail", x: k.tail.x, y: k.tail.y });
    if (canRotate(k)) {
      const b = k.type === "callout" ? k.bounds : bounds(k);
      hs.push({ name: "rotate", x: b.x + b.width / 2, y: b.y - ROTATE_GAP / editor.zoom });
    }
    return hs;
  }

  /** Griffe auf der Leinwand (gedreht wie die Ebene). */
  function worldHandles(layer: Layer): Handle[] {
    const r = rotationOf(layer);
    const c = pivot(layer.kind);
    return handlesOf(layer).map((h) => ({ ...h, ...rotatePoint(h, r, c) }));
  }

  /** Drehgriff einer Mehrfachauswahl bzw. Gruppe. */
  function groupRotateHandle(): Handle | null {
    if (editor.selected || editor.editableSelection.length < 1) return null;
    const box = editor.selectionBounds();
    if (!box) return null;
    return { name: "rotate", x: box.x + box.width / 2, y: box.y - ROTATE_GAP / editor.zoom };
  }

  function handleAt(p: Point): string | null {
    // Mittelgriff des Pfeils liegt auf der Linie: etwas grosszügiger treffen.
    const near = (h: Handle) => {
      const tol = (h.name === "mid" ? 10 : 7) / editor.zoom;
      return Math.abs(h.x - p.x) <= tol && Math.abs(h.y - p.y) <= tol;
    };
    const sel = editor.selected;
    if (sel) {
      if (sel.locked) return null;
      return worldHandles(sel).find(near)?.name ?? null;
    }
    const g = groupRotateHandle();
    return g && near(g) ? "rotate" : null;
  }

  /** Winkel von `c` nach `p` in Grad (0 = nach rechts, im Uhrzeigersinn). */
  function angle(c: Point, p: Point): number {
    return (Math.atan2(p.y - c.y, p.x - c.x) * 180) / Math.PI;
  }

  function applyHandle(orig: LayerKind, handle: string, p: Point, shift: boolean, rotation = 0): LayerKind {
    if (rotation) {
      // Im eigenen System der Ebene rechnen, danach an ihren Platz rücken.
      const local = rotatePoint(p, -rotation, pivot(orig));
      return keepInPlace(orig, applyHandle(orig, handle, local, shift), rotation);
    }
    if (orig.type === "arrow") {
      // Mitte ziehen biegt den Pfeil; nahe der Geraden rastet er gerade ein.
      if (handle === "mid") return { ...orig, control: controlThrough(orig.from, orig.to, p, 6 / editor.zoom) };
      // Ende ziehen: Biegung wandert zur Hälfte mit, damit die Form bleibt.
      const end = handle === "from" ? orig.from : orig.to;
      const control = orig.control && { x: orig.control.x + (p.x - end.x) / 2, y: orig.control.y + (p.y - end.y) / 2 };
      return handle === "from" ? { ...orig, from: p, control } : { ...orig, to: p, control };
    }
    if (orig.type === "line") {
      return handle === "from" ? { ...orig, from: p } : { ...orig, to: p };
    }
    if (orig.type === "callout" && handle === "tail") return { ...orig, tail: p };
    const b = orig.type === "callout" ? orig.bounds : bounds(orig);
    let x0 = b.x;
    let y0 = b.y;
    let x1 = b.x + b.width;
    let y1 = b.y + b.height;
    if (handle.includes("w")) x0 = p.x;
    if (handle.includes("e")) x1 = p.x;
    if (handle.includes("n")) y0 = p.y;
    if (handle.includes("s")) y1 = p.y;
    let r = rectFromPoints({ x: x0, y: y0 }, { x: x1, y: y1 });
    if (shift && b.width > 0 && b.height > 0) {
      const ratio = b.width / b.height;
      r = { ...r, height: r.width / ratio };
    }
    return withBounds(orig, r);
  }

  // --- Ebenen anlegen -----------------------------------------------------------

  /** Neue Nummern: so gross wie die vorhandenen. */
  function stepRadius(): number {
    const existing = editor.doc?.layers.find((l) => l.kind.type === "step");
    return existing && existing.kind.type === "step" ? existing.kind.radius : px(editor.style.stepRadius);
  }

  function newKind(tool: string, p: Point): LayerKind | null {
    const s = editor.style;
    const stroke = { color: s.color, width: px(s.strokeWidth), style: s.lineStyle };
    const box = { x: p.x, y: p.y, width: 0, height: 0 };
    const textStyle = {
      size: px(s.fontSize),
      color: s.color,
      bold: s.bold,
      background: null,
      corner_radius: null,
      align: "left" as const,
      valign: "top" as const,
    };
    switch (tool) {
      case "arrow":
        return { type: "arrow", from: p, to: p, stroke, head: s.arrowHead, head_size: s.arrowHeadSize, control: null };
      case "line":
        return { type: "line", from: p, to: p, stroke };
      case "rectangle":
        return { type: "rectangle", bounds: box, stroke, fill: s.fill ? s.color : null, corner_radius: px(4) };
      case "ellipse":
        return { type: "ellipse", bounds: box, stroke, fill: s.fill ? s.color : null };
      case "pen":
        // Freihand immer durchgezogen.
        return { type: "pen", points: [p], stroke: { ...stroke, style: "solid" } };
      case "highlight":
        return { type: "highlight", bounds: box, color: s.highlightColor };
      case "blur":
        return { type: "blur", bounds: box, radius: px(s.blurRadius) };
      case "pixelate":
        return { type: "pixelate", bounds: box, block: px(s.pixelBlock) };
      case "spotlight":
        return {
          type: "spotlight",
          bounds: box,
          dim: s.spotlightDim,
          corner_radius: px(s.spotlightRadius),
          feather: px(s.spotlightFeather),
        };
      case "magnifier":
        return {
          type: "magnifier",
          bounds: box,
          source: box,
          stroke: { color: s.color, width: px(3), style: "solid" },
          handle: s.magnifierHandle,
        };
      case "callout":
        return {
          type: "callout",
          bounds: box,
          tail: p,
          text: "",
          style: { ...textStyle, color: { r: 255, g: 255, b: 255, a: 255 } },
          fill: s.color,
        };
      case "step":
        return {
          type: "step",
          center: p,
          radius: stepRadius(),
          number: nextStepNumber(editor.doc!, s.stepStart),
          fill: s.color,
          text_color: { r: 255, g: 255, b: 255, a: 255 },
        };
      case "text": {
        const style = {
          ...textStyle,
          background: s.textBackground,
          corner_radius: px(s.textRadius),
          align: s.textAlign,
          valign: s.textValign,
        };
        const pad = textPadding(style);
        return {
          type: "text",
          bounds: { x: p.x, y: p.y - (style.size * LINE_HEIGHT) / 2 - pad, width: px(40), height: style.size * LINE_HEIGHT + 2 * pad },
          text: "",
          style,
          fixed_width: false,
        };
      }
    }
    return null;
  }

  /** Text anlegen und bearbeiten: im aufgezogenen Kasten `box` oder frei an `p`. */
  function createText(p: Point, box: Rect | null) {
    const kind = newKind("text", p);
    if (!kind || kind.type !== "text" || !editor.doc) return;
    if (box) {
      kind.bounds = { ...box, height: Math.max(box.height, kind.bounds.height) };
      kind.fixed_width = true;
    }
    const before = editor.snapshot();
    const layer = newLayer(editor.doc, kind);
    editor.preview((d) => d.layers.push(layer));
    editor.finishPreview(before);
    startEditing(layer.id, before);
  }

  /** Aufgezogenes Rechteck (Shift = Quadrat). */
  function dragRect(start: Point, p: Point, square: boolean): Rect {
    if (!square) return rectFromPoints(start, p);
    const d = Math.max(Math.abs(p.x - start.x), Math.abs(p.y - start.y));
    return rectFromPoints(start, {
      x: start.x + Math.sign(p.x - start.x || 1) * d,
      y: start.y + Math.sign(p.y - start.y || 1) * d,
    });
  }

  function updateCreated(kind: LayerKind, start: Point, p: Point, shift: boolean): LayerKind {
    switch (kind.type) {
      case "arrow":
      case "line": {
        let to = p;
        if (shift) {
          // In 45°-Schritten einrasten.
          const a = Math.round(Math.atan2(p.y - start.y, p.x - start.x) / (Math.PI / 4)) * (Math.PI / 4);
          const len = Math.hypot(p.x - start.x, p.y - start.y);
          to = { x: start.x + Math.cos(a) * len, y: start.y + Math.sin(a) * len };
        }
        return { ...kind, to };
      }
      case "magnifier": {
        const r = dragRect(start, p, true);
        const cx = r.x + r.width / 2;
        const cy = r.y + r.height / 2;
        // Zweifache Vergrösserung des Bereichs unter der Lupe.
        return { ...kind, bounds: r, source: { x: cx - r.width / 4, y: cy - r.height / 4, width: r.width / 2, height: r.height / 2 } };
      }
      case "callout": {
        const r = dragRect(start, p, shift);
        return { ...kind, bounds: r, tail: { x: r.x + r.width * 0.25, y: r.y + r.height + Math.max(px(24), r.height * 0.6) } };
      }
      default:
        if ("bounds" in kind) return { ...kind, bounds: dragRect(start, p, shift) } as LayerKind;
        return kind;
    }
  }

  function tooSmall(kind: LayerKind): boolean {
    const b = bounds(kind);
    const min = px(3);
    if (kind.type === "line" || kind.type === "arrow") return Math.hypot(b.width, b.height) < min;
    if (kind.type === "pen") return kind.points.length < 2;
    return b.width < min || b.height < min;
  }

  // --- Maus -------------------------------------------------------------------

  function topLayerAt(p: Point): Layer | null {
    const layers = editor.doc?.layers ?? [];
    const tol = 4 / editor.zoom;
    for (let i = layers.length - 1; i >= 0; i--) {
      const l = layers[i];
      if (l.visible && !l.locked && hitTest(l, p, tol)) return l;
    }
    return null;
  }

  function layerIndex(id: number): number {
    return editor.doc?.layers.findIndex((l) => l.id === id) ?? -1;
  }

  function onPointerDown(e: PointerEvent) {
    if (!editor.doc || e.button === 2) return;
    // Kein Fokuswechsel/keine Textauswahl durch Klicks auf die Leinwand.
    e.preventDefault();
    if (editor.editingTextId) {
      commitText();
      return;
    }
    container.setPointerCapture(e.pointerId);
    const p = toDoc(e);

    if (spaceDown || e.button === 1) {
      drag = { kind: "pan", start: { x: e.clientX, y: e.clientY }, panX: editor.panX, panY: editor.panY };
      return;
    }

    // Griffe der Auswahl haben Vorrang – bei jedem Werkzeug.
    const handle = handleAt(p);
    if (handle === "rotate") {
      startRotate(p);
      return;
    }
    if (handle && editor.selected) {
      const sel = editor.selected;
      drag = {
        kind: "handle",
        id: sel.id,
        handle,
        orig: clone($state.snapshot(sel.kind) as LayerKind),
        rotation: rotationOf(sel),
        before: editor.snapshot(),
      };
      // Nummern sind gleich gross: andere ändern sich mit – alles neu zeichnen.
      if (sel.kind.type === "step") beginSlowSession();
      else beginSession(layerIndex(sel.id));
      return;
    }

    const tool = editor.tool;
    if (tool === "crop") {
      drag = { kind: "crop", start: p };
      cropRect = { x: p.x, y: p.y, width: 0, height: 0 };
      return;
    }

    if (tool === "select") {
      const hit = topLayerAt(p);
      if (!hit) {
        // Leere Stelle: Auswahlrahmen aufziehen.
        if (!e.shiftKey) editor.selectedIds = [];
        drag = { kind: "marquee", start: p, add: e.shiftKey };
        marquee = { x: p.x, y: p.y, width: 0, height: 0 };
        return;
      }
      if (e.shiftKey) {
        editor.select(hit.id, { add: true });
        if (!editor.selectedIds.includes(hit.id)) return;
      } else if (e.metaKey || e.ctrlKey) {
        // ⌘-Klick: einzelne Ebene aus einer Gruppe.
        editor.select(hit.id, { single: true });
      } else if (!editor.selectedIds.includes(hit.id)) {
        editor.select(hit.id);
      }
      startMove(p);
      return;
    }

    // Klick auf vorhandenen Text mit dem Text-Werkzeug: bearbeiten.
    if (tool === "text") {
      const hit = topLayerAt(p);
      if (hit && (hit.kind.type === "text" || hit.kind.type === "callout")) {
        startEditing(hit.id);
        return;
      }
    }

    // Text: Kasten aufziehen (oder nur klicken, dann wächst er mit).
    if (tool === "text") {
      drag = { kind: "textbox", start: p };
      marquee = { x: p.x, y: p.y, width: 0, height: 0 };
      return;
    }

    // Zeichenwerkzeug über der Auswahl oder einer Ebene derselben Art:
    // verschieben statt neu zeichnen (z. B. eine eben gezeichnete Lupe).
    const movable = movableAt(p);
    if (movable) {
      if (!editor.selectedIds.includes(movable.id)) editor.select(movable.id);
      startMove(p);
      return;
    }

    const kind = newKind(tool, p);
    if (!kind) return;
    const before = editor.snapshot();
    const layer = newLayer(editor.doc, kind);
    editor.preview((d) => d.layers.push(layer));
    editor.selectedId = layer.id;

    if (tool === "step") {
      editor.finishPreview(before);
      return;
    }
    drag = tool === "pen" ? { kind: "pen", id: layer.id, before } : { kind: "create", id: layer.id, start: p, before };
    beginSession(editor.doc.layers.length - 1);
  }

  /** Gleiche Art wie das Werkzeug? „Unkenntlich machen“ umfasst beide Effekte. */
  function sameKind(type: LayerKind["type"], tool: string): boolean {
    if (tool === "blur" || tool === "pixelate") return type === "blur" || type === "pixelate";
    return type === tool;
  }

  /** Ebene, die ein Zeichenwerkzeug an `p` verschieben würde (statt neu zu zeichnen). */
  function movableAt(p: Point): Layer | null {
    const tool = editor.tool;
    if (tool === "select" || tool === "crop" || tool === "background") return null;
    const hit = topLayerAt(p);
    if (!hit || hit.kind.type === "image") return null;
    return editor.selectedIds.includes(hit.id) || sameKind(hit.kind.type, tool) ? hit : null;
  }

  /** Alle ausgewählten (nicht gesperrten) Ebenen verschieben. */
  function startMove(p: Point) {
    const origs = editor.editableSelection.map((l) => clone($state.snapshot(l) as Layer));
    if (!origs.length) return;
    const box = origs.map(visualBounds).reduce(union);
    drag = { kind: "move", origs, start: p, box, before: editor.snapshot() };
    if (origs.length === 1) beginSession(layerIndex(origs[0].id));
    else beginSlowSession();
  }

  function startRotate(p: Point) {
    const origs = editor.editableSelection.map((l) => clone($state.snapshot(l) as Layer));
    if (!origs.length) return;
    const c = origs.length === 1 ? pivot(origs[0].kind) : center(origs.map(visualBounds).reduce(union));
    drag = { kind: "rotate", origs, center: c, startAngle: angle(c, p), before: editor.snapshot() };
    if (origs.length === 1) beginSession(layerIndex(origs[0].id));
    else beginSlowSession();
  }

  function onPointerMove(e: PointerEvent) {
    const p = toDoc(e);
    if (!drag) {
      hoverHandle = handleAt(p);
      hoverMovable = !hoverHandle && !!movableAt(p);
      return;
    }
    switch (drag.kind) {
      case "pan":
        editor.setPan(drag.panX - (e.clientX - drag.start.x) / editor.zoom, drag.panY - (e.clientY - drag.start.y) / editor.zoom);
        return;
      case "crop":
        cropRect = rectFromPoints(drag.start, p);
        return;
      case "textbox":
        marquee = rectFromPoints(drag.start, p);
        return;
      case "create": {
        const { id, start } = drag;
        editor.preview((d) => {
          const l = d.layers.find((l) => l.id === id);
          if (l) l.kind = updateCreated(l.kind, start, p, e.shiftKey);
        });
        return;
      }
      case "pen": {
        const id = drag.id;
        editor.preview((d) => {
          const l = d.layers.find((l) => l.id === id);
          if (l?.kind.type !== "pen") return;
          const last = l.kind.points[l.kind.points.length - 1];
          if (Math.hypot(p.x - last.x, p.y - last.y) >= px(1.5)) l.kind.points.push(p);
        });
        return;
      }
      case "move": {
        const { origs, start, box } = drag;
        let dx = p.x - start.x;
        let dy = p.y - start.y;
        // Einrasten an Zeichenfläche und anderen Ebenen (⌘ gedrückt = aus).
        if (!e.metaKey && editor.doc) {
          const moving = new Set(origs.map((l) => l.id));
          const targets = [
            editor.doc.artboard.rect,
            ...editor.doc.layers.filter((l) => !moving.has(l.id) && l.visible).map(visualBounds),
          ];
          const s = snap({ ...box, x: box.x + dx, y: box.y + dy }, targets, 6 / editor.zoom);
          dx += s.dx;
          dy += s.dy;
          guides = s.guides.x.length || s.guides.y.length ? s.guides : null;
        } else {
          guides = null;
        }
        const byId = new Map(origs.map((l) => [l.id, l]));
        editor.preview((d) => {
          for (const l of d.layers) {
            const o = byId.get(l.id);
            if (o) l.kind = translate(o.kind, dx, dy);
          }
        });
        return;
      }
      case "handle": {
        const { id, handle, orig, rotation } = drag;
        editor.preview((d) => {
          const l = d.layers.find((l) => l.id === id);
          if (!l) return;
          const k = applyHandle(orig, handle, p, e.shiftKey, rotation);
          // Breite von Hand gesetzt: Text bricht ab jetzt darin um.
          l.kind = k.type === "text" ? { ...k, fixed_width: true } : k;
          if (k.type === "step") {
            for (const o of d.layers) if (o.kind.type === "step" && !o.locked) o.kind.radius = k.radius;
          }
        });
        return;
      }
      case "rotate": {
        const { origs, center: c, startAngle } = drag;
        let delta = angle(c, p) - startAngle;
        if (origs.length === 1) {
          // Eine Ebene: absoluter Winkel, mit ⇧ in 15°-Schritten.
          let r = normalizeAngle(origs[0].rotation + delta);
          if (e.shiftKey) r = normalizeAngle(Math.round(r / 15) * 15);
          const id = origs[0].id;
          editor.preview((d) => {
            const l = d.layers.find((l) => l.id === id);
            if (l) l.rotation = r;
          });
          return;
        }
        if (e.shiftKey) delta = Math.round(delta / 15) * 15;
        const byId = new Map(origs.map((l) => [l.id, l]));
        editor.preview((d) => {
          d.layers = d.layers.map((l) => {
            const o = byId.get(l.id);
            return o ? rotateAround(o, delta, c) : l;
          });
        });
        return;
      }
      case "marquee":
        marquee = rectFromPoints(drag.start, p);
        return;
    }
  }

  function onPointerUp() {
    const d = drag;
    drag = null;
    guides = null;
    if (!d) return;
    switch (d.kind) {
      case "textbox": {
        const r = marquee;
        marquee = null;
        createText(d.start, r && r.width > px(12) ? r : null);
        break;
      }
      case "crop": {
        const r = cropRect;
        cropRect = null;
        if (!r || r.width <= px(4) || r.height <= px(4)) break;
        const sel = editor.selected;
        if (sel && !sel.locked && sel.kind.type === "image") {
          cropImageLayer(sel.id, r);
        } else {
          editor.commit((doc) => (doc.artboard.rect = r));
        }
        break;
      }
      case "create":
      case "pen": {
        const l = editor.doc?.layers.find((l) => l.id === d.id);
        if (l && tooSmall(l.kind)) {
          // Nur geklickt – nichts anlegen.
          editor.preview((doc) => (doc.layers = doc.layers.filter((x) => x.id !== d.id)));
          editor.selectedId = null;
        } else {
          editor.finishPreview(d.before);
          if (l?.kind.type === "callout") startEditing(l.id);
        }
        break;
      }
      case "move":
      case "handle":
      case "rotate":
        editor.finishPreview(d.before);
        break;
      case "marquee": {
        const r = marquee;
        marquee = null;
        const doc = editor.doc;
        if (!r || !doc || (r.width < 2 / editor.zoom && r.height < 2 / editor.zoom)) break;
        const hits = doc.layers.filter((l) => l.visible && !l.locked && intersects(visualBounds(l), r));
        const ids = new Set(d.add ? editor.selectedIds : []);
        for (const l of hits) for (const id of groupIds(doc, l.id)) ids.add(id);
        editor.selectedIds = [...ids];
        break;
      }
    }
    endSession();
  }

  /** Bild-Ebene nicht-destruktiv auf `r` (Dokument-Koordinaten) zuschneiden. */
  function cropImageLayer(id: number, rect: Rect) {
    const layer = editor.doc?.layers.find((l) => l.id === id);
    const rotation = layer ? rotationOf(layer) : 0;
    editor.updateKind(id, (k) => {
      if (k.type !== "image") return k;
      // Gedrehtes Bild: Rahmen ins eigene System der Ebene umrechnen.
      const c = pivot(k);
      const corners = [
        { x: rect.x, y: rect.y },
        { x: rect.x + rect.width, y: rect.y },
        { x: rect.x, y: rect.y + rect.height },
        { x: rect.x + rect.width, y: rect.y + rect.height },
      ].map((q) => rotatePoint(q, -rotation, c));
      const r = corners.map((q) => ({ ...q, width: 0, height: 0 })).reduce(union);
      const b = k.bounds;
      const x0 = Math.max(b.x, r.x);
      const y0 = Math.max(b.y, r.y);
      const x1 = Math.min(b.x + b.width, r.x + r.width);
      const y1 = Math.min(b.y + b.height, r.y + r.height);
      if (x1 - x0 < 2 || y1 - y0 < 2) return k;
      const src = k.crop ?? { x: 0, y: 0, width: k.source_size.width, height: k.source_size.height };
      const sx = src.width / b.width;
      const sy = src.height / b.height;
      const cropped: LayerKind = {
        ...k,
        crop: {
          x: src.x + (x0 - b.x) * sx,
          y: src.y + (y0 - b.y) * sy,
          width: (x1 - x0) * sx,
          height: (y1 - y0) * sy,
        },
        bounds: { x: x0, y: y0, width: x1 - x0, height: y1 - y0 },
      };
      return keepInPlace(k, cropped, rotation);
    });
  }

  // --- Bilder hineinziehen und einfügen ------------------------------------------

  let dropActive = $state(false);

  function onDragOver(e: DragEvent) {
    if (e.dataTransfer?.types.includes("text/x-grabmark-capture")) {
      e.preventDefault();
      dropActive = true;
    }
  }

  function onDrop(e: DragEvent) {
    dropActive = false;
    const id = e.dataTransfer?.getData("text/x-grabmark-capture");
    if (id) {
      e.preventDefault();
      editor.addCaptureAsImage(id, toDoc(e));
    }
  }

  // Dateien aus dem Finder (Tauri liefert Pfade und Position).
  onMount(() => {
    let unlisten: (() => void) | undefined;
    if (isTauri) {
      import("@tauri-apps/api/webview").then(async ({ getCurrentWebview }) => {
        unlisten = await getCurrentWebview().onDragDropEvent((event) => {
          const p = event.payload;
          if (p.type === "over" || p.type === "enter") dropActive = true;
          else if (p.type === "leave") dropActive = false;
          else if (p.type === "drop") {
            dropActive = false;
            const ratio = window.devicePixelRatio || 1;
            const at = toDoc({ clientX: p.position.x / ratio, clientY: p.position.y / ratio });
            for (const path of p.paths) {
              if (/\.(png|jpe?g|webp)$/i.test(path)) editor.addImageFile(path, at);
            }
          }
        });
      });
    }
    return () => unlisten?.();
  });

  async function onPaste(e: ClipboardEvent) {
    const target = e.target as HTMLElement | null;
    if (target && (target.tagName === "TEXTAREA" || target.tagName === "INPUT")) return;
    const items = [...(e.clipboardData?.items ?? [])];
    // Datei sofort holen – nach dem ersten await ist die Zwischenablage weg.
    const file = items.find((i) => i.kind === "file" && i.type.startsWith("image/"))?.getAsFile();
    if (!file && !editor.hasLayerClipboard) return;
    e.preventDefault();
    // Noch die eigene ⌘C-Kopie mit Ebenen drin: die Ebenen einfügen, nicht das Bild.
    if (await editor.clipboardHoldsLayers()) {
      editor.pasteLayer();
      return;
    }
    if (file) editor.addImageBytes(new Uint8Array(await file.arrayBuffer()));
  }

  function onDoubleClick(e: MouseEvent) {
    const hit = topLayerAt(toDoc(e));
    if (hit && (hit.kind.type === "text" || hit.kind.type === "callout")) startEditing(hit.id);
  }

  function onWheel(e: WheelEvent) {
    e.preventDefault();
    const r = container.getBoundingClientRect();
    if (e.ctrlKey || e.metaKey) {
      // Pinch (Trackpad) und ⌘ + Mausrad zoomen.
      editor.zoomAt(Math.exp(-e.deltaY * 0.01), e.clientX - r.left, e.clientY - r.top);
    } else {
      editor.setPan(editor.panX + e.deltaX / editor.zoom, editor.panY + e.deltaY / editor.zoom);
    }
  }

  // --- Text bearbeiten ------------------------------------------------------------

  function startEditing(id: number, before?: string) {
    editBefore = before ?? editor.snapshot();
    editor.selectedId = id;
    editor.editingTextId = id;
  }

  // Textfeld fokussieren, sobald es da ist.
  $effect(() => {
    if (textarea && editor.editingTextId) {
      const el = textarea;
      el.focus();
    }
  });

  function onTextInput() {
    const id = editor.editingTextId;
    if (!id || !textarea || !wasm) return;
    const value = textarea.value;
    const w = wasm;
    editor.preview((d) => {
      const l = d.layers.find((l) => l.id === id);
      if (!l) return;
      const k = l.kind;
      if (k.type === "text") {
        const pad = textPadding(k.style);
        const box = k.bounds.width - 2 * pad;
        const inner = k.fixed_width
          ? Math.max(1, box)
          : Math.max(box, measureText(w, value, k.style.size, k.style.bold).width + 2);
        const wrapped = measureText(w, value, k.style.size, k.style.bold, inner);
        k.text = value;
        // Aufgezogener Kasten wird nur höher, nie niedriger als gezogen.
        const height = wrapped.height + 2 * pad;
        k.bounds = { ...k.bounds, width: inner + 2 * pad, height: k.fixed_width ? Math.max(height, k.bounds.height) : height };
      } else if (k.type === "callout") {
        const pad = k.style.size * 0.5;
        const inner = Math.max(1, k.bounds.width - 2 * pad);
        const m = measureText(w, value, k.style.size, k.style.bold, inner);
        k.text = value;
        k.bounds = { ...k.bounds, height: Math.max(k.bounds.height, m.height + 2 * pad) };
      }
    });
  }

  function commitText() {
    const id = editor.editingTextId;
    if (!id) return;
    editor.editingTextId = null;
    const l = editor.doc?.layers.find((l) => l.id === id);
    if (l?.kind.type === "text" && !l.kind.text.trim()) {
      editor.preview((d) => (d.layers = d.layers.filter((x) => x.id !== id)));
      editor.selectedId = null;
    }
    editor.finishPreview(editBefore);
  }

  function onTextKey(e: KeyboardEvent) {
    e.stopPropagation();
    const mod = e.metaKey || e.ctrlKey;
    if (e.key === "Escape" || (e.key === "Enter" && mod)) {
      e.preventDefault();
      commitText();
      return;
    }
    // Speichern und Kopieren mitten im Schreiben: Text übernehmen, dann wie
    // sonst. ⌘C mit markiertem Text kopiert weiterhin nur diesen.
    const key = e.key.toLowerCase();
    const noTextSelected = textarea?.selectionStart === textarea?.selectionEnd;
    if (mod && (key === "s" || (key === "c" && noTextSelected))) {
      e.preventDefault();
      commitText();
      if (key === "s") editor.saveShortcut(e.shiftKey);
      else void editor.copyShortcut();
    }
  }

  // Textfeld über der Ebene, die bearbeitet wird.
  const editing = $derived.by(() => {
    void editor.revision;
    void editor.zoom;
    void editor.panX;
    void editor.panY;
    const id = editor.editingTextId;
    const l = editor.doc?.layers.find((l) => l.id === id);
    if (!l) return null;
    const k = l.kind;
    if (k.type !== "text" && k.type !== "callout") return null;
    const pad = k.type === "text" ? textPadding(k.style) : k.style.size * 0.5;
    const c = pivot(k);
    return {
      rotation: rotationOf(l),
      // Drehpunkt relativ zur linken oberen Ecke des Textfelds.
      originX: (c.x - k.bounds.x - pad) * editor.zoom,
      originY: (c.y - k.bounds.y - pad) * editor.zoom,
      left: sx(k.bounds.x + pad),
      top: sy(k.bounds.y + pad),
      width: Math.max(20, (k.bounds.width - 2 * pad) * editor.zoom),
      height: Math.max(20, (k.bounds.height - 2 * pad) * editor.zoom),
      fontSize: k.style.size * editor.zoom,
      bold: k.style.bold,
      color: toCss(k.style.color),
      align: k.type === "text" && k.fixed_width ? k.style.align : "left",
      text: k.text,
    };
  });

  // Text erst beim Öffnen ins Feld schreiben (nicht bei jeder Eingabe).
  let lastEditing: number | null = null;
  $effect(() => {
    const id = editor.editingTextId;
    if (id !== lastEditing) {
      lastEditing = id;
      if (id && textarea && editing) textarea.value = editing.text;
    }
  });

  // --- Tasten -------------------------------------------------------------------

  function onKey(e: KeyboardEvent) {
    if (e.type === "keydown" && e.key === "Escape" && editor.editingTextId) {
      commitText();
      return;
    }
    // Im Video gehört die Leertaste der Wiedergabe (Zeitleiste).
    if (
      e.code === "Space" &&
      !isVideo &&
      !(e.target instanceof HTMLTextAreaElement) &&
      !(e.target instanceof HTMLInputElement)
    ) {
      spaceDown = e.type === "keydown";
      if (e.type === "keydown") e.preventDefault();
    }
  }

  // --- Anzeige (SVG) ------------------------------------------------------------

  const artboard = $derived.by(() => {
    void editor.revision;
    void editor.zoom;
    void editor.panX;
    void editor.panY;
    const r = editor.doc?.artboard.rect;
    if (!r) return null;
    return { x: sx(r.x), y: sy(r.y), w: r.width * editor.zoom, h: r.height * editor.zoom };
  });

  const selection = $derived.by(() => {
    void editor.revision;
    void editor.zoom;
    void editor.panX;
    void editor.panY;
    const toScreen = (h: Handle) => ({ name: h.name, x: sx(h.x), y: sy(h.y) });
    const outline = (l: Layer) => {
      const b = l.kind.type === "callout" ? l.kind.bounds : bounds(l.kind);
      const r = rotationOf(l);
      const c = pivot(l.kind);
      return [
        { x: b.x, y: b.y },
        { x: b.x + b.width, y: b.y },
        { x: b.x + b.width, y: b.y + b.height },
        { x: b.x, y: b.y + b.height },
      ]
        .map((p) => rotatePoint(p, r, c))
        .map((p) => `${sx(p.x)},${sy(p.y)}`)
        .join(" ");
    };
    const l = editor.selected;
    if (l) {
      if (editor.editingTextId === l.id) return null;
      const handles = l.locked ? [] : worldHandles(l).map(toScreen);
      const rot = handles.find((h) => h.name === "rotate");
      const top = handles.find((h) => h.name === "n");
      return {
        outlines: l.kind.type === "line" || l.kind.type === "arrow" ? [] : [outline(l)],
        box: null,
        handles,
        stem: rot && top ? { x1: top.x, y1: top.y, x2: rot.x, y2: rot.y } : null,
      };
    }
    const layers = editor.selectedLayers;
    if (!layers.length) return null;
    const b = editor.selectionBounds(layers)!;
    const g = groupRotateHandle();
    const rot = g ? toScreen(g) : null;
    return {
      outlines: layers.map(outline),
      box: { x: sx(b.x), y: sy(b.y), w: b.width * editor.zoom, h: b.height * editor.zoom },
      handles: rot ? [rot] : [],
      stem: rot ? { x1: rot.x, y1: sy(b.y), x2: rot.x, y2: rot.y } : null,
    };
  });

  const cursor = $derived.by(() => {
    if (spaceDown) return "grab";
    if (drag?.kind === "rotate") return "grabbing";
    if (hoverHandle === "rotate") return "grab";
    if (hoverHandle) {
      const map: Record<string, string> = {
        n: "ns-resize", s: "ns-resize", e: "ew-resize", w: "ew-resize",
        nw: "nwse-resize", se: "nwse-resize", ne: "nesw-resize", sw: "nesw-resize",
      };
      return map[hoverHandle] ?? "move";
    }
    if (editor.tool === "select") return "default";
    if (hoverMovable || drag?.kind === "move") return "move";
    if (editor.tool === "text") return "text";
    return "crosshair";
  });

  const ocrView = $derived.by(() => {
    void editor.zoom;
    void editor.panX;
    void editor.panY;
    return editor.ocrBlocks?.map((b) => ({
      x: sx(b.x) - 2,
      y: sy(b.y) - 2,
      w: b.width * editor.zoom + 4,
      h: b.height * editor.zoom + 4,
      text: b.text,
    }));
  });

  const cropView = $derived(
    cropRect ? { x: sx(cropRect.x), y: sy(cropRect.y), w: cropRect.width * editor.zoom, h: cropRect.height * editor.zoom } : null,
  );


</script>

<svelte:window onkeydown={onKey} onkeyup={onKey} onpaste={onPaste} />

<div
  class="canvas"
  bind:this={container}
  style:cursor
  onpointerdown={onPointerDown}
  onpointermove={onPointerMove}
  onpointerup={onPointerUp}
  onpointercancel={onPointerUp}
  ondblclick={onDoubleClick}
  onwheel={onWheel}
  ondragover={onDragOver}
  ondragleave={() => (dropActive = false)}
  ondrop={onDrop}
  class:drop={dropActive}
  role="application"
>
  {#if artboard}
    <div
      class="board"
      class:video={isVideo}
      style:left="{artboard.x}px"
      style:top="{artboard.y}px"
      style:width="{artboard.w}px"
      style:height="{artboard.h}px"
    >
      {#if isVideo}
        <VideoStage zoom={editor.zoom} />
      {/if}
    </div>
  {/if}
  <canvas bind:this={baseCanvas}></canvas>
  <canvas bind:this={activeCanvas}></canvas>
  <canvas bind:this={aboveCanvas}></canvas>

  <svg class="overlay" width={width} height={height}>
    {#if artboard}
      <!-- Ausserhalb der Zeichenfläche abdunkeln -->
      <path
        fill-rule="evenodd"
        fill="var(--outside)"
        d="M0 0H{width}V{height}H0Z M{artboard.x} {artboard.y}h{artboard.w}v{artboard.h}h{-artboard.w}Z"
      />
    {/if}
    {#if selection}
      {#each selection.outlines as points, i (i)}
        <polygon class="sel" class:member={!!selection.box} {points} />
      {/each}
      {#if selection.box}
        <rect
          class="sel group"
          x={selection.box.x - 3.5}
          y={selection.box.y - 3.5}
          width={selection.box.w + 7}
          height={selection.box.h + 7}
        />
      {/if}
      {#if selection.stem}
        <line class="stem" x1={selection.stem.x1} y1={selection.stem.y1} x2={selection.stem.x2} y2={selection.stem.y2} />
      {/if}
      {#each selection.handles as h (h.name)}
        {#if h.name === "rotate"}
          <circle class="handle rotate" cx={h.x} cy={h.y} r="5.5" />
        {:else}
          <rect class="handle" class:tail={h.name === "tail"} x={h.x - 4.5} y={h.y - 4.5} width="9" height="9" rx="2" />
        {/if}
      {/each}
    {/if}
    {#if marquee}
      <rect
        class="marquee"
        x={sx(marquee.x)}
        y={sy(marquee.y)}
        width={marquee.width * editor.zoom}
        height={marquee.height * editor.zoom}
      />
    {/if}
    {#if ocrView}
      {#each ocrView as b, i (i)}
        <rect
          class="ocr"
          x={b.x}
          y={b.y}
          width={b.w}
          height={b.h}
          rx="3"
          role="button"
          tabindex="-1"
          onpointerdown={(e) => {
            e.stopPropagation();
            editor.copyText(b.text);
          }}
        ><title>{b.text}</title></rect>
      {/each}
    {/if}
    {#if guides}
      {#each guides.x as gx (gx)}
        <line class="guide" x1={sx(gx)} x2={sx(gx)} y1="0" y2={height} />
      {/each}
      {#each guides.y as gy (gy)}
        <line class="guide" y1={sy(gy)} y2={sy(gy)} x1="0" x2={width} />
      {/each}
    {/if}
    {#if cropView}
      <rect class="crop" x={cropView.x} y={cropView.y} width={cropView.w} height={cropView.h} />
    {/if}
  </svg>

  {#if editing}
    <textarea
      bind:this={textarea}
      class="text-edit"
      style:left="{editing.left}px"
      style:top="{editing.top}px"
      style:width="{editing.width + 4}px"
      style:height="{editing.height + 4}px"
      style:font-size="{editing.fontSize}px"
      style:font-weight={editing.bold ? 700 : 400}
      style:color={editing.color}
      style:line-height={LINE_HEIGHT}
      style:text-align={editing.align}
      style:transform={editing.rotation ? `rotate(${editing.rotation}deg)` : null}
      style:transform-origin="{editing.originX}px {editing.originY}px"
      oninput={onTextInput}
      onkeydown={onTextKey}
      onblur={commitText}
      onpointerdown={(e) => e.stopPropagation()}
      spellcheck="false"
    ></textarea>
  {/if}
</div>

<style>
  .canvas {
    position: relative;
    overflow: hidden;
    background: var(--canvas-bg);
    touch-action: none;
    user-select: none;
  }
  .board.video {
    background: #000;
    isolation: isolate;
    overflow: hidden;
  }
  .board {
    position: absolute;
    /* Schachbrett zeigt Durchsichtigkeit */
    background-color: #fff;
    background-image:
      linear-gradient(45deg, #e6e6e6 25%, transparent 25%),
      linear-gradient(-45deg, #e6e6e6 25%, transparent 25%),
      linear-gradient(45deg, transparent 75%, #e6e6e6 75%),
      linear-gradient(-45deg, transparent 75%, #e6e6e6 75%);
    background-size: 16px 16px;
    background-position: 0 0, 0 8px, 8px -8px, -8px 0;
    box-shadow: 0 2px 16px rgb(0 0 0 / 0.25);
  }
  canvas,
  .overlay {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    pointer-events: none;
  }
  .sel {
    fill: none;
    stroke: var(--selection);
    stroke-width: 1;
    stroke-dasharray: 4 3;
  }
  .sel.member {
    stroke-dasharray: none;
    stroke-opacity: 0.6;
  }
  .sel.group {
    stroke-dasharray: 2 3;
  }
  .stem {
    stroke: var(--selection);
    stroke-width: 1;
  }
  .handle.rotate {
    fill: var(--selection);
    stroke: #fff;
  }
  .marquee {
    fill: rgb(30 136 229 / 0.08);
    stroke: var(--selection);
    stroke-width: 1;
  }
  .handle {
    fill: #fff;
    stroke: var(--selection);
    stroke-width: 1.5;
  }
  .handle.tail {
    fill: var(--selection);
  }
  .ocr {
    fill: rgb(30 136 229 / 0.12);
    stroke: rgb(30 136 229 / 0.8);
    stroke-width: 1;
    pointer-events: all;
    cursor: copy;
  }
  .ocr:hover {
    fill: rgb(30 136 229 / 0.28);
  }
  .canvas.drop {
    outline: 3px dashed var(--selection);
    outline-offset: -6px;
  }
  .guide {
    stroke: #ff3d7f;
    stroke-width: 1;
  }
  .crop {
    fill: rgb(255 255 255 / 0.08);
    stroke: #fff;
    stroke-width: 1;
    stroke-dasharray: 6 4;
  }
  .text-edit {
    position: absolute;
    margin: -2px 0 0 -2px;
    padding: 0;
    border: 1px dashed var(--selection);
    background: transparent;
    font-family: "Inter", sans-serif;
    resize: none;
    outline: none;
    overflow: hidden;
    white-space: pre-wrap;
    caret-color: var(--selection);
  }
</style>
