<script lang="ts">
  // Galerie im Vollbild: alle Aufnahmen als Raster, nach Tagen gruppiert.
  // Suche im erkannten Text, Zeitraum, Art, nur mit Stern, Sortierung.
  // Gefiltert wird in der Bibliothek (fmshot-library), nicht hier.

  import { onMount } from "svelte";
  import { api, fileUrl, type CaptureInfo } from "$lib/api";
  import { locale, t } from "$lib/i18n";
  import { editor } from "./state.svelte";
  import { groupByDay, periodRange, PERIODS, type Period } from "./gallery";

  /** Höchstens so viele auf einmal – sonst Filter eingrenzen. */
  const LIMIT = 1000;
  const MODES = ["region", "window", "screen", "scroll"] as const;

  let text = $state(editor.query);
  let period = $state<Period>("all");
  let customFrom = $state("");
  let customTo = $state("");
  let mode = $state<(typeof MODES)[number] | "">("");
  let starredOnly = $state(false);
  let oldestFirst = $state(false);
  let results = $state<CaptureInfo[]>([]);
  let loading = $state(true);
  let urls = $state<Record<string, string>>({});
  let searchInput: HTMLInputElement;

  const groups = $derived(
    groupByDay(results, Date.now(), locale(), {
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
      });
      if (mine !== request) return; // Eine neuere Suche läuft schon.
      results = found;
      for (const c of found) {
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
    void [text, period, customFrom, customTo, mode, starredOnly, oldestFirst];
    if (timer) clearTimeout(timer);
    timer = setTimeout(load, 150);
  });

  onMount(() => searchInput.focus());

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
    if (starredOnly && !c.starred) results = results.filter((r) => r.id !== c.id);
    void editor.refreshCaptures();
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.stopPropagation();
      editor.galleryOpen = false;
    }
  }

  function time(ms: number | null) {
    return ms ? new Date(ms).toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" }) : "";
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="gallery" role="dialog" aria-label={t("gallery.title")}>
  <header>
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
    <span class="count">{loading ? "…" : results.length === 1 ? t("gallery.count_one") : t("gallery.count", { count: String(results.length) })}</span>
    <button class="close" title="{t('gallery.close')} (Esc)" aria-label={t("gallery.close")} onclick={() => (editor.galleryOpen = false)}>
      ✕
    </button>
  </header>

  <div class="scroll">
    {#if results.length === LIMIT}
      <p class="note">{t("gallery.limit", { count: String(LIMIT) })}</p>
    {/if}
    {#each groups as g (g.key)}
      <section>
        <h2>{g.label}</h2>
        <div class="grid">
          {#each g.items as c (c.id)}
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
            </div>
          {/each}
        </div>
      </section>
    {:else}
      {#if !loading}<p class="empty">{t("gallery.none")}</p>{/if}
    {/each}
  </div>
</div>

<style>
  .gallery {
    position: fixed;
    inset: 0;
    z-index: 20;
    display: grid;
    grid-template-rows: auto 1fr;
    background: var(--bg, var(--surface));
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
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
  }
  .count {
    color: var(--text-muted);
    white-space: nowrap;
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
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent);
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
