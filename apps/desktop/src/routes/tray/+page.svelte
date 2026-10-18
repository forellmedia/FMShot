<script lang="ts">
  // Tray-Fenster: öffnet sich beim Klick aufs Symbol in der Menüleiste
  // (Windows: Taskleiste). Tabs Bild, Video, Einstellungen.

  import { onMount } from "svelte";
  import { api, on, type HotkeyInfo, type Microphone, type Settings } from "$lib/api";
  import type { CaptureMode, VideoTarget } from "$lib/bindings";
  import Icon from "$lib/editor/Icon.svelte";
  import { t, type MessageKey } from "$lib/i18n";

  type Tab = "image" | "video" | "settings";
  const TAB_KEY = "fmshot.tray.tab";

  const ICONS = {
    region: "M4 8V4h4M16 4h4v4M20 16v4h-4M8 20H4v-4",
    window: "M3 5h18v14H3zM3 9h18",
    screen: "M3 4h18v12H3zM8 20h8M12 16v4",
    scroll: "M7 3h10v18H7zM12 7v6M9.5 10.5 12 13l2.5-2.5",
    text: "M5 6V4h14v2M12 4v16M9 20h6",
    gallery: "M3 3h8v8H3zM13 3h8v8h-8zM3 13h8v8H3zM13 13h8v8h-8z",
    folder: "M3 6h6l2 2h10v11H3z",
    settings: "M12 15a3 3 0 1 0 0-6 3 3 0 0 0 0 6zM19 12h2M3 12h2M12 3v2M12 19v2M17 7l1.5-1.5M5.5 18.5 7 17M17 17l1.5 1.5M5.5 5.5 7 7",
    quit: "M10 4H5v16h5M14 8l4 4-4 4M18 12H9",
  };

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

  function storedTab(): Tab {
    try {
      const v = localStorage.getItem(TAB_KEY);
      if (v === "image" || v === "video" || v === "settings") return v;
    } catch {
      // Ohne Speicher: Standard.
    }
    return "image";
  }

  let tab = $state<Tab>(storedTab());
  let settings = $state<Settings | null>(null);
  let hotkeys = $state<HotkeyInfo[]>([]);
  let microphones = $state<Microphone[]>([]);
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
    microphones = await api.listMicrophones();
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
    {#each [["image", "tray.tab_image"], ["video", "tray.tab_video"], ["settings", "tray.tab_settings"]] as [id, label] (id)}
      <button role="tab" aria-selected={tab === id} class:on={tab === id} onclick={() => (tab = id as Tab)}>
        {t(label as MessageKey)}
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
    {:else if tab === "video" && video}
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
    grid-template-columns: repeat(3, 1fr);
    padding: 3px;
    border-radius: var(--radius);
    background: var(--hover);
  }

  .tabs button {
    border: 0;
    background: none;
    color: var(--text-muted);
    font: inherit;
    font-size: 13px;
    padding: 6px 0;
    border-radius: 6px;
    cursor: pointer;
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

  .targets button.on {
    border-color: var(--accent);
    color: var(--text);
    box-shadow: 0 0 0 1px var(--accent);
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
