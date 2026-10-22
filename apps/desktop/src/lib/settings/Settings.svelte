<script lang="ts">
  // Einstellungen: Allgemein, Tastenkürzel, Speichern, Video, Info. Eigenes
  // Fenster (Seite `/settings`), unabhängig von Editor und Galerie.

  import { onMount } from "svelte";
  import { api, on, type ContentSearchStatus, type Diagnostics, type HotkeyInfo, type Settings } from "$lib/api";
  import type { DrawMode, VideoCodec, VideoEncoding, VideoQuality, VideoResolution } from "$lib/bindings";
  import { DEFAULT_FPS, FPS_CHOICES } from "$lib/fps";
  import { t, type MessageKey } from "$lib/i18n";
  import { currentPlatform, formatShortcut } from "$lib/shortcuts";

  const isMac = currentPlatform() === "mac";

  let settings = $state<Settings | null>(null);
  let hotkeys = $state<HotkeyInfo[]>([]);
  let diagnostics = $state<Diagnostics | null>(null);
  let contentStatus = $state<ContentSearchStatus | null>(null);
  /** macOS-Kürzel ⇧⌘3/4/5 noch eingeschaltet? `null` = unbekannt. */
  let systemShortcuts = $state<boolean | null>(null);
  let launchAtLogin = $state(false);
  let version = $state("");
  let tab = $state<"general" | "hotkeys" | "saving" | "video" | "about">("general");
  let recording = $state<string | null>(null);
  /** Fehlermeldung (z. B. Speichern fehlgeschlagen). */
  let error = $state<string | null>(null);

  onMount(async () => {
    settings = await api.getSettings();
    hotkeys = await api.getHotkeys();
    diagnostics = await api.diagnostics();
    version = (await api.appInfo()).version;
    contentStatus = await api.contentSearchStatus().catch(() => null);
    systemShortcuts = await api.systemShortcutsActive().catch(() => null);
    launchAtLogin = await api.getLaunchAtLogin().catch(() => false);
  });

  async function toggleLaunchAtLogin(enabled: boolean) {
    try {
      await api.setLaunchAtLogin(enabled);
    } catch (e) {
      error = String(e);
    }
    launchAtLogin = await api.getLaunchAtLogin().catch(() => false);
  }

  // Fortschritt der Bildinhalt-Erkennung live nachführen.
  $effect(() => {
    let stop: (() => void) | undefined;
    on<ContentSearchStatus>("content-index-progress", (s) => (contentStatus = s)).then((f) => (stop = f));
    return () => stop?.();
  });

  // Zurück aus den Systemeinstellungen: Stand neu lesen.
  async function refreshSystemShortcuts() {
    if (isMac) systemShortcuts = await api.systemShortcutsActive().catch(() => null);
  }

  async function save() {
    if (!settings) return;
    try {
      await api.setSettings($state.snapshot(settings) as Settings);
      hotkeys = await api.getHotkeys();
      error = null;
    } catch (e) {
      error = String(e);
    }
  }

  // --- Tastenkürzel aufnehmen ---------------------------------------------------

  async function startRecording(action: string) {
    recording = action;
    await api.pauseHotkeys(true);
  }

  async function stopRecording() {
    recording = null;
    await api.pauseHotkeys(false);
  }

  function shortcutFromEvent(e: KeyboardEvent): string | null {
    const parts: string[] = [];
    if (isMac ? e.metaKey : e.ctrlKey) parts.push("Primary");
    if (isMac ? e.ctrlKey : e.metaKey) parts.push("Secondary");
    if (e.altKey) parts.push("Alt");
    if (e.shiftKey) parts.push("Shift");
    let key: string | null = null;
    if (/^Digit\d$/.test(e.code)) key = e.code.slice(5);
    else if (/^Key[A-Z]$/.test(e.code)) key = e.code.slice(3);
    else if (e.code === "PrintScreen") key = "PrintScreen";
    if (!key) return null;
    // Ohne Modifikator nur die Druck-Taste – sonst wäre Tippen unmöglich.
    if (!parts.length && key !== "PrintScreen") return null;
    return [...parts, key].join("+");
  }

  async function onRecordKey(e: KeyboardEvent, action: string) {
    e.preventDefault();
    e.stopPropagation();
    if (!settings) return;
    if (e.key === "Escape") {
      await stopRecording();
      return;
    }
    if (e.key === "Backspace" || e.key === "Delete") {
      settings.hotkeys = { ...settings.hotkeys, [action]: "" };
    } else {
      const sc = shortcutFromEvent(e);
      if (!sc) return;
      settings.hotkeys = { ...settings.hotkeys, [action]: sc };
    }
    recording = null;
    await save();
    await api.pauseHotkeys(false);
  }

  async function resetHotkey(action: string) {
    if (!settings) return;
    const copy = { ...settings.hotkeys };
    delete copy[action];
    settings.hotkeys = copy;
    await save();
  }

  async function chooseFolder() {
    if (!settings) return;
    const folder = await api.chooseFolder();
    if (folder) {
      settings.auto_export = { ...settings.auto_export, folder };
      await save();
    }
  }

  const tabs: { id: typeof tab; label: MessageKey }[] = [
    { id: "general", label: "settings.general" },
    { id: "hotkeys", label: "settings.hotkeys" },
    { id: "saving", label: "settings.saving" },
    { id: "video", label: "settings.video" },
    { id: "about", label: "settings.about" },
  ];

  const RESOLUTIONS: { id: VideoResolution; label: MessageKey }[] = [
    { id: "original", label: "settings.resolution_original" },
    { id: "p2160", label: "settings.resolution_2160" },
    { id: "p1440", label: "settings.resolution_1440" },
    { id: "p1080", label: "settings.resolution_1080" },
    { id: "p720", label: "settings.resolution_720" },
  ];
  const CODECS: { id: VideoCodec; label: MessageKey }[] = [
    { id: "h264", label: "settings.codec_h264" },
    { id: "hevc", label: "settings.codec_hevc" },
  ];
  const QUALITIES: { id: VideoQuality; label: MessageKey }[] = [
    { id: "small", label: "settings.quality_small" },
    { id: "standard", label: "settings.quality_standard" },
    { id: "high", label: "settings.quality_high" },
  ];

  const encoding = $derived<VideoEncoding>(
    settings?.video?.encoding ?? { resolution: "original", codec: "h264", quality: "standard" },
  );

  async function setEncoding(patch: Partial<VideoEncoding>) {
    if (!settings) return;
    settings.video = { ...settings.video, encoding: { ...encoding, ...patch } };
    await save();
  }
