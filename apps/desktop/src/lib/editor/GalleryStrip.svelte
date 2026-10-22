<script lang="ts">
  // Bildliste rechts neben der Leinwand: Suche oben (bleibt stehen), darunter
  // die Aufnahmen nach Tagen, neueste zuerst. Nur die Liste scrollt.
  import { onMount, tick } from "svelte";
  import { api, fileUrl, on, type CaptureInfo } from "$lib/api";
  import { locale, t } from "$lib/i18n";
  import { groupByDay } from "./gallery";
  import Cleanup from "./Cleanup.svelte";
  import Icon from "./Icon.svelte";
  import StorageMeter from "./StorageMeter.svelte";
  import { editor } from "./state.svelte";

  const MODES = ["region", "window", "screen", "scroll", "video"] as const;

  let urls = $state<Record<string, string>>({});

  // Vorschaubilder laden (neu bei geänderter Version).
  $effect(() => {
    const version = editor.thumbVersion;
    for (const c of editor.captures) {
      fileUrl(c.thumbnailPath, version).then((u) => (urls[c.id] = u));
    }
  });

  const days = $derived(
    groupByDay(editor.captures, Date.now(), locale(), {
      today: t("gallery.today"),
      yesterday: t("gallery.yesterday"),
      unknown: t("gallery.unknown_date"),
    }),
  );

  // Geöffnete Aufnahme sichtbar halten (z. B. nach Wahl in der Galerie).
  let list: HTMLElement;
  $effect(() => {
    const id = editor.current?.id;
    void editor.captures;
    if (!id) return;
    tick().then(() => {
      const thumb = list?.querySelector<HTMLElement>(`[data-id="${CSS.escape(id)}"]`);
      if (!thumb) return;
      const box = list.getBoundingClientRect();
      const r = thumb.getBoundingClientRect();
      if (r.top < box.top || r.bottom > box.bottom) thumb.scrollIntoView({ block: "nearest" });
    });
  });

  // Der Hintergrund-Job meldet „zu voll“: Aufräumen öffnen (auch wenn das
  // Fenster erst dafür geöffnet wurde und die Meldung schon wartet).
  onMount(() => {
    const take = async () => {
      if (await api.takeStorageWarning()) editor.cleanupOpen = true;
    };
    void take();
    let stop: (() => void) | undefined;
    void on("storage-warning", take).then((f) => (stop = f));
    return () => stop?.();
  });

  let searchTimer: ReturnType<typeof setTimeout> | null = null;
  function onSearch(e: Event) {
    editor.query = (e.currentTarget as HTMLInputElement).value;
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(() => editor.refreshCaptures(), 200);
  }

  function time(ms: number | null) {
    return ms ? new Date(ms).toLocaleTimeString(locale(), { hour: "2-digit", minute: "2-digit" }) : "";
  }

  function kind(c: CaptureInfo) {
    const mode = MODES.find((m) => m === c.mode);
    return mode ? t(`gallery.mode.${mode}`) : "";
  }
</script>

<aside class="side" aria-label={t("gallery.title")}>
  <label class="search">
    <Icon name="search" size={15} />
    <input type="search" placeholder={t("gallery.search")} value={editor.query} oninput={onSearch} />
  </label>
  <div class="list" bind:this={list}>
    {#each days as day (day.key)}
      <h2>{day.label}</h2>
      {#each day.items as c (c.id)}
        <button
          class="thumb"
          data-id={c.id}
          class:current={editor.current?.id === c.id}
          onclick={() => editor.open(c.id)}
          oncontextmenu={(e) => editor.openCaptureMenu(e, c.id)}
          draggable="true"
          ondragstart={(e) => e.dataTransfer?.setData("text/x-nabshot-capture", c.id)}
        >
          <span class="image">
            {#if urls[c.id]}<img src={urls[c.id]} alt="" draggable="false" />{/if}
            {#if c.starred}<span class="star">★</span>{/if}
            {#if c.videoPath}<span class="play" aria-label={t("gallery.mode.video")}>▶</span>{/if}
          </span>
          <span class="meta"><span>{time(c.createdAt)}</span><span class="muted">{kind(c)}</span></span>
        </button>
      {/each}
    {:else}
      <p class="empty">{t("gallery.empty", { shortcut: editor.hotkey("capture_region") })}</p>
    {/each}
  </div>
  <StorageMeter compact />
</aside>

{#if editor.cleanupOpen}
  <Cleanup />
{/if}

<style>
  .side {
    display: flex;
    flex-direction: column;
    min-height: 0;
    gap: var(--space-2);
    padding: var(--space-2);
    border-radius: var(--radius-panel);
    background: var(--glass);
    border: 1px solid var(--glass-edge);
    box-shadow: var(--glass-shadow);
    backdrop-filter: blur(24px) saturate(180%);
    -webkit-backdrop-filter: blur(24px) saturate(180%);
  }
  /* Suche steht fest über der Liste – nur die Liste scrollt. */
  .search {
    flex: none;
    display: flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 10px;
    border-radius: 999px;
    background: var(--glass-strong);
    color: var(--text-muted);
  }
  .search input {
    flex: 1;
    min-width: 0;
    border: 0;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 13px;
    outline: none;
  }
  .list {
    flex: 1;
    min-height: 0;
    overflow-y: auto;
    display: flex;
    flex-direction: column;
    gap: 6px;
    padding: 2px;
    scrollbar-width: thin;
  }
  h2 {
    margin: 6px 4px 0;
    font-size: 12px;
    font-weight: 600;
    color: var(--text-muted);
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }
  .thumb {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 4px;
    padding: 5px;
    border: 0;
    border-radius: 12px;
    background: transparent;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  .thumb:hover {
    background: var(--hover);
  }
  .thumb.current {
    background: var(--glass-strong);
    box-shadow: inset 0 0 0 2px var(--active);
  }
  .image {
    position: relative;
    display: block;
    aspect-ratio: 16 / 10;
    border-radius: 8px;
    overflow: hidden;
    background: var(--canvas-bg);
  }
  .image img {
    width: 100%;
    height: 100%;
    object-fit: contain;
    display: block;
  }
  .meta {
    display: flex;
    justify-content: space-between;
    gap: 6px;
    padding: 0 3px;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }
  .muted {
    color: var(--text-muted);
  }
  .star {
    position: absolute;
    top: 3px;
    right: 5px;
    color: #fbc02d;
    text-shadow: 0 0 2px #000;
    font-size: 13px;
  }
  .play {
    position: absolute;
    left: 5px;
    bottom: 3px;
    color: #fff;
    text-shadow: 0 0 3px #000;
    font-size: 12px;
  }
  .empty {
    margin: var(--space-3) var(--space-2);
    color: var(--text-muted);
    font-size: 13px;
  }
</style>
