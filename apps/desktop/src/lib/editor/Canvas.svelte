<script lang="ts">
  // Leinwand: zeichnet das Dokument mit dem WASM-Renderer und setzt die
  // Werkzeuge um. Beim Ziehen werden die Ebenen unter und über der aktiven
  // Ebene einmal gezeichnet und zwischengespeichert; pro Mausbewegung wird
  // nur die aktive Ebene neu gezeichnet (ADR 0002).

  import { onMount } from "svelte";
  import type { WasmRenderer } from "$lib/wasm/fmshot_render";
  import { fileBytes, isTauri } from "$lib/api";
  import { editor } from "./state.svelte";
  import {
    bounds,
    clone,
    hasBoxHandles,
    hitTest,
    newLayer,
    nextStepNumber,
    rectFromPoints,
    textPadding,
    toCss,
    translate,
    withBounds,
    LINE_HEIGHT,
    type Doc,
    type Layer,
    type LayerKind,
    type Point,
    type Rect,
  } from "./model";
  import { measureText, render, renderer } from "./renderer";
  import { LAYER_MARKER } from "./clipboard";

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

  /** Laufende Zieh-Aktion. */
  type Drag =
    | { kind: "create"; id: number; start: Point; before: string }
    | { kind: "move"; id: number; start: Point; orig: LayerKind; before: string }
    | { kind: "handle"; id: number; handle: string; orig: LayerKind; before: string }
    | { kind: "pen"; id: number; before: string }
    | { kind: "pan"; start: Point; panX: number; panY: number }
    | { kind: "crop"; start: Point };
  let drag: Drag | null = null;
  /** Index der aktiven Ebene beim Ziehen (Zwischenspeicher gültig). */
  let session: { index: number; fast: boolean } | null = null;
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

  /** Dokument zum Zeichnen: Text in Bearbeitung wird ausgeblendet. */
  function renderDoc(): { doc: Doc; skip: number } {
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

  function draw(canvas: HTMLCanvasElement, from: number, to: number, background: boolean) {
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
        background,
        skip,
      });
      if (img) ctx.putImageData(img, 0, 0);
    } catch (e) {
      console.error(e);
    }
  }

  function clear(canvas: HTMLCanvasElement) {
    canvas.getContext("2d")!.clearRect(0, 0, canvas.width, canvas.height);
  }

  function fullRender() {
    draw(baseCanvas, 0, -1, true);
    clear(activeCanvas);
    clear(aboveCanvas);
  }

  /** Beginn einer Zieh-Aktion an Ebene `index`: Zwischenspeicher füllen. */
  function beginSession(index: number) {
    if (!wasm || !editor.doc) return;
    const fast = !wasm.hasEffectAbove(JSON.stringify(editor.doc), index);
    session = { index, fast };
    if (fast) {
      draw(baseCanvas, 0, index, true);
      draw(aboveCanvas, index + 1, -1, false);
      draw(activeCanvas, index, index + 1, false);
    }
  }

  function endSession() {
    session = null;
    scheduleRender();
  }

  function scheduleRender() {
    if (frame) return;
    frame = requestAnimationFrame(() => {
      frame = 0;
      if (session?.fast) draw(activeCanvas, session.index, session.index + 1, false);
      else fullRender();
    });
  }

  // Neu zeichnen, wenn sich Dokument oder Ansicht ändern.
  $effect(() => {
    void editor.revision;
    void editor.zoom;
    void editor.panX;
    void editor.panY;
    void editor.editingTextId;
    void width;
    void height;
    scheduleRender();
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
      for (const c of [baseCanvas, activeCanvas, aboveCanvas]) {
        c.width = Math.round(width * dpr);
        c.height = Math.round(height * dpr);
      }
      editor.viewport = { width, height };
      if (first) editor.fit();
      scheduleRender();
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

  function handlesOf(layer: Layer): Handle[] {
    const k = layer.kind;
    if (k.type === "line" || k.type === "arrow") {
      return [
        { name: "from", x: k.from.x, y: k.from.y },
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
    return hs;
  }

  function handleAt(p: Point): string | null {
    const sel = editor.selected;
    if (!sel || sel.locked) return null;
    const tol = 7 / editor.zoom;
    for (const h of handlesOf(sel)) {
      if (Math.abs(h.x - p.x) <= tol && Math.abs(h.y - p.y) <= tol) return h.name;
    }
    return null;
  }

  function applyHandle(orig: LayerKind, handle: string, p: Point, shift: boolean): LayerKind {
    if (orig.type === "line" || orig.type === "arrow") {
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

  function newKind(tool: string, p: Point): LayerKind | null {
    const s = editor.style;
    const stroke = { color: s.color, width: px(s.strokeWidth) };
    const box = { x: p.x, y: p.y, width: 0, height: 0 };
    const textStyle = {
      size: px(s.fontSize),
      color: s.color,
      bold: s.bold,
      background: null,
    };
    switch (tool) {
      case "arrow":
      case "line":
        return { type: tool, from: p, to: p, stroke };
      case "rectangle":
        return { type: "rectangle", bounds: box, stroke, fill: s.fill ? s.color : null, corner_radius: px(4) };
      case "ellipse":
        return { type: "ellipse", bounds: box, stroke, fill: s.fill ? s.color : null };
      case "pen":
        return { type: "pen", points: [p], stroke };
      case "highlight":
        return { type: "highlight", bounds: box, color: s.highlightColor };
      case "blur":
        return { type: "blur", bounds: box, radius: px(s.blurRadius) };
      case "pixelate":
        return { type: "pixelate", bounds: box, block: px(s.pixelBlock) };
      case "spotlight":
        return { type: "spotlight", bounds: box, dim: s.spotlightDim };
      case "magnifier":
        return { type: "magnifier", bounds: box, source: box, stroke: { color: s.color, width: px(3) } };
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
          radius: px(14),
          number: nextStepNumber(editor.doc!),
          fill: s.color,
          text_color: { r: 255, g: 255, b: 255, a: 255 },
        };
      case "text": {
        const style = {
          ...textStyle,
          background: s.textBackground ? { r: 255, g: 255, b: 255, a: 230 } : null,
        };
        const pad = textPadding(style);
        return {
          type: "text",
          bounds: { x: p.x, y: p.y - (style.size * LINE_HEIGHT) / 2 - pad, width: px(40), height: style.size * LINE_HEIGHT + 2 * pad },
          text: "",
          style,
        };
      }
    }
    return null;
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
    if (handle && editor.selected) {
      const sel = editor.selected;
      drag = { kind: "handle", id: sel.id, handle, orig: clone($state.snapshot(sel.kind) as LayerKind), before: editor.snapshot() };
      beginSession(layerIndex(sel.id));
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
      editor.selectedId = hit?.id ?? null;
      if (hit) {
        drag = { kind: "move", id: hit.id, start: p, orig: clone($state.snapshot(hit.kind) as LayerKind), before: editor.snapshot() };
        beginSession(layerIndex(hit.id));
      }
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
    if (tool === "text") {
      editor.finishPreview(before);
      startEditing(layer.id, before);
      return;
    }
    drag = tool === "pen" ? { kind: "pen", id: layer.id, before } : { kind: "create", id: layer.id, start: p, before };
    beginSession(editor.doc.layers.length - 1);
  }

  function onPointerMove(e: PointerEvent) {
    const p = toDoc(e);
    if (!drag) {
      hoverHandle = handleAt(p);
      return;
    }
    switch (drag.kind) {
      case "pan":
        editor.panX = drag.panX - (e.clientX - drag.start.x) / editor.zoom;
        editor.panY = drag.panY - (e.clientY - drag.start.y) / editor.zoom;
        return;
      case "crop":
        cropRect = rectFromPoints(drag.start, p);
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
        const { id, start, orig } = drag;
        const dx = p.x - start.x;
        const dy = p.y - start.y;
        editor.preview((d) => {
          const l = d.layers.find((l) => l.id === id);
          if (l) l.kind = translate(orig, dx, dy);
        });
        return;
      }
      case "handle": {
        const { id, handle, orig } = drag;
        editor.preview((d) => {
          const l = d.layers.find((l) => l.id === id);
          if (l) l.kind = applyHandle(orig, handle, p, e.shiftKey);
        });
        return;
      }
    }
  }

  function onPointerUp() {
    const d = drag;
    drag = null;
    if (!d) return;
    switch (d.kind) {
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
        editor.finishPreview(d.before);
        break;
    }
    endSession();
  }

  /** Bild-Ebene nicht-destruktiv auf `r` (Dokument-Koordinaten) zuschneiden. */
  function cropImageLayer(id: number, r: Rect) {
    editor.updateKind(id, (k) => {
      if (k.type !== "image") return k;
      const b = k.bounds;
      const x0 = Math.max(b.x, r.x);
      const y0 = Math.max(b.y, r.y);
      const x1 = Math.min(b.x + b.width, r.x + r.width);
      const y1 = Math.min(b.y + b.height, r.y + r.height);
      if (x1 - x0 < 2 || y1 - y0 < 2) return k;
      const src = k.crop ?? { x: 0, y: 0, width: k.source_size.width, height: k.source_size.height };
      const sx = src.width / b.width;
      const sy = src.height / b.height;
      return {
        ...k,
        crop: {
          x: src.x + (x0 - b.x) * sx,
          y: src.y + (y0 - b.y) * sy,
          width: (x1 - x0) * sx,
          height: (y1 - y0) * sy,
        },
        bounds: { x: x0, y: y0, width: x1 - x0, height: y1 - y0 },
      };
    });
  }

  // --- Bilder hineinziehen und einfügen ------------------------------------------

  let dropActive = $state(false);

  function onDragOver(e: DragEvent) {
    if (e.dataTransfer?.types.includes("text/x-fmshot-capture")) {
      e.preventDefault();
      dropActive = true;
    }
  }

  function onDrop(e: DragEvent) {
    dropActive = false;
    const id = e.dataTransfer?.getData("text/x-fmshot-capture");
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
    const image = items.find((i) => i.kind === "file" && i.type.startsWith("image/"));
    if (image) {
      e.preventDefault();
      const file = image.getAsFile();
      if (file) editor.addImageBytes(new Uint8Array(await file.arrayBuffer()));
      return;
    }
    if (e.clipboardData?.getData("text/plain") === LAYER_MARKER && editor.pasteLayer()) {
      e.preventDefault();
    }
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
      editor.panX += e.deltaX / editor.zoom;
      editor.panY += e.deltaY / editor.zoom;
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
        const free = measureText(w, value, k.style.size, k.style.bold);
        const inner = Math.max(k.bounds.width - 2 * pad, free.width + 2);
        const wrapped = measureText(w, value, k.style.size, k.style.bold, inner);
        k.text = value;
        k.bounds = { ...k.bounds, width: inner + 2 * pad, height: wrapped.height + 2 * pad };
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
    if (e.key === "Escape" || (e.key === "Enter" && (e.metaKey || e.ctrlKey))) {
      e.preventDefault();
      commitText();
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
    return {
      left: sx(k.bounds.x + pad),
      top: sy(k.bounds.y + pad),
      width: Math.max(20, (k.bounds.width - 2 * pad) * editor.zoom),
      height: Math.max(20, (k.bounds.height - 2 * pad) * editor.zoom),
      fontSize: k.style.size * editor.zoom,
      bold: k.style.bold,
      color: toCss(k.style.color),
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
    if (e.code === "Space" && !(e.target instanceof HTMLTextAreaElement) && !(e.target instanceof HTMLInputElement)) {
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
    const l = editor.selected;
    if (!l || editor.editingTextId === l.id) return null;
    const b = l.kind.type === "callout" ? l.kind.bounds : bounds(l.kind);
    return {
      box: { x: sx(b.x), y: sy(b.y), w: b.width * editor.zoom, h: b.height * editor.zoom },
      line: l.kind.type === "line" || l.kind.type === "arrow",
      handles: l.locked ? [] : handlesOf(l).map((h) => ({ name: h.name, x: sx(h.x), y: sy(h.y) })),
    };
  });

  const cursor = $derived.by(() => {
    if (spaceDown) return "grab";
    if (hoverHandle) {
      const map: Record<string, string> = {
        n: "ns-resize", s: "ns-resize", e: "ew-resize", w: "ew-resize",
        nw: "nwse-resize", se: "nwse-resize", ne: "nesw-resize", sw: "nesw-resize",
      };
      return map[hoverHandle] ?? "move";
    }
    if (editor.tool === "select") return "default";
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
      style:left="{artboard.x}px"
      style:top="{artboard.y}px"
      style:width="{artboard.w}px"
      style:height="{artboard.h}px"
    ></div>
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
      {#if !selection.line}
        <rect
          class="sel"
          x={selection.box.x - 0.5}
          y={selection.box.y - 0.5}
          width={selection.box.w + 1}
          height={selection.box.h + 1}
        />
      {/if}
      {#each selection.handles as h (h.name)}
        <rect class="handle" class:tail={h.name === "tail"} x={h.x - 4.5} y={h.y - 4.5} width="9" height="9" rx="2" />
      {/each}
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
