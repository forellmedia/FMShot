<script lang="ts">
  // Tray-Fenster: öffnet sich beim Klick aufs Symbol in der Menüleiste
  // (Windows: Taskleiste). Tabs Bild, Bildschirm (Video), Webcam (Video nur
  // mit der Kamera); die Einstellungen öffnet die Fusszeile. Bei Video und Webcam steht der runde
  // Auslöser gleich neben der Zielwahl, Schalter sind Chips darunter – so
  // passt alles ohne Scrollen ins Fenster.

  import { onMount } from "svelte";
  import { api, on, type Camera, type HotkeyInfo, type Microphone, type Settings } from "$lib/api";
  import type {
    CameraFormat,
    CameraSettings,
    CaptureMode,
    VideoTarget,
    WebcamBackground,
    WebcamFraming,
    WebcamLayout,
    WebcamShape,
  } from "$lib/bindings";
  import Icon from "$lib/editor/Icon.svelte";
  import type { IconName } from "$lib/icons";
  import { DEFAULT_FPS, nextFps } from "$lib/fps";
  import { t, type MessageKey } from "$lib/i18n";

  type Tab = "image" | "screen" | "webcam";
  // Schlüssel aus der Zeit als FMShot – bleibt absichtlich gleich.
  const TAB_KEY = "fmshot.tray.tab";

  const TABS: [Tab, MessageKey, IconName][] = [
    ["image", "tray.tab_image", "image"],
    ["screen", "tray.tab_screen", "video"],
    ["webcam", "tray.tab_webcam", "webcam"],
  ];

  const IMAGE_ACTIONS: { mode: CaptureMode; hotkey: string; label: MessageKey; icon: IconName }[] = [
    { mode: "region", hotkey: "capture_region", label: "capture.region", icon: "region" },
    { mode: "window", hotkey: "capture_window", label: "capture.window", icon: "window" },
    { mode: "screen", hotkey: "capture_screen", label: "capture.screen", icon: "screen" },
    { mode: "scroll", hotkey: "capture_scroll", label: "capture.scroll", icon: "scroll" },
    { mode: "text", hotkey: "capture_text", label: "capture.text", icon: "ocr" },
  ];

  const TARGETS: { id: VideoTarget; label: MessageKey; icon: IconName }[] = [
    { id: "region", label: "video.target_region", icon: "region" },
    { id: "window", label: "video.target_window", icon: "window" },
    { id: "screen", label: "video.target_screen", icon: "screen" },
  ];

  // Seitenverhältnis als kleines Rechteck.
  const FORMATS: { id: CameraFormat; label: MessageKey; icon: IconName }[] = [
    { id: "landscape", label: "camera.format_landscape", icon: "formatLandscape" },
    { id: "portrait", label: "camera.format_portrait", icon: "formatPortrait" },
    { id: "square", label: "camera.format_square", icon: "formatSquare" },
  ];

  const SHAPES: { id: WebcamShape; label: MessageKey }[] = [
    { id: "circle", label: "video.shape_circle" },
    { id: "square", label: "video.shape_square" },
    { id: "landscape", label: "video.shape_landscape" },
    { id: "portrait", label: "video.shape_portrait" },
  ];

  const FRAMINGS: { id: WebcamFraming; label: MessageKey }[] = [
    { id: "full", label: "video.framing_full" },
    { id: "head", label: "video.framing_head" },
  ];

  const BACKGROUNDS: { id: WebcamBackground; label: MessageKey }[] = [
    { id: "original", label: "video.background_original" },
    { id: "blur", label: "video.background_blur" },
    { id: "remove", label: "video.background_remove" },
  ];

  // Breite der Webcam als Anteil der Bildbreite.
  const SIZES: { value: number; label: MessageKey }[] = [
    { value: 0.15, label: "video.size_small" },
    { value: 0.2, label: "video.size_medium" },
    { value: 0.28, label: "video.size_large" },
  ];

  const DEFAULT_BORDER = { width: 0.025, color: { r: 255, g: 255, b: 255, a: 255 } };
  const DEFAULT_RADIUS = 0.12;

  function storedTab(): Tab {
    try {
      const v = localStorage.getItem(TAB_KEY);
      // Früher hiess der Tab „Bildschirm“ „Video“.
      if (v === "video") return "screen";
      if (v === "image" || v === "screen" || v === "webcam") return v;
    } catch {
      // Ohne Speicher: Standard.
    }
    return "image";
  }

  let tab = $state<Tab>(storedTab());
  let settings = $state<Settings | null>(null);
  let hotkeys = $state<HotkeyInfo[]>([]);
  let microphones = $state<Microphone[]>([]);
  let cameras = $state<Camera[]>([]);

  $effect(() => {
    try {
      localStorage.setItem(TAB_KEY, tab);
    } catch {
      // egal
    }
  });

  async function refresh() {
    [settings, hotkeys] = await Promise.all([api.getSettings(), api.getHotkeys()]);
    [microphones, cameras] = await Promise.all([api.listMicrophones(), api.listCameras()]);
  }

  onMount(() => {
    void refresh();
    let stop: (() => void) | undefined;
    // Bei jedem Öffnen frisch: Einstellungen oder Mikrofone können sich geändert haben.
    on("tray-opened", () => void refresh()).then((f) => (stop = f));
    return () => stop?.();
  });

  function shortcut(action: string): string {
    return hotkeys.find((h) => h.action === action)?.label ?? "";
  }

  const video = $derived(settings?.video);

  async function setVideo(patch: Partial<NonNullable<Settings["video"]>>) {
    if (!settings) return;
    settings.video = { ...settings.video, ...patch };
    await api.setSettings($state.snapshot(settings) as Settings);
  }

  const layout = $derived<WebcamLayout>(video?.webcam_layout ?? {});

  // Nächste Stufe zur gespeicherten Grösse (auch wenn sie frei gesetzt wurde).
  const sizeValue = $derived(
    SIZES.reduce((a, b) =>
      Math.abs(b.value - (layout.size ?? 0.2)) < Math.abs(a.value - (layout.size ?? 0.2)) ? b : a,
    ).value,
  );

  async function setLayout(patch: Partial<WebcamLayout>) {
    await setVideo({ webcam_layout: { ...layout, ...patch } });
  }

  const camera = $derived<CameraSettings>(
    video?.camera ?? { format: "landscape", framing: "full", follow: true, background: "original", mirror: true },
  );

  async function setCamera(patch: Partial<CameraSettings>) {
    await setVideo({ camera: { ...camera, ...patch } });
  }
