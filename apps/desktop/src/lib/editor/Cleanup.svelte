<script lang="ts">
  // „Speicher aufräumen“: Aufnahmen der Bibliothek bzw. Dateien des
  // Auto-Exports nach Datum oder Grösse sortieren, auswählen, löschen.
  // Öffnet sich über die Speicher-Anzeige oder von selbst, wenn eine Grenze
  // aus den Einstellungen überschritten ist.

  import { onMount } from "svelte";
  import { api, fileUrl, type StorageDetails, type StorageItem } from "$lib/api";
  import { formatBytes, share } from "$lib/bytes";
  import { locale, t } from "$lib/i18n";
  import { fileManager } from "$lib/shortcuts";
  import { editor } from "./state.svelte";

  type Tab = "library" | "export";
  type Sort = "oldest" | "newest" | "largest";

  let details = $state<StorageDetails | null>(null);
  let tab = $state<Tab>("library");
  let sort = $state<Sort>("oldest");
  let selected = $state<Set<string>>(new Set());
  let confirming = $state(false);
  let busy = $state(false);
  let error = $state<string | null>(null);
  let urls = $state<Record<string, string>>({});

  async function load() {
    try {
      details = await api.storageDetails();
      error = null;
      for (const item of details.items) {
        if (!urls[item.capture.id]) fileUrl(item.capture.thumbnailPath).then((u) => (urls[item.capture.id] = u));
      }
      // Zu viel exportiert, Bibliothek aber im Rahmen: gleich dort anfangen.
      const s = details.summary;
      if (share(s.exportBytes, s.exportLimit) > 1 && share(s.libraryBytes, s.libraryLimit) <= 1) tab = "export";
    } catch (e) {
      error = String(e);
    }
  }

  onMount(() => {
    void load();
  });

  const bytesOf = (item: StorageItem) => (tab === "library" ? item.libraryBytes : item.exportBytes) ?? 0;

  const items = $derived.by(() => {
    const list = (details?.items ?? []).filter((i) => bytesOf(i) > 0);
    const date = (i: StorageItem) => i.capture.createdAt ?? 0;
    if (sort === "oldest") list.sort((a, b) => date(a) - date(b));
    else if (sort === "newest") list.sort((a, b) => date(b) - date(a));
    else list.sort((a, b) => bytesOf(b) - bytesOf(a));
    return list;
  });

  const total = $derived((tab === "library" ? details?.summary.libraryBytes : details?.summary.exportBytes) ?? 0);
  const limit = $derived(tab === "library" ? (details?.summary.libraryLimit ?? null) : (details?.summary.exportLimit ?? null));
  const chosen = $derived(items.filter((i) => selected.has(i.capture.id)));
  const chosenBytes = $derived(chosen.reduce((a, i) => a + bytesOf(i), 0));

  function switchTab(next: Tab) {
    tab = next;
    selected = new Set();
    confirming = false;
  }

  function toggle(id: string) {
    const next = new Set(selected);
    if (next.has(id)) next.delete(id);
    else next.add(id);
    selected = next;
    confirming = false;
  }

  function selectAll(on: boolean) {
    selected = on ? new Set(items.map((i) => i.capture.id)) : new Set();
    confirming = false;
  }

  /** So viele der ältesten (ohne ★) auswählen, bis die Grenze wieder passt. */
  function selectToLimit() {
    if (!limit) return;
    let rest = total - limit * 0.9;
    const pick = new Set<string>();
    const oldest = [...items].filter((i) => !i.capture.starred).sort((a, b) => (a.capture.createdAt ?? 0) - (b.capture.createdAt ?? 0));
    for (const i of oldest) {
      if (rest <= 0) break;
      pick.add(i.capture.id);
      rest -= bytesOf(i);
    }
    selected = pick;
    sort = "oldest";
    confirming = false;
  }

  async function remove() {
    if (!confirming) {
      confirming = true;
      return;
    }
    const ids = chosen.map((i) => i.capture.id);
    busy = true;
    try {
      const freed = await api.cleanup(ids, tab === "library", tab === "export");
      editor.flash(t("storage.freed", { size: formatBytes(freed, locale()) }));
      if (tab === "library") await editor.afterCleanup(ids);
    } catch (e) {
      error = String(e);
    } finally {
      busy = false;
      confirming = false;
      selected = new Set();
      await load();
    }
  }

  function close() {
    editor.cleanupOpen = false;
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      if (confirming) confirming = false;
      else close();
    }
  }

  function when(ms: number | null) {
    return ms
      ? new Date(ms).toLocaleString(locale(), { day: "2-digit", month: "2-digit", year: "numeric", hour: "2-digit", minute: "2-digit" })
      : "";
  }

  function fileName(path: string | null) {
    return path?.split(/[\\/]/).pop() ?? "";
  }

  const MODES = ["region", "window", "screen", "scroll", "video"] as const;
  function kind(mode: string) {
    const m = MODES.find((x) => x === mode);
    return m ? t(`gallery.mode.${m}` as "gallery.mode.region") : mode;
  }
