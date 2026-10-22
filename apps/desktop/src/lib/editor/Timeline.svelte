<script lang="ts">
  // Zeitleiste des Video-Editors (ADR 0011, Etappe 4–6): Abspielen, Bereich
  // wählen, Schneiden, Webcam je Abschnitt, Vergrössern (Zoom), Klicks, Ton
  // und Mauszeiger. Pfeile, Rahmen usw. stehen je Werkzeug in einer eigenen
  // Zeile; jede Zeichnung ist ein Block, der sich wählen, verschieben und in
  // der Länge ziehen lässt. Gerechnet wird in `grabmark-video` (WASM).

  import type { WebcamMode } from "$lib/bindings";
  import { t } from "$lib/i18n";
  import Icon from "./Icon.svelte";
  import { editor } from "./state.svelte";
  import { AUDIO_TRACKS, FADE_FRAMES, formatTime, player, type AudioName } from "./video.svelte";
  import { wavePath } from "./audioWave";
  import type { MessageKey } from "$lib/i18n";
  import { dragRange, isBaseLayer, layerLanes, layerTime, type LaneGroup, type TimeBlock } from "./videoLayers";

  let lanes = $state<HTMLDivElement | null>(null);
  let root = $state<HTMLElement | null>(null);
  let width = $state(600);
  let panel = $state<"audio" | "cursor" | null>(null);
  /** Ausgewählter Zoom-Abschnitt (Index in `video.zooms`). */
  let zoomIndex = $state<number | null>(null);
  /** Ausgewählter Klick (Zeit der Aufnahme) – löschen mit Entf. */
  let clickSource = $state<number | null>(null);
  let drag: { start: number; moved: boolean } | null = null;

  const video = $derived(editor.doc?.video ?? null);
  /** Nur Webcam: kein Bildschirm, also weder Webcam-Lage noch Zoom. */
  const cameraOnly = $derived(!!video?.camera_only);
  /** Spalte links für die Namen der Zeilen (Punkte, wie `--gutter`). */
  const GUTTER = 92;
  const duration = $derived(Math.max(1, player.durationMs));
  /** Breite der Spuren ohne Namensspalte. */
  const track = $derived(Math.max(1, width - GUTTER));
  const x = (ms: number) => (ms / duration) * track;

  function timeAt(e: PointerEvent): number {
    const r = lanes!.getBoundingClientRect();
    return Math.max(0, Math.min(duration, ((e.clientX - r.left - GUTTER) / track) * duration));
  }

  function onDown(e: PointerEvent) {
    if (e.button !== 0) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    const ms = timeAt(e);
    drag = { start: ms, moved: false };
    player.pause();
    player.seek(ms);
    player.selection = null;
  }

  function onMove(e: PointerEvent) {
    if (!drag) return;
    const ms = timeAt(e);
    if (Math.abs(x(ms) - x(drag.start)) > 3) drag.moved = true;
    if (drag.moved) player.selection = { start: Math.min(drag.start, ms), end: Math.max(drag.start, ms) };
    player.seek(ms);
  }

  function onUp() {
    drag = null;
  }

  // --- Spuren in Ausgabe-Zeit ---------------------------------------------------

  type Block = { start: number; end: number };

  /** Abschnitte der Webcam als Blöcke mit Modus. */
  const webcamBlocks = $derived.by(() => {
    const sections = video?.webcam?.sections ?? [];
    void player.durationMs;
    return sections.map((s, i) => ({
      mode: s.mode,
      start: player.outputTime(s.start_ms),
      end: i + 1 < sections.length ? player.outputTime(sections[i + 1].start_ms) : player.durationMs,
    }));
  });

  const zoomBlocks = $derived.by(() => {
    void player.durationMs;
    return (video?.zooms ?? []).map((z) => ({ start: player.outputTime(z.start_ms), end: player.outputTime(z.end_ms) }));
  });

  /** Stellen, an denen geschnitten wurde. */
  const cutMarks = $derived.by(() => {
    void player.durationMs;
    return (video?.cuts ?? []).map((c) => player.outputTime(c.start_ms));
  });

  /** Pfeile, Rahmen … als Blöcke, je Werkzeug eine oder mehrere Zeilen. */
  const layerRows = $derived.by(() => {
    const doc = editor.doc;
    if (!doc) return [];
    void player.durationMs;
    return layerLanes(doc, player.durationMs, (ms) => player.outputTime(ms));
  });

  const LANE_LABEL: Record<LaneGroup, MessageKey> = {
    arrow: "video.lane_arrow",
    line: "video.lane_line",
    rectangle: "video.lane_rectangle",
    ellipse: "video.lane_ellipse",
    pen: "video.lane_pen",
    step: "video.lane_step",
    highlight: "video.lane_highlight",
    text: "video.lane_text",
    callout: "video.lane_callout",
    image: "video.lane_image",
    other: "video.lane_other",
  };

  // Block ziehen: verschieben oder am Rand die Länge ändern. Übernommen wird
  // erst beim Loslassen (ein Schritt fürs Rückgängigmachen).
  let blockDrag: { block: TimeBlock; mode: "move" | "start" | "end"; x0: number; moved: boolean } | null = null;
  let dragPreview = $state<{ layer: number; start: number; end: number } | null>(null);

  function blockRange(b: TimeBlock): { start: number; end: number } {
    return dragPreview?.layer === b.layer ? dragPreview : b;
  }

  function blockDown(e: PointerEvent, b: TimeBlock, mode: "move" | "start" | "end") {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    player.pause();
    editor.select(b.layer, { single: true });
    blockDrag = { block: b, mode, x0: e.clientX, moved: false };
  }

  function blockMove(e: PointerEvent) {
    if (!blockDrag) return;
    const dx = e.clientX - blockDrag.x0;
    if (Math.abs(dx) > 2) blockDrag.moved = true;
    if (!blockDrag.moved) return;
    const range = dragRange(blockDrag.block, blockDrag.mode, (dx / track) * duration, duration);
    dragPreview = { layer: blockDrag.block.layer, ...range };
    // Beim Ziehen eines Rands zeigt die Vorschau die Stelle des Rands.
    player.seek(blockDrag.mode === "end" ? range.end - 1 : range.start);
  }

  function blockUp(e: PointerEvent) {
    e.stopPropagation();
    const d = blockDrag;
    blockDrag = null;
    if (!d) return;
    if (d.moved && dragPreview) {
      player.edit({ op: "layerTime", layer: d.block.layer, range: [dragPreview.start, dragPreview.end] });
    } else if (player.outputMs < d.block.start || player.outputMs >= d.block.end) {
      // Nur geklickt: zur Stelle springen, an der die Zeichnung zu sehen ist.
      player.seek(d.block.start);
    }
    dragPreview = null;
  }

  const layer = $derived(editor.selected && !isBaseLayer(editor.selected) ? editor.selected : null);
  const layerBlock = $derived.by((): Block | null => {
    if (!layer || !editor.doc) return null;
    void player.durationMs;
    const tm = layerTime(editor.doc, layer.id);
    return tm ? { start: player.outputTime(tm.start_ms), end: player.outputTime(tm.end_ms) } : { start: 0, end: player.durationMs };
  });

  // --- Ton: Wellenform, Lautstärke-Punkte -------------------------------------

  /** Höhe einer Tonspur-Zeile (Punkte, wie in `.lane.audio`). */
  const AUDIO_H = 36;
  const AUDIO_LABEL: Record<AudioName, MessageKey> = { system: "video.track_system", mic: "video.track_mic" };
  const audioLanes = $derived(AUDIO_TRACKS.filter((n) => !!(n === "system" ? player.urls?.system : player.urls?.mic)));

  const waves = $derived.by(() => {
    void video?.cuts;
    const out: Record<string, string> = {};
    for (const n of audioLanes) {
      const p = player.peaks[n];
      out[n] = p ? wavePath(p, track, AUDIO_H, duration, (ms) => player.sourceTime(ms)) : "";
    }
    return out;
  });

  const base = (n: AudioName) => Math.min(1, n === "system" ? (video?.audio?.system ?? 1) : (video?.audio?.microphone ?? 1));
  const yOf = (v: number) => 3 + (1 - Math.max(0, Math.min(1, v))) * (AUDIO_H - 6);
  const vOf = (y: number) => Math.max(0, Math.min(1, 1 - (y - 3) / (AUDIO_H - 6)));

  /** Lautstärkelinie als Punkte für `<polyline>`. */
  function curvePoints(n: AudioName): string {
    const c = player.volume[n].curve;
    if (!c.length) return `0,${yOf(base(n))} ${track},${yOf(base(n))}`;
    return c.map((p) => `${x(p.t).toFixed(1)},${yOf(p.v).toFixed(1)}`).join(" ");
  }

  /** Ausgewählter Lautstärke-Punkt – löschen mit Entf. */
  let selectedKey = $state<{ track: AudioName; index: number } | null>(null);
  let keyDrag: { track: AudioName; index: number; rect: DOMRect; moved: boolean; x0: number; y0: number } | null = null;
  let keyPreview = $state<{ track: AudioName; index: number; t: number; v: number } | null>(null);

  /** Zeit und Lautstärke an der Mausposition in einer Tonspur-Zeile. */
  function audioPoint(e: PointerEvent | MouseEvent, rect: DOMRect, n: AudioName) {
    const t = Math.max(0, Math.min(duration, ((e.clientX - rect.left) / track) * duration));
    const total = vOf(e.clientY - rect.top);
    const b = base(n);
    return { t, total, v: b > 0 ? Math.min(1, total / b) : 0 };
  }

  function keyDown(e: PointerEvent, n: AudioName, index: number) {
    if (e.button !== 0) return;
    e.stopPropagation();
    e.preventDefault();
    const lane = (e.currentTarget as HTMLElement).closest(".audio-area") as HTMLElement;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    player.pause();
    editor.selectedIds = [];
    clickSource = null;
    selectedKey = { track: n, index };
    keyDrag = { track: n, index, rect: lane.getBoundingClientRect(), moved: false, x0: e.clientX, y0: e.clientY };
  }

  function keyMove(e: PointerEvent) {
    const d = keyDrag;
    if (!d) return;
    if (Math.abs(e.clientX - d.x0) + Math.abs(e.clientY - d.y0) > 2) d.moved = true;
    if (!d.moved) return;
    const p = audioPoint(e, d.rect, d.track);
    keyPreview = { track: d.track, index: d.index, t: p.t, v: p.total };
  }

  function keyUp(e: PointerEvent) {
    e.stopPropagation();
    const d = keyDrag;
    keyDrag = null;
    if (d?.moved && keyPreview) {
      const b = base(d.track);
      player.edit({ op: "volumeKey", track: d.track, index: d.index, t: keyPreview.t, volume: b > 0 ? keyPreview.v / b : 0 });
      selectedKey = null;
    } else if (d) {
      player.seek(player.volume[d.track].keys[d.index]?.t ?? player.outputMs);
    }
    keyPreview = null;
  }

  /**
   * Doppelklick in eine Tonspur: neuer Punkt an dieser Stelle. Die Zeitleiste
   * hält den Zeiger fest (Ziehen zum Auswählen), darum kommt der Doppelklick
   * dort an – die Spur ergibt sich aus der Höhe.
   */
  function onDoubleClick(e: MouseEvent) {
    for (const el of lanes?.querySelectorAll<HTMLElement>(".audio-area") ?? []) {
      const rect = el.getBoundingClientRect();
      if (e.clientY < rect.top || e.clientY > rect.bottom) continue;
      const n = el.dataset.track as AudioName;
      const p = audioPoint(e, rect, n);
      player.edit({ op: "volumeKey", track: n, index: null, t: p.t, volume: p.v });
      player.selection = null;
      return;
    }
  }

  function removeKey(n: AudioName, index: number) {
    player.edit({ op: "removeVolumeKey", track: n, index });
    selectedKey = null;
  }

  function keyPos(n: AudioName, index: number, k: { t: number; v: number }) {
    if (keyPreview?.track === n && keyPreview.index === index) return { t: keyPreview.t, y: yOf(keyPreview.v) };
    return { t: k.t, y: yOf(k.v * base(n)) };
  }

  // --- Blenden am Anfang und Ende ----------------------------------------------

  /** `checked` als Funktion bleibt aktuell, solange das Menü offen ist; `keep` lässt es offen. */
  type MenuItem = { label: string; checked?: boolean | (() => boolean); keep?: boolean; action: () => void };
  let menu = $state<{ x: number; y: number; title: string; items: MenuItem[] } | null>(null);

  /** Mausposition relativ zur Zeitleiste: Ihr Glas-Effekt (`backdrop-filter`)
   *  macht sie zum Bezugsrahmen, auch für `position: fixed`. */
  function menuAt(e: MouseEvent) {
    const r = root?.getBoundingClientRect();
    return { x: e.clientX - (r?.left ?? 0), y: e.clientY - (r?.top ?? 0) };
  }

  function fadeLabel(frames: number): string {
    if (!frames) return t("video.fade_off");
    const fps = video?.fps || 30;
    return t("video.fade_frames", { n: String(frames), s: (frames / fps).toFixed(1) });
  }

  function fadeMenu(e: MouseEvent, side: "in" | "out") {
    e.preventDefault();
    e.stopPropagation();
    const current = side === "in" ? (video?.fade?.in_frames ?? 0) : (video?.fade?.out_frames ?? 0);
    menu = {
      ...menuAt(e),
      title: t(side === "in" ? "video.fade_in" : "video.fade_out"),
      items: FADE_FRAMES.map((f) => ({
        label: fadeLabel(f),
        checked: f === current,
        action: () => player.edit(side === "in" ? { op: "fade", fade_in: f } : { op: "fade", fade_out: f }),
      })),
    };
  }

  function keyMenu(e: MouseEvent, n: AudioName, index: number) {
    e.preventDefault();
    e.stopPropagation();
    selectedKey = { track: n, index };
    menu = {
      ...menuAt(e),
      title: t(AUDIO_LABEL[n]),
      items: [
        { label: t("video.key_delete"), action: () => removeKey(n, index) },
        { label: t("video.keys_clear"), action: () => player.edit({ op: "clearVolumeKeys", track: n }) },
      ],
    };
  }

  function audioMenu(e: MouseEvent, n: AudioName) {
    e.preventDefault();
    e.stopPropagation();
    const rect = (e.currentTarget as HTMLElement).getBoundingClientRect();
    const p = audioPoint(e, rect, n);
    const items: MenuItem[] = [
      { label: t("video.key_add"), action: () => player.edit({ op: "volumeKey", track: n, index: null, t: p.t, volume: p.v }) },
    ];
    if (player.volume[n].keys.length) {
      items.push({ label: t("video.keys_clear"), action: () => player.edit({ op: "clearVolumeKeys", track: n }) });
    }
    menu = { ...menuAt(e), title: t(AUDIO_LABEL[n]), items };
  }

  function closeMenu(e: PointerEvent) {
    if (menu && !(e.target as HTMLElement | null)?.closest(".menu")) menu = null;
  }

  // --- Ansicht: Höhe, einklappen, Spuren zeigen --------------------------------

  /** Gemerkt je Rechner (nicht im Dokument): Höhe, eingeklappt, versteckte Spuren. */
  const VIEW_KEY = "grabmark.timeline";
  type View = { height: number | null; collapsed: boolean; hidden: string[] };

  function loadView(): View {
    try {
      const v = JSON.parse(localStorage.getItem(VIEW_KEY) ?? "null");
      if (v && typeof v === "object") {
        return {
          height: typeof v.height === "number" ? v.height : null,
          collapsed: !!v.collapsed,
          hidden: Array.isArray(v.hidden) ? v.hidden.filter((k: unknown) => typeof k === "string") : [],
        };
      }
    } catch {
      // Kein Speicher (privat, gesperrt): Standard.
    }
    return { height: null, collapsed: false, hidden: [] };
  }

  let view = $state<View>(loadView());
  /** Nur diese Spur zeigen (Klick auf ihren Namen); `null` = alle sichtbaren. */
  let solo = $state<string | null>(null);

  $effect(() => {
    const json = JSON.stringify(view);
    try {
      localStorage.setItem(VIEW_KEY, json);
    } catch {
      // Nicht schlimm: Die Ansicht gilt dann nur bis zum Schliessen.
    }
  });

  const shown = (key: string) => (solo ? solo === key : !view.hidden.includes(key));
  const clicksLane = $derived(player.hasInput && !cameraOnly && (player.clicks.length > 0 || (video?.hidden_clicks?.length ?? 0) > 0));

  /** Alle Spuren, die es in diesem Video gibt – für das Menü „Spuren“. */
  const laneList = $derived.by(() => {
    const out: { key: string; label: string }[] = [{ key: "video", label: t("video.track_video") }];
    for (const n of audioLanes) out.push({ key: n, label: t(AUDIO_LABEL[n]) });
    if (clicksLane) out.push({ key: "clicks", label: t("video.track_clicks") });
    if (video?.webcam && !cameraOnly) out.push({ key: "webcam", label: t("video.webcam") });
    if (!cameraOnly) out.push({ key: "zoom", label: t("video.track_zoom") });
    for (const group of new Set(layerRows.map((r) => r.group))) {
      out.push({ key: `layer:${group}`, label: t(LANE_LABEL[group]) });
    }
    return out;
  });

  function soloLane(e: PointerEvent, key: string) {
    if (e.button !== 0) return;
    e.stopPropagation();
    solo = solo === key ? null : key;
  }

  function toggleLane(key: string) {
    view.hidden = view.hidden.includes(key) ? view.hidden.filter((k) => k !== key) : [...view.hidden, key];
    solo = null;
  }

  function lanesMenu(e: MouseEvent) {
    e.stopPropagation();
    menu = {
      ...menuAt(e),
      title: t("video.lanes"),
      items: [
        ...laneList.map((l) => ({ label: l.label, checked: () => shown(l.key), keep: true, action: () => toggleLane(l.key) })),
        {
          label: t("video.lanes_all"),
          action: () => {
            view.hidden = [];
            solo = null;
          },
        },
      ],
    };
  }

  // Höhe am oberen Rand ziehen; Doppelklick = wieder automatisch.
  let wrap = $state<HTMLDivElement | null>(null);
  let resize: { y0: number; h0: number } | null = null;

  function resizeDown(e: PointerEvent) {
    if (e.button !== 0 || !wrap) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    resize = { y0: e.clientY, h0: view.collapsed ? 0 : wrap.clientHeight };
    view.collapsed = false;
  }

  function resizeMove(e: PointerEvent) {
    if (!resize) return;
    const h = resize.h0 + resize.y0 - e.clientY;
    if (h < 16) {
      view.collapsed = true;
      return;
    }
    view.collapsed = false;
    view.height = Math.round(Math.min(window.innerHeight * 0.75, Math.max(24, h)));
  }

  function resizeUp() {
    resize = null;
  }

  // --- Aktionen ----------------------------------------------------------------

  const sel = $derived(player.selection);

  function setWebcam(mode: WebcamMode) {
    const range = sel ?? { start: player.outputMs, end: player.durationMs };
    player.edit({ op: "webcamMode", start: range.start, end: range.end, mode });
  }

  function cut() {
    const range = sel;
    if (!range) return;
    player.edit({ op: "cut", start: range.start, end: range.end });
    player.selection = null;
    player.seek(range.start);
  }

  function keep() {
    const range = sel;
    if (!range) return;
    player.edit({ op: "trim", start: range.start, end: range.end });
    player.selection = null;
    player.seek(0);
  }

  function addZoom() {
    player.edit({ op: "addZoom", t: player.outputMs, x: 0.5, y: 0.5 });
  }

  function setLayerRange(start: number | null, end: number | null) {
    if (!layer || !layerBlock) return;
    const s = start ?? layerBlock.start;
    const e = end ?? layerBlock.end;
    if (e - s < 100) return;
    player.edit({ op: "layerTime", layer: layer.id, range: [s, e] });
  }

  const zoom = $derived(zoomIndex != null ? (video?.zooms?.[zoomIndex] ?? null) : null);

  /** Ausgewählter Klick in Ausgabe-Zeit, solange es ihn gibt. */
  const clickAt = $derived.by(() => {
    if (clickSource == null) return null;
    const i = player.clickSources.indexOf(clickSource);
    return i < 0 ? null : player.clicks[i];
  });
  const hiddenClicks = $derived(video?.hidden_clicks?.length ?? 0);

  // Eine Ebene gewählt: Der Klick ist nicht mehr gemeint.
  $effect(() => {
    if (editor.selectedIds.length) {
      clickSource = null;
      selectedKey = null;
    }
  });

  function selectClick(e: PointerEvent, i: number) {
    if (e.button !== 0) return;
    e.stopPropagation();
    player.pause();
    // Ebenen abwählen, sonst löscht Entf auch die Ebene.
    editor.selectedIds = [];
    zoomIndex = null;
    clickSource = player.clickSources[i];
    player.seek(player.clicks[i]);
  }

  function deleteClick() {
    if (clickSource == null) return;
    player.edit({ op: "hideClick", source: clickSource });
    clickSource = null;
  }

  function setZoom(patch: Partial<{ x: number; y: number; scale: number }>) {
    if (zoomIndex == null || !zoom) return;
    player.edit({ op: "setZoom", index: zoomIndex, zoom: { ...zoom, ...patch } });
  }

  function onKey(e: KeyboardEvent) {
    const el = e.target as HTMLElement | null;
    if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable)) return;
    if (editor.galleryOpen || editor.editingTextId) return;
    if (menu && e.key === "Escape") {
      menu = null;
      return;
    }
    if (selectedKey && !e.metaKey && !e.ctrlKey && (e.key === "Delete" || e.key === "Backspace")) {
      e.preventDefault();
      removeKey(selectedKey.track, selectedKey.index);
      return;
    }
    if (selectedKey && e.key === "Escape") {
      selectedKey = null;
      return;
    }
    if (clickAt != null && !e.metaKey && !e.ctrlKey && (e.key === "Delete" || e.key === "Backspace")) {
      e.preventDefault();
      deleteClick();
    } else if (clickAt != null && e.key === "Escape") {
      clickSource = null;
    } else if (e.code === "Space") {
      e.preventDefault();
      player.toggle();
    } else if (e.key === "ArrowLeft" && e.altKey) {
      player.seek(player.outputMs - 1000);
    } else if (e.key === "ArrowRight" && e.altKey) {
      player.seek(player.outputMs + 1000);
    }
  }

  const MODE_LABEL: Record<WebcamMode, string> = {
    overlay: "video.webcam_small",
    large: "video.webcam_large",
    hidden: "video.webcam_off",
  };
