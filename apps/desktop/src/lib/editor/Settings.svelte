<script lang="ts">
  // Einstellungen: Allgemein, Tastenkürzel, Speichern, Info.

  import { onMount } from "svelte";
  import { api, on, type ContentSearchStatus, type Diagnostics, type HotkeyInfo, type Settings } from "$lib/api";
  import { t, type MessageKey } from "$lib/i18n";
  import { editor } from "./state.svelte";

  const isMac = typeof navigator !== "undefined" && /Mac/.test(navigator.platform);

  let settings = $state<Settings | null>(null);
  let hotkeys = $state<HotkeyInfo[]>([]);
  let diagnostics = $state<Diagnostics | null>(null);
  let contentStatus = $state<ContentSearchStatus | null>(null);
  /** macOS-Kürzel ⇧⌘3/4/5 noch eingeschaltet? `null` = unbekannt. */
  let systemShortcuts = $state<boolean | null>(null);
  let launchAtLogin = $state(false);
  let version = $state("");
  let tab = $state<"general" | "hotkeys" | "saving" | "about">("general");
  let recording = $state<string | null>(null);
  let initialLanguage: string | null | undefined;

  onMount(async () => {
    settings = await api.getSettings();
    initialLanguage = settings.language;
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
      editor.flash(String(e));
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
      if (settings.language !== initialLanguage) location.reload();
    } catch (e) {
      editor.flash(String(e));
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
    { id: "about", label: "settings.about" },
  ];
</script>

<svelte:window onfocus={refreshSystemShortcuts} />

<div class="backdrop" role="presentation" onpointerdown={() => (editor.settingsOpen = false)}>
  <div class="dialog" role="dialog" aria-label={t("settings.title")} tabindex="-1" onpointerdown={(e) => e.stopPropagation()}>
    <header>
      <h1>{t("settings.title")}</h1>
      <button class="close" aria-label="✕" onclick={() => (editor.settingsOpen = false)}>✕</button>
    </header>
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
          <p class="hint">{t("settings.hotkeys_hint")}</p>
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
            <span>{t("settings.save_format")}</span>
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
        {:else}
          <p><b>FMShot {version}</b> · © forellmedia.ch</p>
          <div class="buttons">
            <button onclick={() => api.reportBug()}>{t("settings.report_bug")}</button>
          </div>
          <p class="hint">{t("settings.report_hint")}</p>
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
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    background: rgb(0 0 0 / 0.35);
    display: grid;
    place-items: center;
    z-index: 40;
  }
  .dialog {
    width: min(560px, calc(100vw - 32px));
    max-height: calc(100vh - 64px);
    overflow: auto;
    background: var(--surface);
    color: var(--text);
    border-radius: 12px;
    box-shadow: 0 20px 60px rgb(0 0 0 / 0.35);
    padding: var(--space-4) var(--space-6) var(--space-6);
  }
  header {
    display: flex;
    justify-content: space-between;
    align-items: center;
  }
  h1 {
    font-size: 18px;
    margin: 0;
  }
  h2 {
    font-size: 13px;
    margin: var(--space-4) 0 var(--space-1);
  }
  nav {
    display: flex;
    gap: 4px;
    margin: var(--space-3) 0 var(--space-4);
    border-bottom: 1px solid var(--border);
  }
  nav button,
  .close,
  .link,
  .buttons button {
    border: none;
    background: none;
    color: var(--text);
    font: inherit;
    cursor: pointer;
  }
  nav button {
    padding: 8px 10px;
    border-bottom: 2px solid transparent;
    margin-bottom: -1px;
  }
  nav button.on {
    border-color: var(--accent);
    font-weight: 600;
  }
  .row {
    display: flex;
    justify-content: space-between;
    align-items: center;
    gap: var(--space-4);
    padding: 7px 0;
  }
  .row.check {
    justify-content: flex-start;
  }
  select,
  input[type="text"] {
    font: inherit;
    padding: 4px 8px;
    border-radius: 6px;
    border: 1px solid var(--border);
    background: var(--bg);
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
    padding: 4px 10px;
    border-radius: 6px;
    border: 1px solid var(--border);
    background: var(--bg);
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
    padding: 6px 12px;
    border-radius: 6px;
    border: 1px solid var(--border);
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