</script>

<svelte:window onfocus={refreshSystemShortcuts} />

<div class="page">
  <!-- Mit Glas (macOS) liegt die Titelleiste über dem Inhalt: Titel selbst zeigen. -->
  <div class="titlebar" data-tauri-drag-region>{t("settings.title")}</div>
  <div class="content">
    <nav>
      {#each tabs as tb (tb.id)}
        <button class:on={tab === tb.id} onclick={() => (tab = tb.id)}>{t(tb.label)}</button>
      {/each}
    </nav>

    {#if settings}
      <section>
        {#if tab === "general"}
          <label class="row">
            <span>{t("settings.language")}</span>
            <select bind:value={settings.language} onchange={save}>
              <option value={null}>{t("settings.language_system")}</option>
              <option value="de">Deutsch</option>
              <option value="en">English</option>
            </select>
          </label>
          <label class="row">
            <span>{t("settings.after_capture")}</span>
            <select bind:value={settings.after_capture} onchange={save}>
              <option value="preview">{t("settings.after_preview")}</option>
              <option value="editor">{t("settings.after_editor")}</option>
              <option value="clipboard">{t("settings.after_clipboard")}</option>
            </select>
          </label>
          {#if settings.after_capture === "preview"}
            <label class="row">
              <span>{t("settings.preview_position")}</span>
              <select bind:value={settings.preview_position} onchange={save}>
                <option value="mouse">{t("settings.preview_mouse")}</option>
                <option value="corner">{t("settings.preview_corner")}</option>
              </select>
            </label>
          {/if}
          <label class="row check">
            <input type="checkbox" checked={launchAtLogin} onchange={(e) => toggleLaunchAtLogin(e.currentTarget.checked)} />
            <span>{t("settings.launch_at_login")}</span>
          </label>
          <label class="row check">
            <input type="checkbox" bind:checked={settings.copy_to_clipboard} onchange={save} />
            <span>{t("settings.copy_to_clipboard")}</span>
          </label>
          <label class="row">
            <span>{t("settings.scroll_mode")}</span>
            <select bind:value={settings.scroll_mode} onchange={save}>
              <option value="manual">{t("settings.scroll_manual")}</option>
              <option value="auto">{t("settings.scroll_auto")}</option>
            </select>
          </label>
        {:else if tab === "hotkeys"}
          <p class="hint">{t("settings.hotkeys_hint", { backspace: formatShortcut("Backspace") })}</p>
          {#each hotkeys as hk (hk.action)}
            <div class="row">
              <span>{t(`hotkey.${hk.action}` as MessageKey)}</span>
              <div class="hk">
                <button
                  class="key"
                  class:conflict={hk.conflict}
                  class:rec={recording === hk.action}
                  onclick={() => startRecording(hk.action)}
                  onkeydown={(e) => recording === hk.action && onRecordKey(e, hk.action)}
                  onblur={() => recording === hk.action && stopRecording()}
                >
                  {recording === hk.action ? t("settings.press_keys") : hk.label || t("settings.none")}
                </button>
                <button class="link" onclick={() => resetHotkey(hk.action)}>{t("settings.reset")}</button>
              </div>
            </div>
          {/each}
          {#if hotkeys.some((h) => h.conflict)}
            <p class="warn">{t("settings.conflict")}</p>
          {/if}
          {#if isMac}
            <label class="row check">
              <input type="checkbox" bind:checked={settings.replace_mac_system_shortcuts} onchange={save} />
              <span>{t("settings.replace_system")}</span>
            </label>
            {#if settings.replace_mac_system_shortcuts}
              {#if systemShortcuts === false}
                <p class="hint ok">✓ {t("settings.replace_system_done")}</p>
              {:else}
                <p class="hint">{t("settings.replace_system_hint")}</p>
                <div class="buttons">
                  <button onclick={() => api.openShortcutSettings()}>{t("settings.replace_system_open")}</button>
                </div>
              {/if}
            {/if}
          {/if}
        {:else if tab === "saving"}
          <label class="row">
            <span>{t("settings.save_format", { shortcut: formatShortcut("Primary+S") })}</span>
            <select
              value={settings.save_format ?? "png"}
              onchange={(e) => {
                settings!.save_format = e.currentTarget.value as "png" | "jpg";
                save();
              }}
            >
              <option value="png">PNG</option>
              <option value="jpg">JPEG</option>
            </select>
          </label>
          <label class="row check">
            <input
              type="checkbox"
              checked={settings.auto_export?.enabled}
              onchange={(e) => {
                settings!.auto_export = { ...settings!.auto_export, enabled: e.currentTarget.checked };
                save();
              }}
            />
            <span>{t("settings.auto_export")}</span>
          </label>
          {#if settings.auto_export?.enabled}
            <div class="row">
              <span>{t("settings.folder")}</span>
              <div class="hk">
                <code>{settings.auto_export.folder ?? t("settings.desktop")}</code>
                <button class="link" onclick={chooseFolder}>{t("settings.choose")}</button>
              </div>
            </div>
            <label class="row">
              <span>{t("settings.pattern")}</span>
              <input
                type="text"
                value={settings.auto_export.pattern}
                onchange={(e) => {
                  settings!.auto_export = { ...settings!.auto_export, pattern: e.currentTarget.value };
                  save();
                }}
              />
            </label>
            <p class="hint">{t("settings.pattern_hint")}</p>
            <label class="row">
              <span>{t("settings.format")}</span>
              <select
                value={settings.auto_export.format}
                onchange={(e) => {
                  settings!.auto_export = { ...settings!.auto_export, format: e.currentTarget.value as "png" | "jpg" };
                  save();
                }}
              >
                <option value="png">PNG</option>
                <option value="jpg">JPG</option>
              </select>
            </label>
          {/if}
          <label class="row">
            <span>{t("settings.retention")}</span>
            <select
              value={settings.retention_days ?? 0}
              onchange={(e) => {
                const v = +e.currentTarget.value;
                settings!.retention_days = v ? v : null;
                save();
              }}
            >
              <option value={0}>{t("settings.retention_never")}</option>
              <option value={7}>{t("settings.days", { n: "7" })}</option>
              <option value={30}>{t("settings.days", { n: "30" })}</option>
              <option value={90}>{t("settings.days", { n: "90" })}</option>
              <option value={365}>{t("settings.days", { n: "365" })}</option>
            </select>
          </label>
          <p class="hint">{t("settings.retention_hint")}</p>
          <label class="row check">
            <input
              type="checkbox"
              bind:checked={settings.content_search}
              disabled={contentStatus?.available === false}
              onchange={save}
            />
            <span>{t("settings.content_search")}</span>
          </label>
          <p class="hint">
            {#if contentStatus?.available === false}
              {t("settings.content_search_missing")}
            {:else}
              {t("settings.content_search_hint")}
              {#if contentStatus && settings.content_search}
                {contentStatus.done >= contentStatus.total
                  ? t("settings.content_search_done", { count: String(contentStatus.total) })
                  : t("settings.content_search_progress", {
                      done: String(contentStatus.done),
                      total: String(contentStatus.total),
                    })}
              {/if}
            {/if}
          </p>
        {:else if tab === "video"}
          <label class="row">
            <span>{t("settings.video_resolution")}</span>
            <select
              value={encoding.resolution}
              onchange={(e) => setEncoding({ resolution: e.currentTarget.value as VideoResolution })}
            >
              {#each RESOLUTIONS as r (r.id)}
                <option value={r.id}>{t(r.label)}</option>
              {/each}
            </select>
          </label>
          <label class="row">
            <span>{t("settings.video_codec")}</span>
            <select value={encoding.codec} onchange={(e) => setEncoding({ codec: e.currentTarget.value as VideoCodec })}>
              {#each CODECS as c (c.id)}
                <option value={c.id}>{t(c.label)}</option>
              {/each}
            </select>
          </label>
          {#if encoding.codec === "hevc"}
            <p class="hint">{t("settings.codec_hint")}</p>
          {/if}
          <label class="row">
            <span>{t("settings.video_quality")}</span>
            <select
              value={encoding.quality}
              onchange={(e) => setEncoding({ quality: e.currentTarget.value as VideoQuality })}
            >
              {#each QUALITIES as q (q.id)}
                <option value={q.id}>{t(q.label)}</option>
              {/each}
            </select>
          </label>
          <label class="row">
            <span>{t("video.fps")}</span>
            <select
              value={String(settings.video?.fps ?? DEFAULT_FPS)}
              onchange={(e) => {
                settings!.video = { ...settings!.video, fps: Number(e.currentTarget.value) };
                save();
              }}
            >
              {#each FPS_CHOICES as fps (fps)}
                <option value={String(fps)}>{fps}</option>
              {/each}
            </select>
          </label>
          <p class="hint">{t("settings.video_hint")}</p>

          <h2>{t("settings.draw_title")}</h2>
          <label class="row">
            <span>{t("settings.draw_mode")}</span>
            <select
              value={settings.video?.draw_mode ?? "sticky"}
              onchange={(e) => {
                settings!.video = { ...settings!.video, draw_mode: e.currentTarget.value as DrawMode };
                save();
              }}
            >
              <option value="sticky">{t("settings.draw_mode_sticky")}</option>
              <option value="once">{t("settings.draw_mode_once")}</option>
            </select>
          </label>
          <p class="hint">
            {t((settings.video?.draw_mode ?? "sticky") === "once" ? "settings.draw_hint_once" : "settings.draw_hint_sticky", {
              shortcut: hotkeys.find((h) => h.action === "record_draw")?.label ?? "",
            })}
          </p>
        {:else}
          <p><b>Grabmark {version}</b> · © forellmedia.ch</p>
          <div class="buttons">
            <button onclick={() => api.reportBug()}>{t("settings.report_bug")}</button>
          </div>
          <p class="hint">{t("settings.report_hint")}</p>
          <label class="row">
            <span>{t("settings.log_retention")}</span>
            <select
              value={settings.log_retention_days ?? 30}
              onchange={(e) => {
                settings!.log_retention_days = +e.currentTarget.value;
                save();
              }}
            >
              {#each [10, 30, 60, 360] as n (n)}
                <option value={n}>{t("settings.days", { n: String(n) })}</option>
              {/each}
            </select>
          </label>
          <div class="buttons">
            <button onclick={() => api.openLogFolder()}>{t("settings.open_logs")}</button>
          </div>
          <p class="hint">{t("settings.log_hint")}</p>
          {#if diagnostics}
            <h2>{t("settings.diagnostics")}</h2>
            <p class="hint">{diagnostics.platform} · {diagnostics.library}</p>
            <ul class="timings">
              {#each diagnostics.timings as line, i (i)}
                <li>{line}</li>
              {:else}
                <li>{t("settings.no_timings")}</li>
              {/each}
            </ul>
          {/if}
          <p class="hint">{t("settings.licenses")}</p>
        {/if}
      </section>
    {/if}
    {#if error}
      <p class="hint warn" role="alert">{error}</p>
    {/if}
  </div>
</div>

<style>
  .page {
    height: 100vh;
    overflow: auto;
    background: var(--window-bg);
    color: var(--text);
  }
  .titlebar {
    display: none;
  }
  :global(.glass) .titlebar {
    position: sticky;
    top: 0;
    z-index: 2;
    display: grid;
    place-items: center;
    height: 38px;
    font-size: 13px;
    font-weight: 600;
    background: var(--glass);
    backdrop-filter: blur(24px) saturate(180%);
    -webkit-backdrop-filter: blur(24px) saturate(180%);
  }
  .content {
    max-width: 580px;
    margin: 0 auto;
    padding: var(--space-2) var(--space-6) var(--space-6);
  }
  h2 {
    font-size: 13px;
    margin: var(--space-4) 0 var(--space-1);
  }
  /* Reiter als Kapsel, Inhalt als Glasfläche darunter. */
  nav {
    display: flex;
    gap: 2px;
    width: fit-content;
    margin: var(--space-3) auto var(--space-4);
    padding: 4px;
    border-radius: 999px;
    background: var(--hover);
  }
  section {
    padding: 4px var(--space-4) var(--space-3);
    border-radius: var(--radius-panel);
    background: var(--glass);
    border: 1px solid var(--glass-edge);
    box-shadow: var(--glass-shadow);
  }
  nav button,
  .link,
  .buttons button {
    border: none;
    background: none;
    color: var(--text);
    font: inherit;
    cursor: pointer;
  }
  nav button {
    padding: 6px 14px;
    border-radius: 999px;
    font-size: 13px;
  }
  nav button.on {
    background: var(--glass-strong);
    font-weight: 600;
    box-shadow: 0 2px 6px rgb(30 40 90 / 0.12);
  }
  .row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-4);
    padding: 9px 0;
  }
  .row + .row {
    border-top: 1px solid var(--hover);
  }
  /* Ein/Aus als Schalter rechts – wie im Tray und in den Systemeinstellungen. */
  .row.check {
    flex-direction: row-reverse;
  }
  .row.check input[type="checkbox"] {
    appearance: none;
    -webkit-appearance: none;
    flex: none;
    position: relative;
    width: 30px;
    height: 18px;
    margin: 0;
    border-radius: 9px;
    background: var(--border);
    cursor: pointer;
    transition: background 0.15s;
  }
  .row.check input[type="checkbox"]::after {
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
  .row.check input[type="checkbox"]:checked {
    background: var(--switch-on);
  }
  .row.check input[type="checkbox"]:checked::after {
    transform: translateX(12px);
  }
  .row.check input[type="checkbox"]:disabled {
    opacity: 0.45;
    cursor: default;
  }
  .row.check input[type="checkbox"]:focus-visible {
    outline: 2px solid var(--selection);
    outline-offset: 2px;
  }
  select,
  input[type="text"] {
    font: inherit;
    padding: 5px 10px;
    border-radius: 8px;
    border: 1px solid var(--glass-edge);
    background: var(--glass-strong);
    color: var(--text);
    min-width: 200px;
  }
  .hk {
    display: flex;
    gap: var(--space-2);
    align-items: center;
  }
  .key {
    min-width: 120px;
    padding: 5px 10px;
    border-radius: 8px;
    border: 1px solid var(--glass-edge);
    background: var(--glass-strong);
    color: var(--text);
    font: inherit;
    cursor: pointer;
  }
  .key.rec {
    border-color: var(--selection);
    color: var(--selection);
  }
  .key.conflict {
    border-color: var(--accent);
  }
  .link {
    color: var(--selection);
  }
  .hint {
    color: var(--text-muted);
    font-size: 12px;
    margin: 2px 0 8px;
  }
  .warn {
    color: var(--accent);
  }
  .hint.ok {
    color: #2e7d32;
  }
  .buttons button {
    padding: 6px 14px;
    border-radius: 999px;
    border: 1px solid var(--glass-edge);
    background: var(--glass-strong);
  }
  code {
    font-size: 12px;
    color: var(--text-muted);
    max-width: 260px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .timings {
    font-size: 12px;
    font-family: ui-monospace, monospace;
    padding-left: 18px;
    color: var(--text-muted);
  }
</style>