</script>


{#snippet chip(label: string, icon: IconName, on: boolean, toggle: () => void, disabled = false)}
  <button class="chip" class:on aria-pressed={on} {disabled} onclick={toggle}>
    <Icon name={icon} size={15} />
    {label}
  </button>
{/snippet}

{#snippet shutter(mode: CaptureMode, hotkey: string)}
  {@const title = t("tray.shutter", { shortcut: shortcut(hotkey) })}
  <button class="shutter" {title} aria-label={title} onclick={() => api.startCapture(mode)}>
    <span class="disc"></span>
    <kbd>{shortcut(hotkey)}</kbd>
  </button>
{/snippet}

<div class="tray">
  <div class="tabs" role="tablist">
    {#each TABS as [id, label, icon] (id)}
      <button role="tab" aria-selected={tab === id} class:on={tab === id} onclick={() => (tab = id)}>
        <Icon name={icon} size={20} />
        <span>{t(label)}</span>
      </button>
    {/each}
  </div>

  <section>
    {#if tab === "image"}
      <ul class="actions">
        {#each IMAGE_ACTIONS as a (a.mode)}
          <li>
            <button class="action" onclick={() => api.startCapture(a.mode)}>
              <Icon name={a.icon} size={20} />
              <span class="label">{t(a.label)}</span>
              <kbd>{shortcut(a.hotkey)}</kbd>
            </button>
          </li>
        {/each}
      </ul>
    {:else if tab === "screen" && video}
      <div class="start">
        <div class="targets" role="radiogroup" aria-label={t("video.target")}>
          {#each TARGETS as target (target.id)}
            <button
              role="radio"
              aria-checked={video.target === target.id}
              class:on={video.target === target.id}
              onclick={() => setVideo({ target: target.id })}
            >
              <Icon name={target.icon} size={22} />
              <span>{t(target.label)}</span>
            </button>
          {/each}
        </div>
        {@render shutter("video", "capture_video")}
      </div>

      <div class="chips">
        {@render chip(t("video.microphone"), "mic", video.microphone ?? true, () =>
          setVideo({ microphone: !(video.microphone ?? true) }),
        )}
        {@render chip(t("tray.chip_sound"), "speaker", video.system_audio ?? true, () =>
          setVideo({ system_audio: !(video.system_audio ?? true) }),
        )}
        {@render chip(
          t("video.webcam"),
          "webcam",
          (video.webcam ?? false) && cameras.length > 0,
          () => setVideo({ webcam: !(video.webcam ?? false) }),
          cameras.length === 0,
        )}
        {@render chip(t("tray.chip_countdown"), "timer", video.countdown ?? true, () =>
          setVideo({ countdown: !(video.countdown ?? true) }),
        )}
        {@render chip(
          t("tray.chip_fps", { n: String(video.fps ?? DEFAULT_FPS) }),
          "play",
          (video.fps ?? DEFAULT_FPS) !== DEFAULT_FPS,
          () => setVideo({ fps: nextFps(video.fps ?? DEFAULT_FPS) }),
        )}
      </div>

      <div class="rows">
        {#if video.microphone ?? true}
          <select
            aria-label={t("video.microphone")}
            value={video.microphone_id ?? ""}
            onchange={(e) => setVideo({ microphone_id: e.currentTarget.value || null })}
          >
            <option value="">{t("video.microphone_default")}</option>
            {#each microphones as m (m.id)}
              <option value={m.id}>{m.name}</option>
            {/each}
          </select>
        {/if}
        {#if (video.webcam ?? false) && cameras.length > 0}
          <select
            aria-label={t("video.webcam")}
            value={video.camera_id ?? ""}
            onchange={(e) => setVideo({ camera_id: e.currentTarget.value || null })}
          >
            <option value="">{t("video.camera_default")}</option>
            {#each cameras as c (c.id)}
              <option value={c.id}>{c.name}</option>
            {/each}
          </select>
          <details class="webcam">
            <summary>
              {t("tray.webcam_options")}
              <span class="sum">
                {t(SHAPES.find((s) => s.id === (layout.shape ?? "circle"))?.label ?? "video.shape_circle")} ·
                {t(SIZES.find((s) => s.value === sizeValue)?.label ?? "video.size_medium")}
              </span>
            </summary>
            <label class="row">
              <span>{t("video.webcam_shape")}</span>
              <select
                value={layout.shape ?? "circle"}
                onchange={(e) => setLayout({ shape: e.currentTarget.value as WebcamShape })}
              >
                {#each SHAPES as s (s.id)}
                  <option value={s.id}>{t(s.label)}</option>
                {/each}
              </select>
            </label>
            <label class="row">
              <span>{t("video.webcam_size")}</span>
              <select value={String(sizeValue)} onchange={(e) => setLayout({ size: Number(e.currentTarget.value) })}>
                {#each SIZES as s (s.value)}
                  <option value={String(s.value)}>{t(s.label)}</option>
                {/each}
              </select>
            </label>
            <label class="row">
              <span>{t("video.webcam_framing")}</span>
              <select
                value={layout.framing ?? "full"}
                onchange={(e) => setLayout({ framing: e.currentTarget.value as WebcamFraming })}
              >
                {#each FRAMINGS as f (f.id)}
                  <option value={f.id}>{t(f.label)}</option>
                {/each}
              </select>
            </label>
            {#if (layout.framing ?? "full") === "head"}
              <label class="row">
                <span>{t("video.webcam_follow")}</span>
                <input
                  type="checkbox"
                  class="switch"
                  checked={layout.follow ?? true}
                  onchange={(e) => setLayout({ follow: e.currentTarget.checked })}
                />
              </label>
            {/if}
            <label class="row">
              <span>{t("video.webcam_background")}</span>
              <select
                value={layout.background ?? "original"}
                onchange={(e) => setLayout({ background: e.currentTarget.value as WebcamBackground })}
              >
                {#each BACKGROUNDS as b (b.id)}
                  <option value={b.id}>{t(b.label)}</option>
                {/each}
              </select>
            </label>
            {#if (layout.shape ?? "circle") !== "circle"}
              <label class="row">
                <span>{t("video.webcam_rounded")}</span>
                <input
                  type="checkbox"
                  class="switch"
                  checked={(layout.corner_radius ?? DEFAULT_RADIUS) > 0}
                  onchange={(e) => setLayout({ corner_radius: e.currentTarget.checked ? DEFAULT_RADIUS : 0 })}
                />
              </label>
            {/if}
            {#if layout.background !== "remove"}
              <label class="row">
                <span>{t("video.webcam_border")}</span>
                <input
                  type="checkbox"
                  class="switch"
                  checked={layout.border !== null}
                  onchange={(e) => setLayout({ border: e.currentTarget.checked ? DEFAULT_BORDER : null })}
                />
              </label>
              <label class="row">
                <span>{t("video.webcam_shadow")}</span>
                <input
                  type="checkbox"
                  class="switch"
                  checked={layout.shadow ?? true}
                  onchange={(e) => setLayout({ shadow: e.currentTarget.checked })}
                />
              </label>
            {/if}
            <label class="row">
              <span>{t("video.webcam_mirror")}</span>
              <input
                type="checkbox"
                class="switch"
                checked={layout.mirror ?? true}
                onchange={(e) => setLayout({ mirror: e.currentTarget.checked })}
              />
            </label>
            <p class="hint">{t("video.webcam_hint")}</p>
          </details>
        {/if}
      </div>
      <p class="hint">{t("video.stop_hint", { shortcut: shortcut("capture_video") })}</p>
    {:else if tab === "webcam" && video}
      <div class="start">
        <div class="targets" role="radiogroup" aria-label={t("camera.format")}>
          {#each FORMATS as f (f.id)}
            <button
              role="radio"
              aria-checked={camera.format === f.id}
              class:on={camera.format === f.id}
              onclick={() => setCamera({ format: f.id })}
            >
              <Icon name={f.icon} size={22} />
              <span>{t(f.label)}</span>
            </button>
          {/each}
        </div>
        {#if cameras.length > 0}
          {@render shutter("webcam", "capture_webcam")}
        {/if}
      </div>

      {#if cameras.length === 0}
        <p class="hint">{t("video.no_camera")}</p>
      {:else}
        <div class="chips">
          {@render chip(t("video.microphone"), "mic", video.microphone ?? true, () =>
            setVideo({ microphone: !(video.microphone ?? true) }),
          )}
          {@render chip(t("video.webcam_mirror"), "webcam", camera.mirror ?? true, () => setCamera({ mirror: !(camera.mirror ?? true) }))}
          {@render chip(t("tray.chip_countdown"), "timer", video.countdown ?? true, () =>
            setVideo({ countdown: !(video.countdown ?? true) }),
          )}
        </div>
        <div class="rows">
          <label class="row">
            <span>{t("camera.camera")}</span>
            <select
              value={video.camera_id ?? ""}
              onchange={(e) => setVideo({ camera_id: e.currentTarget.value || null })}
            >
              <option value="">{t("video.camera_default")}</option>
              {#each cameras as c (c.id)}
                <option value={c.id}>{c.name}</option>
              {/each}
            </select>
          </label>
          {#if video.microphone ?? true}
            <label class="row">
              <span>{t("video.microphone")}</span>
              <select
                value={video.microphone_id ?? ""}
                onchange={(e) => setVideo({ microphone_id: e.currentTarget.value || null })}
              >
                <option value="">{t("video.microphone_default")}</option>
                {#each microphones as m (m.id)}
                  <option value={m.id}>{m.name}</option>
                {/each}
              </select>
            </label>
          {/if}
          <label class="row">
            <span>{t("video.webcam_framing")}</span>
            <select
              value={camera.framing}
              onchange={(e) => setCamera({ framing: e.currentTarget.value as WebcamFraming })}
            >
              {#each FRAMINGS as f (f.id)}
                <option value={f.id}>{t(f.label)}</option>
              {/each}
            </select>
          </label>
          {#if camera.framing === "head"}
            <label class="row">
              <span>{t("video.webcam_follow")}</span>
              <input
                type="checkbox"
                class="switch"
                checked={camera.follow}
                onchange={(e) => setCamera({ follow: e.currentTarget.checked })}
              />
            </label>
          {/if}
          <label class="row">
            <span>{t("video.webcam_background")}</span>
            <select
              value={camera.background}
              onchange={(e) => setCamera({ background: e.currentTarget.value as WebcamBackground })}
            >
              {#each BACKGROUNDS as b (b.id)}
                <option value={b.id}>{t(b.label)}</option>
              {/each}
            </select>
          </label>
        </div>
        <p class="hint">{t("camera.hint")}</p>
      {/if}
    {/if}
  </section>

  <footer>
    <button class="link" onclick={() => api.openMain("editor")}>
      <Icon name="editor" size={16} />
      {t("tray.open_editor")}
    </button>
    <button class="link" onclick={() => api.openMain("gallery")}>
      <Icon name="gallery" size={16} />
      {t("tray.open_gallery")}
      <kbd>{shortcut("open_gallery")}</kbd>
    </button>
    <button class="link" onclick={() => api.openMain("settings")}>
      <Icon name="settings" size={16} />
      {t("tray.settings")}
    </button>
    <button class="link quiet" onclick={() => api.quitApp()}>
      <Icon name="quit" size={16} />
      {t("tray.quit")}
    </button>
  </footer>
</div>

<style>
  :global(html),
  :global(body) {
    overflow: hidden;
    user-select: none;
    -webkit-user-select: none;
  }

  .tray {
    display: flex;
    flex-direction: column;
    height: 100vh;
    box-sizing: border-box;
    padding: var(--space-3);
    gap: var(--space-3);
  }

  .tabs {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    padding: 4px;
    border-radius: 999px;
    background: var(--hover);
  }

  .tabs button {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 3px;
    border: 0;
    background: none;
    color: var(--text-muted);
    font: inherit;
    font-size: 11px;
    padding: 6px 2px 5px;
    border-radius: 999px;
    cursor: pointer;
    white-space: nowrap;
  }

  .tabs button.on {
    background: var(--glass-strong);
    color: var(--text);
    font-weight: 600;
    box-shadow: 0 2px 8px rgb(30 40 90 / 0.12);
  }

  section {
    flex: 1;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: var(--space-3);
    min-height: 0;
  }

  .actions {
    list-style: none;
    margin: 0;
    padding: 0;
    display: flex;
    flex-direction: column;
    gap: 2px;
  }

  .action,
  .link {
    display: flex;
    align-items: center;
    gap: var(--space-3);
    width: 100%;
    border: 0;
    background: none;
    color: var(--text);
    font: inherit;
    text-align: left;
    padding: 8px var(--space-2);
    border-radius: 10px;
    cursor: pointer;
  }

  .action:hover,
  .link:hover {
    background: var(--hover);
  }

  .label {
    flex: 1;
  }

  kbd {
    font-family: inherit;
    font-size: 12px;
    color: var(--text-muted);
    margin-left: auto;
    font-variant-numeric: tabular-nums;
  }

  /* Zielwahl und Auslöser in einer Zeile. */
  .start {
    display: flex;
    gap: var(--space-2);
    align-items: stretch;
    padding-top: 2px;
  }

  .targets {
    flex: 1;
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: 6px;
  }

  .targets button {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 4px;
    padding: 9px 2px 7px;
    border: 1px solid var(--glass-edge);
    border-radius: 14px;
    background: var(--glass);
    color: var(--text-muted);
    font: inherit;
    font-size: 11px;
    cursor: pointer;
  }

  .targets button:hover {
    color: var(--text);
  }

  /* Innen statt aussen verstärkt: Der Rand bleibt innerhalb des Knopfs und
     wird vom scrollenden Bereich nicht abgeschnitten. */
  .targets button.on {
    background: var(--glass-strong);
    color: var(--active);
    box-shadow: 0 4px 12px rgb(30 40 90 / 0.12);
  }

  /* Runder Auslöser wie an einer Kamera: roter Kern im Ring. */
  .shutter {
    flex: none;
    width: 68px;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: 5px;
    padding: 0;
    border: 0;
    background: none;
    cursor: pointer;
  }

  .disc {
    width: 44px;
    height: 44px;
    box-sizing: border-box;
    border-radius: 50%;
    background: var(--accent);
    border: 4px solid var(--glass-strong);
    box-shadow:
      0 0 0 1px var(--glass-edge),
      0 6px 16px color-mix(in srgb, var(--accent) 40%, transparent);
    transition: transform 0.12s ease;
  }

  .shutter:hover .disc {
    transform: scale(1.06);
  }

  .shutter:active .disc {
    transform: scale(0.94);
  }

  .shutter:focus-visible {
    outline: none;
  }

  .shutter:focus-visible .disc {
    box-shadow:
      0 0 0 2px var(--accent),
      0 0 0 5px color-mix(in srgb, var(--accent) 30%, transparent);
  }

  .shutter kbd {
    margin: 0;
    font-size: 11px;
  }

  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }

  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 4px 10px 4px 8px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: transparent;
    color: var(--text-muted);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }

  .chip:hover:not(:disabled, .on) {
    color: var(--text);
    background: var(--hover);
  }

  .chip.on:hover {
    background: color-mix(in srgb, var(--active) 22%, transparent);
  }

  .chip.on {
    border-color: color-mix(in srgb, var(--active) 35%, transparent);
    background: color-mix(in srgb, var(--active) 14%, transparent);
    color: color-mix(in srgb, var(--active) 75%, var(--text));
  }

  .chip:disabled {
    opacity: 0.45;
    cursor: default;
  }

  .rows {
    display: flex;
    flex-direction: column;
    gap: 6px;
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    min-height: 26px;
    font-size: 13px;
  }

  select {
    font: inherit;
    font-size: 13px;
    color: var(--text);
    background: var(--glass-strong);
    border: 1px solid var(--glass-edge);
    border-radius: 10px;
    padding: 5px 8px;
  }

  .rows > select {
    width: 100%;
  }

  /* Webcam-Darstellung: zugeklappt nur eine Zeile mit dem Wichtigsten. */
  .webcam {
    border: 1px solid var(--glass-edge);
    background: var(--glass);
    border-radius: 12px;
    padding: 0 var(--space-2);
  }

  .webcam[open] {
    padding-bottom: var(--space-2);
  }

  .webcam summary {
    display: flex;
    align-items: center;
    gap: var(--space-2);
    padding: 7px 0;
    font-size: 13px;
    cursor: pointer;
    list-style: none;
  }

  .webcam summary::-webkit-details-marker {
    display: none;
  }

  .webcam summary::after {
    content: "";
    width: 6px;
    height: 6px;
    margin-left: 2px;
    border-right: 1.5px solid var(--text-muted);
    border-bottom: 1.5px solid var(--text-muted);
    transform: rotate(-45deg);
    transition: transform 0.12s ease;
  }

  .webcam[open] summary::after {
    transform: rotate(45deg);
  }

  .sum {
    margin-left: auto;
    color: var(--text-muted);
    font-size: 12px;
  }

  .webcam .row + .row {
    margin-top: 4px;
  }

  /* Schalter statt Häkchen – wie in den Systemeinstellungen. */
  .switch {
    appearance: none;
    -webkit-appearance: none;
    flex: none;
    width: 30px;
    height: 18px;
    margin: 0;
    border-radius: 9px;
    background: var(--border);
    position: relative;
    cursor: pointer;
    transition: background 0.15s;
  }

  .switch::after {
    content: "";
    position: absolute;
    top: 2px;
    left: 2px;
    width: 14px;
    height: 14px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 2px rgb(0 0 0 / 0.25);
    transition: transform 0.15s;
  }

  .switch:checked {
    background: var(--switch-on);
  }

  .switch:checked::after {
    transform: translateX(12px);
  }

  .hint {
    margin: 0;
    font-size: 12px;
    color: var(--text-muted);
  }

  footer {
    display: flex;
    flex-direction: column;
    gap: 1px;
    border-top: 1px solid var(--hover);
    padding-top: var(--space-2);
  }

  .link {
    font-size: 13px;
    padding: 6px var(--space-2);
  }

  .quiet {
    color: var(--text-muted);
  }

  @media (prefers-reduced-motion: reduce) {
    .disc,
    .webcam summary::after {
      transition: none;
    }
  }
</style>
