<script lang="ts">
  // Tray-Fenster: öffnet sich beim Klick aufs Symbol in der Menüleiste
  // (Windows: Taskleiste). Tabs Bild, Bildschirm (Video), Webcam (Video nur
  // mit der Kamera), Einstellungen.

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
  import { t, type MessageKey } from "$lib/i18n";

  type Tab = "image" | "screen" | "webcam" | "settings";
  // Schlüssel aus der Zeit als FMShot – bleibt absichtlich gleich.
  const TAB_KEY = "fmshot.tray.tab";

  const ICONS = {
    region: "M4 8V4h4M16 4h4v4M20 16v4h-4M8 20H4v-4",
    window: "M3 5h18v14H3zM3 9h18",
    screen: "M3 4h18v12H3zM8 20h8M12 16v4",
    scroll: "M7 3h10v18H7zM12 7v6M9.5 10.5 12 13l2.5-2.5",
    text: "M5 6V4h14v2M12 4v16M9 20h6",
    gallery: "M3 3h8v8H3zM13 3h8v8h-8zM3 13h8v8H3zM13 13h8v8h-8z",
    // Stift
    editor: "M4 20h4L19 9l-4-4L4 16zM13.5 6.5l4 4",
    folder: "M3 6h6l2 2h10v11H3z",
    // Zahnrad
    settings:
      "M12.22 2h-.44a2 2 0 0 0-2 2v.18a2 2 0 0 1-1 1.73l-.43.25a2 2 0 0 1-2 0l-.15-.08a2 2 0 0 0-2.73.73l-.22.38a2 2 0 0 0 .73 2.73l.15.1a2 2 0 0 1 1 1.72v.51a2 2 0 0 1-1 1.74l-.15.09a2 2 0 0 0-.73 2.73l.22.38a2 2 0 0 0 2.73.73l.15-.08a2 2 0 0 1 2 0l.43.25a2 2 0 0 1 1 1.73V20a2 2 0 0 0 2 2h.44a2 2 0 0 0 2-2v-.18a2 2 0 0 1 1-1.73l.43-.25a2 2 0 0 1 2 0l.15.08a2 2 0 0 0 2.73-.73l.22-.39a2 2 0 0 0-.73-2.73l-.15-.08a2 2 0 0 1-1-1.74v-.5a2 2 0 0 1 1-1.74l.15-.09a2 2 0 0 0 .73-2.73l-.22-.38a2 2 0 0 0-2.73-.73l-.15.08a2 2 0 0 1-2 0l-.43-.25a2 2 0 0 1-1-1.73V4a2 2 0 0 0-2-2zM9 12a3 3 0 1 0 6 0 3 3 0 1 0-6 0",
    quit: "M10 4H5v16h5M14 8l4 4-4 4M18 12H9",
    // Reiter: Foto (Rahmen mit Berg und Sonne), Bildschirm mit Wiedergabe-Dreieck,
    // Webcam (Linse auf Fuss).
    image: "M3 5h18v14H3zM3 16l5-5 4 4 3-3 6 6M14 8.5a1.5 1.5 0 1 0 3 0 1.5 1.5 0 1 0-3 0",
    record: "M3 4h18v12H3zM8 20h8M12 16v4M10.5 7.5v5l4-2.5z",
    webcam: "M6 10a6 6 0 1 0 12 0 6 6 0 1 0-12 0M10 10a2 2 0 1 0 4 0 2 2 0 1 0-4 0M12 16v4M8 20h8",
  };

  const TABS: [Tab, MessageKey, string][] = [
    ["image", "tray.tab_image", ICONS.image],
    ["screen", "tray.tab_screen", ICONS.record],
    ["webcam", "tray.tab_webcam", ICONS.webcam],
    ["settings", "tray.tab_settings", ICONS.settings],
  ];

  const IMAGE_ACTIONS: { mode: CaptureMode; hotkey: string; label: MessageKey; icon: string }[] = [
    { mode: "region", hotkey: "capture_region", label: "capture.region", icon: ICONS.region },
    { mode: "window", hotkey: "capture_window", label: "capture.window", icon: ICONS.window },
    { mode: "screen", hotkey: "capture_screen", label: "capture.screen", icon: ICONS.screen },
    { mode: "scroll", hotkey: "capture_scroll", label: "capture.scroll", icon: ICONS.scroll },
    { mode: "text", hotkey: "capture_text", label: "capture.text", icon: ICONS.text },
  ];

  const TARGETS: { id: VideoTarget; label: MessageKey; icon: string }[] = [
    { id: "region", label: "video.target_region", icon: ICONS.region },
    { id: "window", label: "video.target_window", icon: ICONS.window },
    { id: "screen", label: "video.target_screen", icon: ICONS.screen },
  ];

  // Seitenverhältnis als kleines Rechteck (Breite × Höhe im 24er-Feld).
  const FORMATS: { id: CameraFormat; label: MessageKey; icon: string }[] = [
    { id: "landscape", label: "camera.format_landscape", icon: "M3 7h18v10H3z" },
    { id: "portrait", label: "camera.format_portrait", icon: "M7 3h10v18H7z" },
    { id: "square", label: "camera.format_square", icon: "M5 5h14v14H5z" },
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
      if (v === "image" || v === "screen" || v === "webcam" || v === "settings") return v;
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
  let launchAtLogin = $state(false);

  $effect(() => {
    try {
      localStorage.setItem(TAB_KEY, tab);
    } catch {
      // egal
    }
  });

  async function refresh() {
    [settings, hotkeys, launchAtLogin] = await Promise.all([
      api.getSettings(),
      api.getHotkeys(),
      api.getLaunchAtLogin(),
    ]);
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

  async function toggleLaunchAtLogin(enabled: boolean) {
    try {
      await api.setLaunchAtLogin(enabled);
    } finally {
      launchAtLogin = await api.getLaunchAtLogin();
    }
  }
</script>

<div class="tray">
  <div class="tabs" role="tablist">
    {#each TABS as [id, label, icon] (id)}
      <button role="tab" aria-selected={tab === id} class:on={tab === id} onclick={() => (tab = id)}>
        <Icon d={icon} size={20} />
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
              <Icon d={a.icon} />
              <span class="label">{t(a.label)}</span>
              <kbd>{shortcut(a.hotkey)}</kbd>
            </button>
          </li>
        {/each}
      </ul>
    {:else if tab === "screen" && video}
      <div class="targets" role="radiogroup" aria-label={t("video.target")}>
        {#each TARGETS as target (target.id)}
          <button
            role="radio"
            aria-checked={video.target === target.id}
            class:on={video.target === target.id}
            onclick={() => setVideo({ target: target.id })}
          >
            <Icon d={target.icon} size={22} />
            <span>{t(target.label)}</span>
          </button>
        {/each}
      </div>

      <div class="rows">
        <label class="row">
          <span>{t("video.microphone")}</span>
          <input
            type="checkbox"
            class="switch"
            checked={video.microphone ?? true}
            onchange={(e) => setVideo({ microphone: e.currentTarget.checked })}
          />
        </label>
        {#if video.microphone ?? true}
          <select
            class="mic"
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
        <label class="row">
          <span>{t("video.system_audio")}</span>
          <input
            type="checkbox"
            class="switch"
            checked={video.system_audio ?? true}
            onchange={(e) => setVideo({ system_audio: e.currentTarget.checked })}
          />
        </label>
        <label class="row">
          <span>{t("video.webcam")}</span>
          <input
            type="checkbox"
            class="switch"
            disabled={cameras.length === 0}
            checked={(video.webcam ?? false) && cameras.length > 0}
            onchange={(e) => setVideo({ webcam: e.currentTarget.checked })}
          />
        </label>
        {#if (video.webcam ?? false) && cameras.length > 0}
          <select
            class="mic"
            aria-label={t("video.webcam")}
            value={video.camera_id ?? ""}
            onchange={(e) => setVideo({ camera_id: e.currentTarget.value || null })}
          >
            <option value="">{t("video.camera_default")}</option>
            {#each cameras as c (c.id)}
              <option value={c.id}>{c.name}</option>
            {/each}
          </select>
          <div class="webcam">
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
            <p class="hint left">{t("video.webcam_hint")}</p>
          </div>
        {/if}
        <label class="row">
          <span>{t("video.countdown_setting")}</span>
          <input
            type="checkbox"
            class="switch"
            checked={video.countdown ?? true}
            onchange={(e) => setVideo({ countdown: e.currentTarget.checked })}
          />
        </label>
        <label class="row">
          <span>{t("video.fps")}</span>
          <select value={String(video.fps ?? 30)} onchange={(e) => setVideo({ fps: Number(e.currentTarget.value) })}>
            <option value="30">30</option>
            <option value="60">60</option>
          </select>
        </label>
      </div>

      <button class="record" onclick={() => api.startCapture("video")}>
        <span class="dot"></span>
        {t("video.start")}
        <kbd>{shortcut("capture_video")}</kbd>
      </button>
      <p class="hint">{t("video.stop_hint", { shortcut: shortcut("capture_video") })}</p>
    {:else if tab === "webcam" && video}
      <div class="targets" role="radiogroup" aria-label={t("camera.format")}>
        {#each FORMATS as f (f.id)}
          <button
            role="radio"
            aria-checked={camera.format === f.id}
            class:on={camera.format === f.id}
            onclick={() => setCamera({ format: f.id })}
          >
            <Icon d={f.icon} size={22} />
            <span>{t(f.label)}</span>
          </button>
        {/each}
      </div>

      {#if cameras.length === 0}
        <p class="hint">{t("video.no_camera")}</p>
      {:else}
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
          <label class="row">
            <span>{t("video.webcam_mirror")}</span>
            <input
              type="checkbox"
              class="switch"
              checked={camera.mirror}
              onchange={(e) => setCamera({ mirror: e.currentTarget.checked })}
            />
          </label>
          <label class="row">
            <span>{t("video.microphone")}</span>
            <input
              type="checkbox"
              class="switch"
              checked={video.microphone ?? true}
              onchange={(e) => setVideo({ microphone: e.currentTarget.checked })}
            />
          </label>
          {#if video.microphone ?? true}
            <select
              class="mic"
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
          <label class="row">
            <span>{t("video.countdown_setting")}</span>
            <input
              type="checkbox"
              class="switch"
              checked={video.countdown ?? true}
              onchange={(e) => setVideo({ countdown: e.currentTarget.checked })}
            />
          </label>
        </div>

        <button class="record" onclick={() => api.startCapture("webcam")}>
          <span class="dot"></span>
          {t("camera.start")}
          <kbd>{shortcut("capture_webcam")}</kbd>
        </button>
        <p class="hint">{t("camera.hint")}</p>
      {/if}
    {:else if tab === "settings"}
      <div class="rows">
        <label class="row">
          <span>{t("settings.launch_at_login")}</span>
          <input
            type="checkbox"
            class="switch"
            checked={launchAtLogin}
            onchange={(e) => toggleLaunchAtLogin(e.currentTarget.checked)}
          />
        </label>
      </div>
      <ul class="actions">
        <li>
          <button class="action" onclick={() => api.openMain("settings")}>
            <Icon d={ICONS.settings} />
            <span class="label">{t("tray.settings_open")}</span>
          </button>
        </li>
        <li>
          <button class="action" onclick={() => api.openMain("library")}>
            <Icon d={ICONS.folder} />
            <span class="label">{t("tray.open_library")}</span>
          </button>
        </li>
      </ul>
    {/if}
  </section>

  <footer>
    <button class="link" onclick={() => api.openMain("editor")}>
      <Icon d={ICONS.editor} size={16} />
      {t("tray.open_editor")}
    </button>
    <button class="link" onclick={() => api.openMain("gallery")}>
      <Icon d={ICONS.gallery} size={16} />
      {t("tray.open_gallery")}
      <kbd>{shortcut("open_gallery")}</kbd>
    </button>
    <button class="link quiet" onclick={() => api.quitApp()}>
      <Icon d={ICONS.quit} size={16} />
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
    grid-template-columns: repeat(4, 1fr);
    padding: 3px;
    border-radius: var(--radius);
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
    border-radius: 6px;
    cursor: pointer;
    white-space: nowrap;
  }

  .tabs button.on {
    background: var(--surface);
    color: var(--text);
    font-weight: 600;
    box-shadow: 0 1px 2px rgb(0 0 0 / 0.12);
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
    padding: 9px var(--space-2);
    border-radius: 6px;
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
  }

  .targets {
    display: grid;
    grid-template-columns: repeat(3, 1fr);
    gap: var(--space-2);
  }

  .targets button {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    padding: var(--space-3) var(--space-1);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--text-muted);
    font: inherit;
    font-size: 12px;
    cursor: pointer;
  }

  /* Innen statt aussen verstärkt: Der Rand bleibt innerhalb des Knopfs und
     wird vom scrollenden Bereich nicht abgeschnitten. */
  .targets button.on {
    border-color: var(--accent);
    color: var(--text);
    box-shadow: inset 0 0 0 1px var(--accent);
  }

  .rows {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
  }

  .row {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    min-height: 28px;
  }

  select {
    font: inherit;
    font-size: 13px;
    color: var(--text);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 3px 6px;
  }

  .mic {
    width: 100%;
    margin-top: -2px;
  }

  /* Schalter statt Häkchen – wie in den Systemeinstellungen. */
  .switch {
    appearance: none;
    -webkit-appearance: none;
    flex: none;
    width: 34px;
    height: 20px;
    margin: 0;
    border-radius: 10px;
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
    width: 16px;
    height: 16px;
    border-radius: 50%;
    background: #fff;
    box-shadow: 0 1px 2px rgb(0 0 0 / 0.25);
    transition: transform 0.15s;
  }

  .switch:checked {
    background: var(--accent);
  }

  .switch:checked::after {
    transform: translateX(14px);
  }

  .record {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: var(--space-2);
    margin-top: auto;
    border: 0;
    border-radius: var(--radius);
    background: var(--accent);
    color: #fff;
    font: inherit;
    font-weight: 600;
    padding: 11px;
    cursor: pointer;
  }

  .record:hover {
    filter: brightness(1.08);
  }

  .record kbd {
    color: rgb(255 255 255 / 0.75);
    margin-left: var(--space-2);
  }

  .dot {
    width: 10px;
    height: 10px;
    border-radius: 50%;
    background: #fff;
  }

  .hint {
    margin: 0;
    font-size: 12px;
    color: var(--text-muted);
    text-align: center;
  }

  .hint.left {
    text-align: left;
  }

  /* Einstellungen der Webcam, eingerückt unter dem Schalter. */
  .webcam {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    padding-left: var(--space-3);
    border-left: 2px solid var(--border);
  }

  .switch:disabled {
    opacity: 0.5;
    cursor: default;
  }

  footer {
    display: flex;
    flex-direction: column;
    gap: 2px;
    border-top: 1px solid var(--border);
    padding-top: var(--space-2);
  }

  .link {
    font-size: 13px;
    padding: 7px var(--space-2);
  }

  .quiet {
    color: var(--text-muted);
  }
</style>
