<script lang="ts">
  // Zeitleiste des Video-Editors (ADR 0011, Etappe 4–6): Abspielen, Bereich
  // wählen, Schneiden, Webcam je Abschnitt, Zoom, Klicks, Zeit von Ebenen,
  // Ton und Mauszeiger. Gerechnet wird in `fmshot-video` (WASM).

  import type { WebcamMode } from "$lib/bindings";
  import { t } from "$lib/i18n";
  import Icon from "./Icon.svelte";
  import { editor } from "./state.svelte";
  import { formatTime, player } from "./video.svelte";
  import { isBaseLayer, layerTime } from "./videoLayers";

  let lanes = $state<HTMLDivElement | null>(null);
  let width = $state(600);
  let panel = $state<"audio" | "cursor" | null>(null);
  /** Ausgewählter Zoom-Abschnitt (Index in `video.zooms`). */
  let zoomIndex = $state<number | null>(null);
  let drag: { start: number; moved: boolean } | null = null;

  const video = $derived(editor.doc?.video ?? null);
  const duration = $derived(Math.max(1, player.durationMs));
  const x = (ms: number) => (ms / duration) * width;

  function timeAt(e: PointerEvent): number {
    const r = lanes!.getBoundingClientRect();
    return Math.max(0, Math.min(duration, ((e.clientX - r.left) / r.width) * duration));
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

  const layer = $derived(editor.selected && !isBaseLayer(editor.selected) ? editor.selected : null);
  const layerBlock = $derived.by((): Block | null => {
    if (!layer || !editor.doc) return null;
    void player.durationMs;
    const tm = layerTime(editor.doc, layer.id);
    return tm ? { start: player.outputTime(tm.start_ms), end: player.outputTime(tm.end_ms) } : { start: 0, end: player.durationMs };
  });

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

  function setZoom(patch: Partial<{ x: number; y: number; scale: number }>) {
    if (zoomIndex == null || !zoom) return;
    player.edit({ op: "setZoom", index: zoomIndex, zoom: { ...zoom, ...patch } });
  }

  function onKey(e: KeyboardEvent) {
    const el = e.target as HTMLElement | null;
    if (el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable)) return;
    if (editor.settingsOpen || editor.galleryOpen || editor.editingTextId) return;
    if (e.code === "Space") {
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

<svelte:window onkeydown={onKey} />

<section class="timeline" aria-label={t("video.timeline")}>
  <div class="bar">
    <button class="play" title={t(player.playing ? "video.pause" : "video.play")} onclick={() => player.toggle()}>
      <Icon d={player.playing ? "M8 5v14M16 5v14" : "M7 4l13 8-13 8z"} />
    </button>
    <span class="time">{formatTime(player.outputMs, true)} / {formatTime(player.durationMs)}</span>

    <span class="sep"></span>
    <button disabled={!sel} title={t("video.cut_hint")} onclick={cut}>{t("video.cut")}</button>
    <button disabled={!sel} onclick={keep}>{t("video.keep")}</button>
    {#if video?.cuts?.length}
      <button class="link" onclick={() => player.edit({ op: "uncut" })}>{t("video.uncut")}</button>
    {/if}

    {#if video?.webcam}
      <span class="sep"></span>
      <span class="label">{t("video.webcam")}</span>
      <button onclick={() => setWebcam("overlay")}>{t("video.webcam_small")}</button>
      <button onclick={() => setWebcam("large")}>{t("video.webcam_large")}</button>
      <button onclick={() => setWebcam("hidden")}>{t("video.webcam_off")}</button>
    {/if}

    <span class="sep"></span>
    <button onclick={addZoom}>{t("video.zoom_add")}</button>
    {#if player.hasInput}
      <button title={t("video.zoom_auto_hint")} onclick={() => player.edit({ op: "autoZoom" })}>{t("video.zoom_auto")}</button>
    {/if}

    <span class="grow"></span>
    {#if player.exporting != null}
      <span class="status">{t("video.updating", { percent: String(Math.round(player.exporting * 100)) })}</span>
    {:else if player.dirty}
      <span class="status">{t("video.pending")}</span>
    {/if}
    <button class="icon" class:on={panel === "audio"} title={t("video.audio")} onclick={() => (panel = panel === "audio" ? null : "audio")}>
      <Icon d="M4 9v6h4l5 4V5L8 9zM16 9a4 4 0 010 6M18.5 6.5a8 8 0 010 11" />
    </button>
    {#if player.hasInput}
      <button class="icon" class:on={panel === "cursor"} title={t("video.cursor")} onclick={() => (panel = panel === "cursor" ? null : "cursor")}>
        <Icon d="M5 3l13 8-6 1.5L9 19z" />
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
    </div>
  {/if}

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
  >
    <div class="lane clip">
      <span class="name">{t("video.track_video")}</span>
      {#each cutMarks as m, i (i)}
        <span class="cutmark" style:left="{x(m)}px" title={t("video.cut_here")}></span>
      {/each}
      {#each player.clicks as c, i (i)}
        <span class="click" style:left="{x(c)}px"></span>
      {/each}
    </div>
    {#if video?.webcam}
      <div class="lane">
        <span class="name">{t("video.webcam")}</span>
        {#each webcamBlocks as b, i (i)}
          {#if b.mode !== "hidden" && b.end > b.start}
            <span class="block cam {b.mode}" style:left="{x(b.start)}px" style:width="{x(b.end) - x(b.start)}px">
              {t(MODE_LABEL[b.mode] as "video.webcam_small")}
            </span>
          {/if}
        {/each}
      </div>
    {/if}
    <div class="lane">
      <span class="name">{t("video.track_zoom")}</span>
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
    {#if layerBlock}
      <div class="lane">
        <span class="name">{t("video.track_layer")}</span>
        <span class="block layer" style:left="{x(layerBlock.start)}px" style:width="{Math.max(4, x(layerBlock.end) - x(layerBlock.start))}px"></span>
      </div>
    {/if}
    {#if sel}
      <span class="selection" style:left="{x(sel.start)}px" style:width="{x(sel.end) - x(sel.start)}px"></span>
    {/if}
    <span class="playhead" style:left="{x(player.outputMs)}px"></span>
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
    </div>
  {/if}
</section>

<style>
  .timeline {
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 8px 12px 10px;
    background: var(--surface);
    border-top: 1px solid var(--border);
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
  button:hover:not(:disabled) {
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
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
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
  .muted,
  .status {
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
  .lane {
    position: relative;
    height: 20px;
    border-radius: 4px;
    background: var(--bg);
    overflow: hidden;
  }
  .lane.clip {
    background: color-mix(in srgb, var(--selection) 18%, var(--bg));
  }
  .name {
    position: absolute;
    left: 6px;
    top: 3px;
    color: var(--text-muted);
    pointer-events: none;
    z-index: 2;
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
  .block.layer {
    background: #fb8c00;
  }
  .cutmark {
    position: absolute;
    top: 0;
    bottom: 0;
    width: 2px;
    margin-left: -1px;
    background: var(--accent);
  }
  .click {
    position: absolute;
    top: 7px;
    width: 6px;
    height: 6px;
    margin-left: -3px;
    border-radius: 50%;
    background: var(--accent);
    opacity: 0.7;
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
