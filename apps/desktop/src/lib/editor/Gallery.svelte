<script lang="ts">
  // Galerie im Vollbild: alle Aufnahmen als Raster, nach Tagen gruppiert.
  // Suche im erkannten Text und im Bildinhalt (ADR 0010), Schnellfilter,
  // Zeitraum, Art, nur mit Stern, Sortierung. Gefiltert wird im Backend.

  import { onMount } from "svelte";
  import { api, fileUrl, on, type CaptureInfo, type ContentSearchStatus, type GalleryItem, type GalleryResult } from "$lib/api";
  import { locale, t, type MessageKey } from "$lib/i18n";
  import { editor } from "./state.svelte";
  import { fitInRow, groupByDay, periodRange, PERIODS, type Period } from "./gallery";
  import Icon from "./Icon.svelte";
  import Popover from "./Popover.svelte";
  import StorageMeter from "./StorageMeter.svelte";

  /** Höchstens so viele auf einmal – sonst Filter eingrenzen. */
  const LIMIT = 1000;
  const MODES = ["region", "window", "screen", "scroll", "video"] as const;

  let text = $state(editor.query);
  let period = $state<Period>("all");
  let customFrom = $state("");
  let customTo = $state("");
  let mode = $state<(typeof MODES)[number] | "">("");
  let starredOnly = $state(false);
  let oldestFirst = $state(false);
  let filter = $state<string | null>(null);
  let filters = $state<GalleryResult["groups"]>([]);
  /** Schnellfilter alle zeigen (umbrechend) oder nur eine Zeile mit „Mehr…“. */
  let filtersOpen = $state(false);
  let chipsWidth = $state(0);
  let measure = $state<HTMLElement>();
  /** Indizes der Filter, die eingeklappt in die Zeile passen. */
  let fitting = $state<number[] | null>(null);

  const chips = $derived([
    { key: "", label: t("gallery.groups.all"), count: null as number | null, on: filter === null, pick: () => (filter = null) },
    ...filters.map((f) => ({
      key: f.group,
      label: t(`group.${f.group}` as MessageKey),
      count: f.count as number | null,
      on: filter === f.group,
      pick: () => (filter = filter === f.group ? null : f.group),
    })),
  ]);
  const overflow = $derived(fitting != null && fitting.length < chips.length);

  // Breiten aus der unsichtbaren Messzeile: so viele Filter, wie mit
  // „Mehr…“ in eine Zeile passen; der gewählte bleibt immer dabei.
  $effect(() => {
    void chips;
    if (!measure || chipsWidth <= 0) return;
    const widths = [...measure.children].map((el) => (el as HTMLElement).offsetWidth);
    const more = widths.pop() ?? 0;
    const keep = chips.findIndex((c) => c.on);
    fitting = fitInRow(widths, chipsWidth, 6, more, keep < 0 ? null : keep);
  });
  let status = $state<ContentSearchStatus | null>(null);
  /** Bildinhalt-Erkennung eingeschaltet (Einstellungen). */
  let contentOn = $state(false);

  let results = $state<GalleryItem[]>([]);
  let loading = $state(true);
  let urls = $state<Record<string, string>>({});
  /** Offene Stichwörter: beim Darüberfahren, per Klick festgehalten. */
  let tagPop = $state<{ item: GalleryItem; anchor: HTMLElement; pinned: boolean } | null>(null);
  let tagTimer: ReturnType<typeof setTimeout> | undefined;
  let searchInput: HTMLInputElement;

  const groups = $derived(
    groupByDay(results.map((r) => ({ ...r, createdAt: r.capture.createdAt })), Date.now(), locale(), {
      today: t("gallery.today"),
      yesterday: t("gallery.yesterday"),
      unknown: t("gallery.unknown_date"),
    }),
  );

  let request = 0;
  async function load() {
    const mine = ++request;
    loading = true;
    const range = periodRange(period, Date.now(), { from: customFrom, to: customTo });
    try {
      const found = await api.queryCaptures({
        text,
        fromMs: range.fromMs,
        toMs: range.toMs,
        mode: mode || null,
        starredOnly,
        oldestFirst,
        limit: LIMIT,
        group: filter,
      });
      if (mine !== request) return; // Eine neuere Suche läuft schon.
      results = found.items;
      filters = found.groups;
      // Gewählter Filter ohne Treffer mehr (z. B. nach neuer Suche): aufheben.
      if (filter && !found.groups.some((g) => g.group === filter)) filter = null;
      for (const { capture: c } of found.items) {
        if (!urls[c.id]) fileUrl(c.thumbnailPath, editor.thumbVersion).then((u) => (urls[c.id] = u));
      }
    } catch (e) {
      editor.flash(String(e));
    } finally {
      if (mine === request) loading = false;
    }
  }

  // Neu suchen, sobald sich ein Filter ändert (Text leicht verzögert).
  let timer: ReturnType<typeof setTimeout> | null = null;
  $effect(() => {
    void [text, period, customFrom, customTo, mode, starredOnly, oldestFirst, filter];
    if (timer) clearTimeout(timer);
    timer = setTimeout(load, 150);
  });

  onMount(() => {
    searchInput.focus();
    api.getSettings().then((s) => (contentOn = s.content_search ?? true));
    api.contentSearchStatus().then((s) => (status = s)).catch(() => {});
    let stop: (() => void) | undefined;
    on<ContentSearchStatus>("content-index-progress", (s) => (status = s)).then((f) => (stop = f));
    // Neue Stichwörter: Ergebnis auffrischen.
    let stopLib: (() => void) | undefined;
    on("library-changed", () => load()).then((f) => (stopLib = f));
    return () => {
      stop?.();
      stopLib?.();
    };
  });

  const indexing = $derived(contentOn && status?.available && status.done < status.total);

  function tagLabel(key: string) {
    return t(`tag.${key}` as MessageKey);
  }

  function showTags(item: GalleryItem, anchor: HTMLElement, pinned: boolean) {
    clearTimeout(tagTimer);
    if (tagPop?.item.capture.id === item.capture.id && (tagPop.pinned || !pinned)) {
      // Erneuter Klick auf festgehaltene Stichwörter schliesst sie.
      if (pinned) tagPop = null;
      return;
    }
    tagPop = { item, anchor, pinned };
  }

  /** Maus verlässt Knopf oder Popover: kurz warten, ob sie hinüberwechselt. */
  function leaveTags() {
    if (tagPop && !tagPop.pinned) tagTimer = setTimeout(() => (tagPop = null), 200);
  }

  function searchTag(tag: string) {
    tagPop = null;
    text = tagLabel(tag);
  }

  /** Warum eine Aufnahme zur Suche passt – nur bei Suchtext. */
  function reason(item: GalleryItem): string | null {
    if (!text.trim() || item.matched.length === 0) return null;
    const m = item.matched[0];
    if (m === "text") return t("gallery.match.text");
    if (m === "content") return t("gallery.match.content");
    return t("gallery.match.tag", { tag: tagLabel(m) });
  }

  async function open(c: CaptureInfo) {
    editor.galleryOpen = false;
    // Steht die Aufnahme wegen der Suche nicht in der Leiste unten,
    // die Suche dort leeren – sonst lässt sie sich nicht anzeigen.
    if (!editor.captures.some((x) => x.id === c.id) && editor.query) {
      editor.query = "";
      await editor.refreshCaptures();
    }
    await editor.open(c.id);
  }

  async function toggleStar(e: MouseEvent, c: CaptureInfo) {
    e.stopPropagation();
    await api.setStarred(c.id, !c.starred);
    c.starred = !c.starred;
    if (starredOnly && !c.starred) results = results.filter((r) => r.capture.id !== c.id);
    void editor.refreshCaptures();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.stopPropagation();
      editor.galleryOpen = false;
    }
  }

  /** „1:23“ – Länge eines Videos. */
  function formatDuration(ms: number) {
    const s = Math.round(ms / 1000);
    return `${Math.floor(s / 60)}:${String(s % 60).padStart(2, "0")}`;
  }

  function time(ms: number | null) {
    return ms ? new Date(ms).toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" }) : "";
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="gallery" role="dialog" aria-label={t("gallery.title")}>
  <header data-tauri-drag-region>
    <h1>{t("gallery.title")}</h1>
    <input
      bind:this={searchInput}
      class="search"
      type="search"
      placeholder={t("gallery.search")}
      bind:value={text}
    />
    <select bind:value={period} aria-label={t("gallery.period")}>
      {#each PERIODS as p (p)}
        <option value={p}>{t(`gallery.period.${p}`)}</option>
      {/each}
    </select>
    {#if period === "custom"}
      <label>{t("gallery.from")} <input type="date" bind:value={customFrom} /></label>
      <label>{t("gallery.to")} <input type="date" bind:value={customTo} /></label>
    {/if}
    <select bind:value={mode} aria-label={t("gallery.mode")}>
      <option value="">{t("gallery.mode.all")}</option>
      {#each MODES as m (m)}
        <option value={m}>{t(`gallery.mode.${m}`)}</option>
      {/each}
    </select>
    <select bind:value={oldestFirst} aria-label={t("gallery.sort")}>
      <option value={false}>{t("gallery.sort.newest")}</option>
      <option value={true}>{t("gallery.sort.oldest")}</option>
    </select>
    <button class="toggle" class:on={starredOnly} onclick={() => (starredOnly = !starredOnly)}>
      ★ {t("gallery.starred_only")}
    </button>
    {#if indexing && status}
      <span class="indexing">{t("gallery.indexing", { done: String(status.done), total: String(status.total) })}</span>
    {/if}
    <span class="count">{loading ? "…" : results.length === 1 ? t("gallery.count_one") : t("gallery.count", { count: String(results.length) })}</span>
    <button class="close" title="{t('gallery.close')} (Esc)" aria-label={t("gallery.close")} onclick={() => (editor.galleryOpen = false)}>
      ✕
    </button>
  </header>

  {#if filters.length && contentOn && status?.available}
    <div class="filters">
      <!-- Unsichtbare Zeile mit allen Filtern, nur zum Messen der Breiten. -->
      <div class="measure" aria-hidden="true" bind:this={measure}>
        {#each chips as c (c.key)}
          <span class="chip">{c.label}{#if c.count != null} <span class="n">{c.count}</span>{/if}</span>
        {/each}
        <span class="chip more">{t("gallery.more")}</span>
      </div>
      <div
        class="chips"
        class:open={filtersOpen}
        role="toolbar"
        aria-label={t("gallery.groups")}
        bind:clientWidth={chipsWidth}
      >
        {#each chips as c, i (c.key)}
          {#if filtersOpen || !fitting || fitting.includes(i)}
            <button class="chip" class:on={c.on} onclick={c.pick}>
              {c.label}{#if c.count != null} <span class="n">{c.count}</span>{/if}
            </button>
          {/if}
        {/each}
        {#if overflow}
          <button class="chip more" aria-expanded={filtersOpen} onclick={() => (filtersOpen = !filtersOpen)}>
            {filtersOpen ? t("gallery.less") : t("gallery.more")}
          </button>
        {/if}
      </div>
    </div>
  {/if}

  <div class="scroll">
    {#if results.length === LIMIT}
      <p class="note">{t("gallery.limit", { count: String(LIMIT) })}</p>
    {/if}
    {#each groups as g (g.key)}
      <section>
        <h2>{g.label}</h2>
        <div class="grid">
          {#each g.items as item (item.capture.id)}
            {@const c = item.capture}
            {@const why = reason(item)}
            <div
              class="tile"
              class:current={editor.current?.id === c.id}
              role="button"
              tabindex="0"
              onclick={() => open(c)}
              onkeydown={(e) => (e.key === "Enter" || e.key === " ") && open(c)}
            >
              <div class="image">
                {#if urls[c.id]}<img src={urls[c.id]} alt="" draggable="false" />{/if}
                {#if c.durationMs != null}<span class="duration">▶ {formatDuration(c.durationMs)}</span>{/if}
                {#if why}<span class="why">{why}</span>{/if}
              </div>
              <div class="meta">
                <span>{time(c.createdAt)}</span>
                <span class="muted">{t(`gallery.mode.${c.mode as (typeof MODES)[number]}`) ?? c.mode} · {c.width}×{c.height}</span>
              </div>
              <button
                class="star"
                class:on={c.starred}
                title={c.starred ? t("gallery.unstar") : t("gallery.star")}
                aria-label={c.starred ? t("gallery.unstar") : t("gallery.star")}
                onclick={(e) => toggleStar(e, c)}
              >
                {c.starred ? "★" : "☆"}
              </button>
              {#if item.tags.length}
                <button
                  class="tagbtn"
                  class:open={tagPop?.item.capture.id === c.id}
                  aria-label={t("gallery.tags")}
                  aria-haspopup="dialog"
                  aria-expanded={tagPop?.item.capture.id === c.id}
                  onpointerenter={(e) => showTags(item, e.currentTarget, false)}
                  onpointerleave={leaveTags}
                  onclick={(e) => {
                    e.stopPropagation();
                    showTags(item, e.currentTarget, true);
                  }}
                  onkeydown={(e) => e.stopPropagation()}
                >
                  <Icon name="tag" size={15} />
                </button>
              {/if}
            </div>
          {/each}
        </div>
      </section>
    {:else}
      {#if !loading}<p class="empty">{t("gallery.none")}</p>{/if}
    {/each}
  </div>
  <footer class="storage">
    <StorageMeter />
  </footer>
</div>

{#if tagPop}
  <Popover
    anchor={tagPop.anchor}
    label={t("gallery.tags")}
    onclose={() => (tagPop = null)}
    onpointerenter={() => clearTimeout(tagTimer)}
    onpointerleave={leaveTags}
  >
    <div class="pop-title">{t("gallery.tags")}</div>
    <div class="tags">
      {#each tagPop.item.tags as tag (tag)}
        <button class="tag" title={t("gallery.search_tag")} onclick={() => searchTag(tag)}>{tagLabel(tag)}</button>
      {/each}
    </div>
  </Popover>
{/if}

<style>
  .gallery {
    position: fixed;
    inset: 0;
    z-index: 20;
    display: grid;
    grid-template-rows: auto auto 1fr auto;
    background: var(--bg, var(--surface));
  }
  footer.storage {
    display: flex;
    justify-content: flex-end;
    padding: 2px var(--space-4);
    border-top: 1px solid var(--border);
    background: var(--surface);
  }
  header {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    background: var(--surface);
    border-bottom: 1px solid var(--border);
  }
  /* Platz für die Fensterknöpfe von macOS (Titelleiste über dem Inhalt). */
  :global(.glass) header {
    padding-left: 84px;
  }
  h1 {
    margin: 0 var(--space-3) 0 0;
    font-size: 18px;
  }
  .search {
    flex: 1 1 220px;
    min-width: 160px;
    max-width: 420px;
  }
  input,
  select {
    padding: 5px 8px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: var(--surface);
    color: var(--text);
    font: inherit;
  }
  label {
    display: flex;
    align-items: center;
    gap: 6px;
    color: var(--text-muted);
  }
  .toggle,
  .close {
    padding: 5px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: transparent;
    color: var(--text);
    font: inherit;
    cursor: pointer;
    white-space: nowrap;
  }
  .toggle.on {
    background: var(--active);
    border-color: var(--active);
    color: var(--on-active);
  }
  .count,
  .indexing {
    color: var(--text-muted);
    white-space: nowrap;
  }
  .indexing {
    font-size: 12px;
  }
  /* Schnellfilter: kein Querscrollen. Eingeklappt eine Zeile mit „Mehr…“,
     aufgeklappt umbrechend, höchstens vier Zeilen hoch, dann scrollt sie. */
  .filters {
    position: relative;
    padding: var(--space-2) var(--space-4);
    background: var(--surface);
    border-bottom: 1px solid var(--border);
  }
  .chips {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
    max-height: 30px;
    overflow: hidden;
  }
  .chips.open {
    max-height: calc(4 * 30px + 3 * 6px);
    overflow-y: auto;
  }
  .measure {
    position: absolute;
    left: 0;
    top: 0;
    display: flex;
    gap: 6px;
    visibility: hidden;
    pointer-events: none;
    white-space: nowrap;
  }
  .chip.more {
    color: var(--active);
    border-color: transparent;
  }
  .chip {
    flex: none;
    box-sizing: border-box;
    height: 30px;
    padding: 4px 12px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 13px;
    cursor: pointer;
    white-space: nowrap;
  }
  .chip:hover {
    background: var(--hover);
  }
  .chip .n {
    margin-left: 2px;
    opacity: 0.6;
    font-variant-numeric: tabular-nums;
  }
  .chip.on {
    background: var(--active);
    border-color: var(--active);
    color: var(--on-active);
  }
  /* Grund des Treffers (immer). */
  .why {
    position: absolute;
    top: 6px;
    left: 6px;
    max-width: calc(100% - 82px);
    padding: 2px 8px;
    border-radius: 999px;
    background: rgb(0 0 0 / 0.6);
    color: #fff;
    font-size: 11px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .duration {
    position: absolute;
    right: 6px;
    bottom: 6px;
    padding: 2px 8px;
    border-radius: 999px;
    background: rgb(0 0 0 / 0.6);
    color: #fff;
    font-size: 11px;
    font-variant-numeric: tabular-nums;
  }

  /* Stichwörter: Symbol neben dem Stern, Liste als Popover daran. */
  .tagbtn {
    position: absolute;
    top: 6px;
    right: 40px;
    display: grid;
    place-items: center;
    width: 28px;
    height: 28px;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: rgb(0 0 0 / 0.45);
    color: #fff;
    cursor: pointer;
    opacity: 0;
  }
  .tile:hover .tagbtn,
  .tile:focus-visible .tagbtn,
  .tagbtn:focus-visible,
  .tagbtn.open {
    opacity: 1;
  }
  .tagbtn.open {
    background: var(--active);
    color: var(--on-active);
  }
  .pop-title {
    margin-bottom: 8px;
    color: var(--text-muted);
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
  }
  .tags {
    display: flex;
    flex-wrap: wrap;
    gap: 6px;
  }
  .tag {
    max-width: 100%;
    padding: 3px 10px;
    border: 1px solid var(--border);
    border-radius: 999px;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
    cursor: pointer;
  }
  .tag:hover,
  .tag:focus-visible {
    background: var(--active);
    border-color: var(--active);
    color: var(--on-active);
    outline: none;
  }
  .close {
    margin-left: auto;
  }
  .scroll {
    overflow-y: auto;
    padding: var(--space-4);
    background: var(--canvas-bg);
  }
  h2 {
    margin: var(--space-4) 0 var(--space-3);
    font-size: 14px;
    color: var(--text-muted);
  }
  section:first-child h2 {
    margin-top: 0;
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(auto-fill, minmax(200px, 1fr));
    gap: var(--space-3);
  }
  .tile {
    position: relative;
    display: flex;
    flex-direction: column;
    border: 1px solid var(--border);
    border-radius: 8px;
    overflow: hidden;
    background: var(--surface);
    cursor: pointer;
  }
  .tile:hover,
  .tile:focus-visible {
    border-color: var(--selection);
    outline: none;
  }
  .tile.current {
    border-color: var(--active);
    box-shadow: 0 0 0 1px var(--active);
  }
  .image {
    position: relative;
    aspect-ratio: 16 / 10;
    overflow: hidden;
    background: var(--hover);
  }
  /* Ganzes Bild zeigen, egal ob Hoch- oder Querformat. */
  img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: contain;
  }
  .meta {
    display: flex;
    justify-content: space-between;
    gap: 6px;
    padding: 6px 8px;
    font-size: 12px;
  }
  .muted {
    color: var(--text-muted);
  }
  .star {
    position: absolute;
    top: 6px;
    right: 6px;
    width: 28px;
    height: 28px;
    border: none;
    border-radius: 50%;
    background: rgb(0 0 0 / 0.45);
    color: #fff;
    font-size: 15px;
    cursor: pointer;
    opacity: 0;
  }
  .tile:hover .star,
  .star.on {
    opacity: 1;
  }
  .star.on {
    color: #fdd835;
  }
  .note,
  .empty {
    color: var(--text-muted);
  }
  .empty {
    text-align: center;
    margin-top: 15vh;
  }
</style>