</script>

<svelte:window onkeydown={onKey} onpointerdown={closeMenu} />

{#snippet laneName(key: string, label: string)}
  <button
    class="name"
    class:solo={solo === key}
    title={t(solo === key ? "video.lane_solo_off" : "video.lane_solo")}
    onpointerdown={(e) => soloLane(e, key)}
  >
    {label}
  </button>
{/snippet}

<section class="timeline" aria-label={t("video.timeline")} bind:this={root}>
  <!-- svelte-ignore a11y_no_static_element_interactions -->
  <div
    class="resize"
    title={t("video.resize_hint")}
    onpointerdown={resizeDown}
    onpointermove={resizeMove}
    onpointerup={resizeUp}
    onpointercancel={resizeUp}
    ondblclick={() => {
      view.height = null;
      view.collapsed = false;
    }}
  ></div>
  <div class="bar">
    <button
      class="icon fold"
      class:folded={view.collapsed}
      title={t(view.collapsed ? "video.lanes_expand" : "video.lanes_collapse")}
      aria-expanded={!view.collapsed}
      onclick={() => (view.collapsed = !view.collapsed)}
    >
      <Icon name="caret" size={14} />
    </button>
    <button class="play" title={t(player.playing ? "video.pause" : "video.play")} onclick={() => player.toggle()}>
      <Icon name={player.playing ? "pause" : "play"} />
    </button>
    <span class="time">{formatTime(player.outputMs, true)} / {formatTime(player.durationMs)}</span>

    <span class="sep"></span>
    <button disabled={!sel} title={t("video.cut_hint")} onclick={cut}>{t("video.cut")}</button>
    <button disabled={!sel} onclick={keep}>{t("video.keep")}</button>
    {#if video?.cuts?.length}
      <button class="link" onclick={() => player.edit({ op: "uncut" })}>{t("video.uncut")}</button>
    {/if}

    {#if video?.webcam && !cameraOnly}
      <span class="sep"></span>
      <span class="label">{t("video.webcam")}</span>
      <button onclick={() => setWebcam("overlay")}>{t("video.webcam_small")}</button>
      <button onclick={() => setWebcam("large")}>{t("video.webcam_large")}</button>
      <button onclick={() => setWebcam("hidden")}>{t("video.webcam_off")}</button>
    {/if}

    {#if !cameraOnly}
      <span class="sep"></span>
      <button title={t("video.zoom_hint")} onclick={addZoom}>{t("video.zoom_add")}</button>
      {#if player.hasInput}
        <button title={t("video.zoom_auto_hint")} onclick={() => player.edit({ op: "autoZoom" })}>{t("video.zoom_auto")}</button>
      {/if}
    {/if}

    <span class="grow"></span>
    <button class="icon" class:on={!!solo || view.hidden.length > 0} title={t("video.lanes")} onclick={lanesMenu}>
      <Icon name="layers" />
    </button>
    <button class="icon" class:on={panel === "audio"} title={t("video.audio")} onclick={() => (panel = panel === "audio" ? null : "audio")}>
      <Icon name="speaker" />
    </button>
    {#if player.hasInput}
      <button class="icon" class:on={panel === "cursor"} title={t("video.cursor")} onclick={() => (panel = panel === "cursor" ? null : "cursor")}>
        <Icon name="select" />
      </button>
    {/if}
    <button onclick={() => player.snapshot()}>{t("video.snapshot")}</button>
    <button onclick={() => player.gif()}>GIF</button>
    <button onclick={() => player.copy()}>{t("video.copy")}</button>
    <button onclick={() => player.saveAs()}>{t("video.save_as")}</button>
  </div>

  {#if panel === "audio" && video}
    <div class="panel">
      <label>
        <span>{t("video.volume_system")}</span>
        <input
          type="range"
          min="0"
          max="1"
          step="0.05"
          value={video.audio?.system ?? 1}
          onchange={(e) => player.update((v) => (v.audio = { ...(v.audio ?? { system: 1, microphone: 1 }), system: Number(e.currentTarget.value) }))}
        />
      </label>
      <label>
        <span>{t("video.volume_mic")}</span>
        <input
          type="range"
          min="0"
          max="1"
          step="0.05"
          value={video.audio?.microphone ?? 1}
          onchange={(e) => player.update((v) => (v.audio = { ...(v.audio ?? { system: 1, microphone: 1 }), microphone: Number(e.currentTarget.value) }))}
        />
      </label>
      <span class="muted">{t("video.audio_lane_hint")}</span>
    </div>
  {:else if panel === "cursor" && video}
    {@const c = video.cursor ?? { visible: true, size: 1.5, highlight_clicks: true, click_color: { r: 229, g: 57, b: 53, a: 255 } }}
    <div class="panel">
      <label class="check">
        <input type="checkbox" checked={c.visible} onchange={(e) => player.update((v) => (v.cursor = { ...c, visible: e.currentTarget.checked }))} />
        <span>{t("video.cursor_show")}</span>
      </label>
      <label>
        <span>{t("video.cursor_size")}</span>
        <input
          type="range"
          min="1"
          max="3"
          step="0.25"
          value={c.size}
          onchange={(e) => player.update((v) => (v.cursor = { ...c, size: Number(e.currentTarget.value) }))}
        />
      </label>
      <label class="check">
        <input
          type="checkbox"
          checked={c.highlight_clicks}
          onchange={(e) => player.update((v) => (v.cursor = { ...c, highlight_clicks: e.currentTarget.checked }))}
        />
        <span>{t("video.cursor_clicks")}</span>
      </label>
      {#if hiddenClicks}
        <button class="link" onclick={() => player.edit({ op: "showClicks" })}>
          {t("video.clicks_restore", { count: String(hiddenClicks) })}
        </button>
      {/if}
    </div>
  {/if}

  <div
    class="lanes-wrap"
    class:collapsed={view.collapsed}
    bind:this={wrap}
    style:max-height={view.height != null ? `${view.height}px` : null}
  >
  <div
    class="lanes"
    bind:this={lanes}
    bind:clientWidth={width}
    role="slider"
    tabindex="-1"
    aria-label={t("video.timeline")}
    aria-valuemin={0}
    aria-valuemax={duration}
    aria-valuenow={player.outputMs}
    onpointerdown={onDown}
    onpointermove={onMove}
    onpointerup={onUp}
    onpointercancel={onUp}
    ondblclick={onDoubleClick}
  >
    {#if shown("video")}
    <div class="lane clip">
      {@render laneName("video", t("video.track_video"))}
      {#each cutMarks as m, i (i)}
        <span class="cutmark" style:left="{x(m)}px" title={t("video.cut_here")}></span>
      {/each}
      {#each ["in", "out"] as const as side (side)}
        {@const len = x(side === "in" ? player.fadeMs[0] : player.fadeMs[1])}
        {#if len > 0}
          <svg class="fade-shape {side}" width={len} height="20" aria-hidden="true">
            <polygon points={side === "in" ? `0,0 ${len},0 0,20` : `0,0 ${len},0 ${len},20`} />
            <line x1={side === "in" ? 0 : len} y1="20" x2={side === "in" ? len : 0} y2="0" />
          </svg>
        {/if}
        <span
          class="fade-zone {side}"
          style:width="{Math.max(12, len)}px"
          title={`${t(side === "in" ? "video.fade_in" : "video.fade_out")}: ${fadeLabel(side === "in" ? (video?.fade?.in_frames ?? 0) : (video?.fade?.out_frames ?? 0))} – ${t("video.fade_hint")}`}
          role="button"
          tabindex="-1"
          oncontextmenu={(e) => fadeMenu(e, side)}
        ></span>
      {/each}
    </div>
    {/if}
    {#each audioLanes.filter((n) => shown(n)) as n (n)}
      <div class="lane audio">
        {@render laneName(n, t(AUDIO_LABEL[n]))}
        <!-- svelte-ignore a11y_no_static_element_interactions -->
        <div
          class="audio-area"
          data-track={n}
          title={t("video.audio_lane_hint")}
          oncontextmenu={(e) => audioMenu(e, n)}
        >
          <svg width={track} height={AUDIO_H} aria-hidden="true">
            {#if waves[n]}<path class="wave" d={waves[n]} />{/if}
            <polyline class="level" points={curvePoints(n)} />
          </svg>
          {#each player.volume[n].keys as k, i (i)}
            {@const pos = keyPos(n, i, k)}
            <button
              class="key"
              class:selected={selectedKey?.track === n && selectedKey.index === i}
              style:left="{x(pos.t)}px"
              style:top="{pos.y}px"
              aria-label={t("video.key_at", { time: formatTime(pos.t, true) })}
              onpointerdown={(e) => keyDown(e, n, i)}
              onpointermove={keyMove}
              onpointerup={keyUp}
              onpointercancel={keyUp}
              oncontextmenu={(e) => keyMenu(e, n, i)}
            ></button>
          {/each}
        </div>
      </div>
    {/each}
    {#if clicksLane && shown("clicks")}
      <div class="lane">
        {@render laneName("clicks", t("video.track_clicks"))}
        {#each player.clicks as c, i (player.clickSources[i])}
          <button
            class="click"
            class:selected={clickSource === player.clickSources[i]}
            style:left="{x(c)}px"
            title={t("video.click_hint")}
            aria-label={t("video.click_at", { time: formatTime(c, true) })}
            onpointerdown={(e) => selectClick(e, i)}
          ></button>
        {/each}
      </div>
    {/if}
    {#if video?.webcam && !cameraOnly && shown("webcam")}
      <div class="lane">
        {@render laneName("webcam", t("video.webcam"))}
        {#each webcamBlocks as b, i (i)}
          {#if b.mode !== "hidden" && b.end > b.start}
            <span class="block cam {b.mode}" style:left="{x(b.start)}px" style:width="{x(b.end) - x(b.start)}px">
              {t(MODE_LABEL[b.mode] as "video.webcam_small")}
            </span>
          {/if}
        {/each}
      </div>
    {/if}
    {#if !cameraOnly && shown("zoom")}
      <div class="lane" title={t("video.zoom_hint")}>
        {@render laneName("zoom", t("video.track_zoom"))}
        {#each zoomBlocks as b, i (i)}
          <button
            class="block zoom"
            class:selected={zoomIndex === i}
            style:left="{x(b.start)}px"
            style:width="{Math.max(4, x(b.end) - x(b.start))}px"
            onpointerdown={(e) => {
              e.stopPropagation();
              zoomIndex = zoomIndex === i ? null : i;
            }}
          >
            {(video?.zooms?.[i]?.scale ?? 2).toFixed(1)}×
          </button>
        {/each}
      </div>
    {/if}
    {#each layerRows.filter((r) => shown(`layer:${r.group}`)) as row, i (i)}
      <div class="lane layers" class:first={row.first}>
        {#if row.first}
          {@render laneName(`layer:${row.group}`, t(LANE_LABEL[row.group]))}
        {/if}
        {#each row.blocks as b (b.layer)}
          {@const r = blockRange(b)}
          <button
            class="block layer"
            class:selected={editor.selectedIds.includes(b.layer)}
            class:always={b.always}
            class:hidden={b.hidden}
            style:left="{x(r.start)}px"
            style:width="{Math.max(6, x(r.end) - x(r.start))}px"
            style:--layer-color={b.color ?? "#fb8c00"}
            title={t(b.always ? "video.layer_block_always" : "video.layer_block_hint")}
            onpointerdown={(e) => blockDown(e, b, "move")}
            onpointermove={blockMove}
            onpointerup={blockUp}
            onpointercancel={blockUp}
          >
            <span class="grip start" aria-hidden="true" onpointerdown={(e) => blockDown(e, b, "start")}></span>
            {#if b.label}<span class="text">{b.label}</span>{/if}
            <span class="grip end" aria-hidden="true" onpointerdown={(e) => blockDown(e, b, "end")}></span>
          </button>
        {/each}
      </div>
    {/each}
    {#if sel}
      <span class="selection" style:left="{GUTTER + x(sel.start)}px" style:width="{x(sel.end) - x(sel.start)}px"></span>
    {/if}
    <span class="playhead" style:left="{GUTTER + x(player.outputMs)}px"></span>
  </div>
  </div>

  {#if zoom}
    <div class="panel">
      <span class="label">{t("video.track_zoom")}</span>
      <label>
        <span>{t("video.zoom_scale")}</span>
        <input type="range" min="1.25" max="4" step="0.25" value={zoom.scale} onchange={(e) => setZoom({ scale: Number(e.currentTarget.value) })} />
      </label>
      <label>
        <span>{t("video.zoom_x")}</span>
        <input type="range" min="0" max="1" step="0.01" value={zoom.x} onchange={(e) => setZoom({ x: Number(e.currentTarget.value) })} />
      </label>
      <label>
        <span>{t("video.zoom_y")}</span>
        <input type="range" min="0" max="1" step="0.01" value={zoom.y} onchange={(e) => setZoom({ y: Number(e.currentTarget.value) })} />
      </label>
      <button
        onclick={() => {
          player.edit({ op: "setZoom", index: zoomIndex!, zoom: null });
          zoomIndex = null;
        }}>{t("video.zoom_delete")}</button
      >
    </div>
  {/if}
  {#if clickAt != null}
    <div class="panel">
      <span class="label">{t("video.click")}</span>
      <span class="muted">{formatTime(clickAt, true)}</span>
      <span class="grow"></span>
      {#if hiddenClicks}
        <button class="link" onclick={() => player.edit({ op: "showClicks" })}>
          {t("video.clicks_restore", { count: String(hiddenClicks) })}
        </button>
      {/if}
      <button onclick={deleteClick}>{t("video.click_delete")}</button>
    </div>
  {/if}
  {#if layer && layerBlock}
    <div class="panel">
      <span class="label">{t("video.layer_time")}</span>
      <span class="muted">{formatTime(layerBlock.start, true)} – {formatTime(layerBlock.end, true)}</span>
      <button onclick={() => setLayerRange(player.outputMs, null)}>{t("video.layer_from_here")}</button>
      <button onclick={() => setLayerRange(null, player.outputMs)}>{t("video.layer_until_here")}</button>
      {#if sel}
        <button onclick={() => setLayerRange(sel.start, sel.end)}>{t("video.layer_selection")}</button>
      {/if}
      <button class="link" onclick={() => player.edit({ op: "layerTime", layer: layer.id, range: null })}>{t("video.layer_always")}</button>
      <span class="grow"></span>
      <button onclick={() => editor.deleteSelected()}>{t("video.layer_delete")}</button>
    </div>
  {/if}
  {#if menu}
    <div class="menu" role="menu" style:left="{menu.x}px" style:top="{menu.y}px">
      <div class="menu-title">{menu.title}</div>
      {#each menu.items as item, i (i)}
        {@const checked = typeof item.checked === "function" ? item.checked() : !!item.checked}
        <button
          role="menuitemradio"
          aria-checked={checked}
          onclick={() => {
            item.action();
            if (!item.keep) menu = null;
          }}
        >
          <span class="check">{checked ? "✓" : ""}</span>{item.label}
        </button>
      {/each}
    </div>
  {/if}
</section>

<style>
  .timeline {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px 12px 10px;
    border-radius: var(--radius-panel);
    background: var(--glass);
    border: 1px solid var(--glass-edge);
    box-shadow: var(--glass-shadow);
    backdrop-filter: blur(24px) saturate(180%);
    -webkit-backdrop-filter: blur(24px) saturate(180%);
    font-size: 12px;
    user-select: none;
  }
  .bar,
  .panel {
    display: flex;
    align-items: center;
    gap: 6px;
    flex-wrap: wrap;
  }
  .panel {
    padding: 4px 0;
  }
  .panel label {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  button {
    font: inherit;
    color: var(--text);
    background: var(--bg);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 3px 8px;
    cursor: pointer;
    white-space: nowrap;
  }
  button:hover:not(:disabled, .on) {
    background: var(--hover);
  }
  button:disabled {
    opacity: 0.45;
    cursor: default;
  }
  button.icon,
  button.play {
    display: grid;
    place-items: center;
    padding: 3px 6px;
  }
  button.on {
    background: var(--active);
    border-color: var(--active);
    color: var(--on-active);
  }
  button.link {
    border: none;
    background: none;
    color: var(--text-muted);
    text-decoration: underline;
  }
  .time {
    font-variant-numeric: tabular-nums;
    min-width: 92px;
  }
  .label,
  .muted {
    color: var(--text-muted);
  }
  .sep {
    width: 1px;
    height: 18px;
    background: var(--border);
    margin: 0 2px;
  }
  .grow {
    flex: 1;
  }
  .lanes {
    position: relative;
    display: flex;
    flex-direction: column;
    gap: 3px;
    cursor: text;
    touch-action: none;
  }
  .lanes {
    --gutter: 92px;
    padding-left: var(--gutter);
  }
  .lane {
    position: relative;
    height: 20px;
    border-radius: 4px;
    background: var(--bg);
  }
  .lane.clip {
    background: color-mix(in srgb, var(--selection) 18%, var(--bg));
  }
  .name {
    position: absolute;
    left: calc(4px - var(--gutter));
    width: calc(var(--gutter) - 10px);
    top: 1px;
    padding: 1px 4px;
    border: none;
    border-radius: 4px;
    background: none;
    color: var(--text-muted);
    font: inherit;
    text-align: left;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    cursor: pointer;
  }
  .name:hover {
    background: var(--hover);
    color: var(--text);
  }
  .name.solo {
    background: var(--active);
    color: var(--on-active);
  }
  .resize {
    position: absolute;
    left: 12px;
    right: 12px;
    top: -4px;
    height: 8px;
    cursor: ns-resize;
    z-index: 5;
  }
  .resize::after {
    content: "";
    position: absolute;
    left: 50%;
    top: 2px;
    width: 36px;
    height: 4px;
    margin-left: -18px;
    border-radius: 2px;
    background: var(--border);
    opacity: 0;
    transition: opacity 0.15s;
  }
  .timeline:hover .resize::after {
    opacity: 1;
  }
  .lanes-wrap {
    max-height: 40vh;
    overflow-y: auto;
    overflow-x: hidden;
    scrollbar-width: thin;
  }
  .lanes-wrap.collapsed {
    display: none;
  }
  .fold :global(svg) {
    transition: transform 0.15s;
  }
  .fold.folded :global(svg) {
    transform: rotate(-90deg);
  }
  .block {
    position: absolute;
    top: 2px;
    bottom: 2px;
    border-radius: 3px;
    font-size: 10px;
    line-height: 16px;
    padding: 0 4px;
    overflow: hidden;
    color: #fff;
    border: none;
  }
  .block.cam.overlay {
    background: #43a047;
  }
  .block.cam.large {
    background: #1e88e5;
  }
  .block.zoom {
    background: #8e24aa;
    cursor: pointer;
    z-index: 3;
  }
  .block.zoom.selected {
    outline: 2px solid var(--text);
  }
  .lane.layers:not(.first) {
    margin-top: -1px;
  }
  .block.layer {
    background: color-mix(in srgb, var(--layer-color) 75%, var(--bg));
    cursor: grab;
    z-index: 3;
    padding: 0 7px;
    display: flex;
    align-items: center;
  }
  .block.layer:active {
    cursor: grabbing;
  }
  .block.layer.always {
    background: color-mix(in srgb, var(--layer-color) 35%, var(--bg));
  }
  .block.layer.hidden {
    opacity: 0.4;
  }
  .block.layer.selected {
    outline: 2px solid var(--text);
    outline-offset: -1px;
  }
  .block.layer .text {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    pointer-events: none;
  }
  .grip {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 6px;
    cursor: ew-resize;
  }
  .grip.start {
    left: 0;
  }
  .grip.end {
    right: 0;
  }
  .block.layer.selected .grip {
    background: rgb(255 255 255 / 0.35);
  }
  .lane.audio {
    height: 36px;
  }
  .audio-area {
    position: absolute;
    inset: 0;
    cursor: crosshair;
  }
  .audio-area svg {
    position: absolute;
    left: 0;
    top: 0;
    pointer-events: none;
  }
  .wave {
    fill: color-mix(in srgb, #00897b 45%, transparent);
  }
  .level {
    fill: none;
    stroke: #ffb300;
    stroke-width: 1.5;
  }
  .key {
    position: absolute;
    width: 14px;
    height: 14px;
    margin: -7px 0 0 -7px;
    padding: 0;
    border: 4px solid transparent;
    border-radius: 50%;
    background: #ffb300 content-box;
    cursor: grab;
    z-index: 3;
  }
  .key:active {
    cursor: grabbing;
  }
  .key.selected {
    box-shadow: 0 0 0 2px var(--text);
  }
  .fade-zone {
    position: absolute;
    top: 0;
    bottom: 0;
    cursor: context-menu;
    z-index: 2;
  }
  .fade-zone.in {
    left: 0;
  }
  .fade-zone.out {
    right: 0;
  }
  .fade-zone:hover {
    background: rgb(0 0 0 / 0.12);
  }
  .fade-shape {
    position: absolute;
    top: 0;
    pointer-events: none;
  }
  .fade-shape.in {
    left: 0;
  }
  .fade-shape.out {
    right: 0;
  }
  .fade-shape polygon {
    fill: rgb(0 0 0 / 0.35);
  }
  .fade-shape line {
    stroke: var(--text);
    stroke-width: 1;
  }
  .menu {
    position: absolute;
    z-index: 20;
    display: flex;
    flex-direction: column;
    min-width: 150px;
    padding: 4px;
    border-radius: 8px;
    background: var(--bg);
    border: 1px solid var(--border);
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.25);
    transform: translateY(-100%);
  }
  .menu-title {
    padding: 2px 8px 4px;
    color: var(--text-muted);
    font-size: 11px;
  }
  .menu button {
    border: none;
    background: none;
    text-align: left;
    padding: 4px 8px;
    border-radius: 4px;
  }
  .menu button:hover {
    background: var(--hover);
  }
  .menu .check {
    display: inline-block;
    width: 16px;
  }
  .cutmark {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 2px;
    margin-left: -1px;
    background: var(--accent);
  }
  /* Punkt mit grösserer Trefferfläche (Rand durchsichtig). */
  .click {
    position: absolute;
    top: 2px;
    width: 16px;
    height: 16px;
    margin-left: -8px;
    padding: 0;
    border: 5px solid transparent;
    border-radius: 50%;
    background: var(--accent) content-box;
    opacity: 0.75;
    cursor: pointer;
    z-index: 3;
  }
  .click:hover,
  .click.selected {
    opacity: 1;
    border-width: 4px;
  }
  .click.selected {
    box-shadow: 0 0 0 2px var(--text);
  }
  .selection {
    position: absolute;
    top: 0;
    bottom: 0;
    background: rgb(30 136 229 / 0.18);
    border-left: 1px solid var(--selection);
    border-right: 1px solid var(--selection);
    pointer-events: none;
  }
  .playhead {
    position: absolute;
    top: -2px;
    bottom: -2px;
    width: 2px;
    margin-left: -1px;
    background: var(--text);
    pointer-events: none;
    z-index: 4;
  }
</style>
