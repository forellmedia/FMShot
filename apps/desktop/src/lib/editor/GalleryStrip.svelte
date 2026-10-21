<script lang="ts">
  import { tick } from "svelte";
  import { fileUrl } from "$lib/api";
  import { t } from "$lib/i18n";
  import { editor } from "./state.svelte";

  let urls = $state<Record<string, string>>({});

  // Vorschaubilder laden (neu bei geänderter Version).
  $effect(() => {
    const version = editor.thumbVersion;
    for (const c of editor.captures) {
      fileUrl(c.thumbnailPath, version).then((u) => (urls[c.id] = u));
    }
  });

  // Geöffnete Aufnahme sichtbar halten (z. B. nach Wahl in der Galerie).
  let strip: HTMLElement;
  $effect(() => {
    const id = editor.current?.id;
    void editor.captures;
    if (!id) return;
    tick().then(() => {
      const thumb = strip?.querySelector<HTMLElement>(`[data-id="${CSS.escape(id)}"]`);
      if (!thumb) return;
      const box = strip.getBoundingClientRect();
      const r = thumb.getBoundingClientRect();
      if (r.left < box.left || r.right > box.right) thumb.scrollIntoView({ inline: "center", block: "nearest" });
    });
  });

  let searchTimer: ReturnType<typeof setTimeout> | null = null;
  function onSearch(e: Event) {
    editor.query = (e.currentTarget as HTMLInputElement).value;
    if (searchTimer) clearTimeout(searchTimer);
    searchTimer = setTimeout(() => editor.refreshCaptures(), 200);
  }

  function time(ms: number | null) {
    return ms ? new Date(ms).toLocaleString() : "";
  }
</script>

<section class="strip" bind:this={strip} aria-label={t("gallery.title")}>
  <input class="search" type="search" placeholder={t("gallery.search")} value={editor.query} oninput={onSearch} />
  {#each editor.captures as c (c.id)}
    <button
      class="thumb"
      data-id={c.id}
      class:current={editor.current?.id === c.id}
      title={time(c.createdAt)}
      onclick={() => editor.open(c.id)}
      oncontextmenu={(e) => editor.openCaptureMenu(e, c.id)}
      draggable="true"
      ondragstart={(e) => e.dataTransfer?.setData("text/x-grabmark-capture", c.id)}
    >
      {#if urls[c.id]}
        <img src={urls[c.id]} alt="" draggable="false" />
      {/if}
      {#if c.starred}<span class="star">★</span>{/if}
      {#if c.videoPath}<span class="play" aria-label={t("gallery.mode.video")}>▶</span>{/if}
    </button>
  {:else}
    <p class="empty">{t("gallery.empty", { shortcut: editor.hotkey("capture_region") })}</p>
  {/each}
</section>

<style>
  .strip {
    display: flex;
    gap: var(--space-2);
    padding: var(--space-2) var(--space-3);
    overflow-x: auto;
    background: var(--surface);
    border-top: 1px solid var(--border);
    min-height: 92px;
    align-items: center;
  }
  .search {
    flex: none;
    width: 180px;
    align-self: center;
    padding: 6px 10px;
    border-radius: 8px;
    border: 1px solid var(--border);
    background: var(--bg);
    color: var(--text);
    font: inherit;
  }
  .thumb {
    position: relative;
    flex: none;
    height: 72px;
    min-width: 72px;
    max-width: 140px;
    padding: 0;
    border: 2px solid transparent;
    border-radius: 6px;
    background: var(--canvas-bg);
    overflow: hidden;
    cursor: pointer;
  }
  .thumb.current {
    border-color: var(--active);
  }
  .thumb img {
    height: 100%;
    width: 100%;
    object-fit: contain;
    display: block;
  }
  .star {
    position: absolute;
    top: 2px;
    right: 4px;
    color: #fbc02d;
    text-shadow: 0 0 2px #000;
    font-size: 13px;
  }
  .play {
    position: absolute;
    left: 4px;
    bottom: 3px;
    color: #fff;
    text-shadow: 0 0 3px #000;
    font-size: 12px;
  }
  .empty {
    color: var(--text-muted);
    margin: 0 auto;
  }
</style>
