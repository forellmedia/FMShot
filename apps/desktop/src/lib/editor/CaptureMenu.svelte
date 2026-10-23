<script lang="ts">
  // Kontextmenü einer Aufnahme – dasselbe für die Leiste unten und die
  // Leinwand. Öffnet am Mauszeiger und bleibt ganz im Fenster. Schliesst bei
  // Klick daneben, Escape, Scrollen und Grössenänderung.

  import { api, type CaptureInfo } from "$lib/api";
  import { t } from "$lib/i18n";
  import { fileManager } from "$lib/shortcuts";
  import { editor } from "./state.svelte";

  let menuEl = $state<HTMLElement>();
  let pos = $state({ x: 0, y: 0, ready: false });

  /** Aktueller Stand der Aufnahme (Stern kann sich geändert haben). */
  const capture = $derived.by((): CaptureInfo | null => {
    const id = editor.captureMenu?.id;
    if (!id) return null;
    return (
      editor.captures.find((c) => c.id === id) ??
      (editor.current?.id === id ? editor.current : null) ??
      editor.captureMenu?.capture ??
      null
    );
  });

  /** Stern oder Löschen: Die Galerie zeigt den neuen Stand. */
  async function change(fn: () => Promise<unknown>) {
    await fn();
    editor.galleryRevision++;
  }

  // Am Zeiger; reicht der Platz nicht, nach links bzw. oben aufklappen.
  $effect(() => {
    const at = editor.captureMenu;
    if (!at || !menuEl) return;
    const { width: w, height: h } = menuEl.getBoundingClientRect();
    const x = at.x + w > window.innerWidth - 8 ? Math.max(8, at.x - w) : at.x;
    const y = at.y + h > window.innerHeight - 8 ? Math.max(8, at.y - h) : at.y;
    pos = { x, y, ready: true };
  });

  $effect(() => {
    if (!editor.captureMenu) pos.ready = false;
  });

  const close = () => (editor.captureMenu = null);

  async function act(fn: (c: CaptureInfo) => Promise<unknown>) {
    const c = capture;
    close();
    if (c) await fn(c);
  }

  function onPointerDown(e: PointerEvent) {
    if (editor.captureMenu && !menuEl?.contains(e.target as Node)) close();
  }

  function onKey(e: KeyboardEvent) {
    if (editor.captureMenu && e.key === "Escape") {
      e.stopPropagation();
      close();
    }
  }
</script>

<svelte:window onpointerdown={onPointerDown} onkeydowncapture={onKey} onresize={close} onblur={close} />

{#if editor.captureMenu && capture}
  <ul
    class="menu"
    class:ready={pos.ready}
    bind:this={menuEl}
    style:left="{pos.x}px"
    style:top="{pos.y}px"
    role="menu"
    onscroll={close}
  >
    {#if editor.galleryOpen || editor.current?.id !== capture.id}
      <li><button role="menuitem" onclick={() => act((c) => editor.openFromGallery(c.id))}>{t("gallery.open")}</button></li>
    {/if}
    {#if !editor.galleryOpen}
      <li>
        <button role="menuitem" onclick={() => act(async (c) => editor.showInGallery(c.id))}>{t("gallery.show_in_gallery")}</button>
      </li>
    {/if}
    <li>
      <button role="menuitem" onclick={() => act((c) => change(() => editor.toggleStar(c)))}>
        {capture.starred ? t("gallery.unstar") : t("gallery.star")}
      </button>
    </li>
    <li>
      <button role="menuitem" onclick={() => act((c) => api.revealCapture(c.id))}>{t("gallery.reveal", { app: fileManager() })}</button>
    </li>
    <li class="danger">
      <button role="menuitem" onclick={() => act((c) => change(() => editor.deleteCapture(c.id)))}>{t("gallery.delete")}</button>
    </li>
  </ul>
{/if}

<style>
  .menu {
    position: fixed;
    z-index: 60;
    width: max-content;
    min-width: 160px;
    list-style: none;
    margin: 0;
    padding: 4px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.2);
    visibility: hidden;
  }
  .menu.ready {
    visibility: visible;
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
  .menu button:hover,
  .menu button:focus-visible {
    background: var(--hover);
    outline: none;
  }
  .danger button {
    color: var(--accent);
  }
</style>