</script>

<svelte:window onkeydown={onKey} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onclick={close}>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="dialog" role="dialog" aria-modal="true" aria-label={t("storage.title")} tabindex="-1" onclick={(e) => e.stopPropagation()}>
    <header>
      <h2>{t("storage.title")}</h2>
      <button class="close" aria-label={t("storage.close")} onclick={close}>×</button>
    </header>

    {#if details}
      {@const s = details.summary}
      {#if share(s.libraryBytes, s.libraryLimit) > 1 || share(s.exportBytes, s.exportLimit) > 1}
        <p class="warn">
          {share(s.libraryBytes, s.libraryLimit) > 1
            ? t("storage.over_library", { size: formatBytes(s.libraryBytes, locale()), limit: formatBytes(s.libraryLimit ?? 0, locale()) })
            : t("storage.over_export", { size: formatBytes(s.exportBytes, locale()), limit: formatBytes(s.exportLimit ?? 0, locale()) })}
        </p>
      {/if}

      <div class="tabs" role="tablist">
        <button role="tab" aria-selected={tab === "library"} class:on={tab === "library"} onclick={() => switchTab("library")}>
          {t("storage.library")} · {formatBytes(s.libraryBytes, locale())}
        </button>
        <button role="tab" aria-selected={tab === "export"} class:on={tab === "export"} onclick={() => switchTab("export")}>
          {t("storage.export")} · {formatBytes(s.exportBytes, locale())}
        </button>
      </div>

      <div class="where">
        <span class="muted">{tab === "library" ? s.libraryDir : s.exportDir}</span>
        <button class="link" onclick={() => api.openStorageFolder(tab === "export")}>{t("storage.show_folder", { app: fileManager() })}</button>
        <span class="grow"></span>
        {#if limit}
          <span class="muted">{t("storage.limit", { limit: formatBytes(limit, locale()) })}</span>
        {/if}
      </div>
      <p class="hint">{t(tab === "library" ? "storage.library_hint" : "storage.export_hint")}</p>

      <div class="tools">
        <label>
          <span class="muted">{t("storage.sort")}</span>
          <select bind:value={sort}>
            <option value="oldest">{t("storage.sort_oldest")}</option>
            <option value="newest">{t("storage.sort_newest")}</option>
            <option value="largest">{t("storage.sort_largest")}</option>
          </select>
        </label>
        <button class="link" onclick={() => selectAll(chosen.length < items.length)}>
          {chosen.length < items.length ? t("storage.select_all") : t("storage.select_none")}
        </button>
        {#if limit && total > limit}
          <button class="link" onclick={selectToLimit}>{t("storage.select_to_limit")}</button>
        {/if}
      </div>

      <div class="list">
        {#each items as item (item.capture.id)}
          {@const c = item.capture}
          <label class="item" class:checked={selected.has(c.id)}>
            <input type="checkbox" checked={selected.has(c.id)} onchange={() => toggle(c.id)} />
            <span class="thumb">{#if urls[c.id]}<img src={urls[c.id]} alt="" />{/if}</span>
            <span class="info">
              <span>{when(c.createdAt)}{#if c.starred}<span class="star" title={t("storage.starred")}> ★</span>{/if}</span>
              <span class="muted">
                {tab === "export" ? fileName(item.exportPath) : `${kind(c.mode)} · ${c.width}×${c.height}`}
              </span>
            </span>
            <span class="size">{formatBytes(bytesOf(item), locale())}</span>
          </label>
        {:else}
          <p class="empty">{t("storage.empty")}</p>
        {/each}
      </div>
    {:else if !error}
      <p class="muted">{t("storage.measuring")}</p>
    {/if}

    {#if error}<p class="warn" role="alert">{error}</p>{/if}

    <footer>
      <button class="link" onclick={() => api.openMain("settings")}>{t("storage.settings")}</button>
      <span class="grow"></span>
      {#if chosen.length}
        <span class="muted">{t("storage.selected", { count: String(chosen.length), size: formatBytes(chosenBytes, locale()) })}</span>
      {/if}
      <button class="danger" class:confirm={confirming} disabled={!chosen.length || busy} onclick={remove}>
        {confirming
          ? t(tab === "library" ? "storage.confirm_library" : "storage.confirm_export", { count: String(chosen.length) })
          : t(tab === "library" ? "storage.delete_library" : "storage.delete_export")}
      </button>
    </footer>
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 30;
    display: grid;
    place-items: center;
    background: rgb(0 0 0 / 0.3);
  }
  .dialog {
    display: flex;
    flex-direction: column;
    gap: var(--space-2);
    width: min(720px, calc(100vw - 32px));
    max-height: min(760px, calc(100vh - 48px));
    padding: var(--space-3) var(--space-4);
    border: 1px solid var(--border);
    border-radius: 12px;
    background: var(--surface);
    color: var(--text);
    box-shadow: 0 12px 40px rgb(0 0 0 / 0.3);
    outline: none;
    font-size: 13px;
  }
  header,
  footer,
  .where,
  .tools {
    display: flex;
    align-items: center;
    gap: var(--space-2);
  }
  h2 {
    margin: 0;
    font-size: 15px;
  }
  .close {
    margin-left: auto;
    border: none;
    background: none;
    color: var(--text-muted);
    font-size: 20px;
    cursor: pointer;
  }
  .warn {
    margin: 0;
    padding: 8px 10px;
    border-radius: 8px;
    background: color-mix(in srgb, #e53935 12%, transparent);
    color: var(--text);
  }
  .hint {
    margin: 0;
    color: var(--text-muted);
    font-size: 12px;
  }
  .tabs {
    display: flex;
    gap: 4px;
  }
  .tabs button,
  footer button:not(.link),
  select {
    padding: 4px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--bg);
    color: var(--text);
    font: inherit;
    cursor: pointer;
  }
  .tabs button.on {
    background: var(--active);
    border-color: var(--active);
    color: var(--on-active);
  }
  .muted {
    color: var(--text-muted);
  }
  .where .muted {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
  }
  .grow {
    flex: 1;
  }
  button.link {
    border: none;
    background: none;
    padding: 0;
    color: var(--accent);
    font: inherit;
    cursor: pointer;
    white-space: nowrap;
  }
  .tools label {
    display: flex;
    align-items: center;
    gap: 6px;
  }
  .list {
    flex: 1;
    min-height: 120px;
    overflow-y: auto;
    border: 1px solid var(--border);
    border-radius: 8px;
  }
  .item {
    display: grid;
    grid-template-columns: auto 56px 1fr auto;
    align-items: center;
    gap: var(--space-2);
    padding: 6px 10px;
    border-bottom: 1px solid var(--border);
    cursor: pointer;
  }
  .item:last-child {
    border-bottom: none;
  }
  .item:hover {
    background: var(--hover);
  }
  .item.checked {
    background: color-mix(in srgb, var(--selection) 14%, transparent);
  }
  .thumb {
    width: 56px;
    height: 36px;
    border-radius: 4px;
    overflow: hidden;
    background: var(--canvas-bg);
  }
  .thumb img {
    width: 100%;
    height: 100%;
    object-fit: cover;
  }
  .info {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }
  .info .muted {
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
    font-size: 12px;
  }
  .star {
    color: #f9a825;
  }
  .size {
    font-variant-numeric: tabular-nums;
  }
  .empty {
    padding: var(--space-3);
    color: var(--text-muted);
    text-align: center;
  }
  footer {
    padding-top: var(--space-1);
  }
  button.danger {
    border-color: #e53935;
    color: #e53935;
  }
  button.danger.confirm {
    background: #e53935;
    color: #fff;
  }
  button:disabled {
    opacity: 0.45;
    cursor: default;
  }
</style>
