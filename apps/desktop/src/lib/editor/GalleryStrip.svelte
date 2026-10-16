<script lang="ts">
  import { fileUrl, type CaptureInfo } from "$lib/api";
  import { t } from "$lib/i18n";
  import { editor } from "./state.svelte";

  let menu = $state<{ x: number; y: number; capture: CaptureInfo } | null>(null);
  let urls = $state<Record<string, string>>({});

  // Vorschaubilder laden (neu bei geänderter Version).
  $effect(() => {
    const version = editor.thumbVersion;
    for (const c of editor.captures) {
      fileUrl(c.thumbnailPath, version).then((u) => (urls[c.id] = u));
    }
  });

  function contextMenu(e: MouseEvent, capture: CaptureInfo) {
    e.preventDefault();
    menu = { x: e.clientX, y: e.clientY, capture };
  }

  async function act(fn: (c: CaptureInfo) => Promise<unknown>) {
    const c = menu?.capture;
    menu = null;
    if (c) await fn(c);
  }

  function time(ms: number | null) {
    return ms ? new Date(ms).toLocaleString() : "";
  }
</script>

<svelte:window onclick={() => (menu = null)} />

<section class="strip" aria-label={t("gallery.title")}>
  {#each editor.captures as c (c.id)}
    <button
      class="thumb"
      class:current={editor.current?.id === c.id}
      title={time(c.createdAt)}
      onclick={() => editor.open(c.id)}
      oncontextmenu={(e) => contextMenu(e, c)}
      draggable="true"
      ondragstart={(e) => e.dataTransfer?.setData("text/x-fmshot-capture", c.id)}
    >
      {#if urls[c.id]}
        <img src={urls[c.id]} alt="" draggable="false" />
      {/if}
      {#if c.starred}<span class="star">★</span>{/if}
    </button>
  {:else}
    <p class="empty">{t("gallery.empty")}</p>
  {/each}
</section>

{#if menu}
  <ul class="menu" style:left="{menu.x}px" style:top="{menu.y}px" role="menu">
    <li><button onclick={() => act((c) => editor.open(c.id))}>{t("gallery.open")}</button></li>
    <li>
      <button onclick={() => act((c) => editor.toggleStar(c))}>
        {menu.capture.starred ? t("gallery.unstar") : t("gallery.star")}
      </button>
    </li>
    <li><button onclick={() => act(async (c) => (await import("$lib/api")).api.revealCapture(c.id))}>{t("gallery.reveal")}</button></li>
    <li class="danger"><button onclick={() => act((c) => editor.deleteCapture(c.id))}>{t("gallery.delete")}</button></li>
  </ul>
{/if}

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
    border-color: var(--accent);
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
  .empty {
    color: var(--text-muted);
    margin: 0 auto;
  }
  .menu {
    position: fixed;
    z-index: 20;
    list-style: none;
    margin: 0;
    padding: 4px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 6px 24px rgb(0 0 0 / 0.2);
    transform: translateY(-100%);
  }
  .menu button {
    display: block;
    width: 100%;
    text-align: left;
    padding: 6px 12px;
    border: none;
    background: none;
    color: var(--text);
    font: inherit;
    border-radius: 5px;
    cursor: pointer;
  }
  .menu button:hover {
    background: var(--hover);
  }
  .danger button {
    color: var(--accent);
  }
</style>
