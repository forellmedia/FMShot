<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import { t } from "$lib/i18n";
  import Canvas from "./Canvas.svelte";
  import Dropdown from "./Dropdown.svelte";
  import Gallery from "./Gallery.svelte";
  import { editor } from "./state.svelte";
  import GalleryStrip from "./GalleryStrip.svelte";
  import Icon from "./Icon.svelte";
  import LayersPanel from "./LayersPanel.svelte";
  import { LAYER_MARKER } from "./clipboard";
  import PropertiesBar from "./PropertiesBar.svelte";
  import Settings from "./Settings.svelte";
  import Toolbar from "./Toolbar.svelte";
  import { toolByKey } from "./tools";

  onMount(() => {
    editor.init();
    // Zum Testen im Browser: Zustand von aussen erreichbar.
    if (import.meta.env.DEV) (window as unknown as { __fmshot: typeof editor }).__fmshot = editor;
  });

  function isTyping(e: KeyboardEvent) {
    const el = e.target as HTMLElement | null;
    return !!el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable);
  }

  function onKeyDown(e: KeyboardEvent) {
    if (isTyping(e) || editor.settingsOpen || editor.galleryOpen) return;
    const modKey = e.metaKey || e.ctrlKey;
    if (modKey && e.key === ",") {
      e.preventDefault();
      editor.settingsOpen = true;
      return;
    }
    if (!editor.doc) return;
    const mod = e.metaKey || e.ctrlKey;
    const key = e.key.toLowerCase();
    if (mod) {
      const actions: Record<string, () => void> = {
        z: () => (e.shiftKey ? editor.redo() : editor.undo()),
        y: () => editor.redo(),
        c: () => {
          // Mit Auswahl: Ebene kopieren, sonst das fertige Bild.
          if (editor.copyLayer()) api.copyText(LAYER_MARKER);
          else editor.copy();
        },
        d: () => editor.duplicate(),
        a: () => editor.selectAll(),
        g: () => (e.shiftKey ? editor.ungroupSelection() : editor.groupSelection()),
        s: () => editor.save(),
        "0": () => editor.fit(),
        "1": () => editor.actualSize(),
        "=": () => editor.zoomAt(1.25),
        "+": () => editor.zoomAt(1.25),
        "-": () => editor.zoomAt(0.8),
      };
      const action = actions[key];
      if (action) {
        e.preventDefault();
        action();
      }
      return;
    }
    if (key === "backspace" || key === "delete") {
      e.preventDefault();
      editor.deleteSelected();
      return;
    }
    if (key === "escape") {
      editor.selectedIds = [];
      return;
    }
    const arrows: Record<string, [number, number]> = {
      arrowleft: [-1, 0],
      arrowright: [1, 0],
      arrowup: [0, -1],
      arrowdown: [0, 1],
    };
    if (arrows[key] && editor.editableSelection.length) {
      e.preventDefault();
      const step = (e.shiftKey ? 10 : 1) * editor.ratio;
      const [dx, dy] = arrows[key];
      editor.nudgeSelection(dx * step, dy * step);
      return;
    }
    const tool = toolByKey(key);
    if (tool) {
      editor.setTool(tool.id);
    }
  }

  const zoomPercent = $derived(Math.round(editor.zoom * editor.ratio * 100));

  // Kopfleiste passt sich der Fensterbreite an, statt über den Rand zu laufen:
  // - schmal: Nebensachen ins „Mehr“-Menü, PNG/JPG unter „Sichern“
  // - Eigenschaften passen nicht zwischen die Knöpfe: eigene Zeile darunter
  let headerWidth = $state(0);
  let captureWidth = $state(0);
  let actionsWidth = $state(0);
  let propsWidth = $state(0);
  const COMPACT_BELOW = 900;
  const compact = $derived(headerWidth > 0 && headerWidth < COMPACT_BELOW);
  // Innenabstand (2 × 12) und Spaltenabstände (2 × 16) der Kopfleiste.
  const twoRows = $derived(captureWidth + actionsWidth + propsWidth + 2 * 12 + 2 * 16 > headerWidth);
</script>

<svelte:window onkeydown={onKeyDown} />

<div class="editor">
  <header class:two-rows={twoRows} bind:clientWidth={headerWidth}>
    <div class="capture" bind:clientWidth={captureWidth}>
      <button title="{t('gallery.title')} (⌥⌘G)" aria-label={t("gallery.title")} onclick={() => (editor.galleryOpen = true)}>
        <Icon d="M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z" />
      </button>
      <span class="sep"></span>
      <button title={t("capture.region")} onclick={() => api.startCapture("region")}>
        <Icon d="M4 8V4h4M16 4h4v4M20 16v4h-4M8 20H4v-4" />
      </button>
      <button title={t("capture.window")} onclick={() => api.startCapture("window")}>
        <Icon d="M3 5h18v14H3zM3 9h18" />
      </button>
      <button title={t("capture.screen")} onclick={() => api.startCapture("screen")}>
        <Icon d="M3 4h18v12H3zM8 20h8M12 16v4" />
      </button>
      <button title={t("capture.scroll")} onclick={() => api.startCapture("scroll")}>
        <Icon d="M6 3h12v9H6zM12 15v6M9 18l3 3 3-3" />
      </button>
      <button title={t("capture.text")} onclick={() => api.startCapture("text")}>
        <Icon d="M4 8V4h4M16 4h4v4M20 16v4h-4M8 20H4v-4M9 9h6M12 9v6" />
      </button>
    </div>
    <PropertiesBar bind:contentWidth={propsWidth} />
    <div class="actions" bind:clientWidth={actionsWidth}>
      <button title="{t('editor.undo')} (⌘Z)" disabled={!editor.canUndo} onclick={() => editor.undo()}>
        <Icon d="M9 14L4 9l5-5M4 9h11a5 5 0 010 10h-3" />
      </button>
      <button title="{t('editor.redo')} (⇧⌘Z)" disabled={!editor.canRedo} onclick={() => editor.redo()}>
        <Icon d="M15 14l5-5-5-5M20 9H9a5 5 0 000 10h3" />
      </button>
      <span class="sep"></span>
      <button title={t("editor.zoom_out")} onclick={() => editor.zoomAt(0.8)}><Icon d="M5 12h14" /></button>
      <button class="zoom" title={t("editor.zoom_fit")} onclick={() => editor.fit()}>{zoomPercent}%</button>
      <button title={t("editor.zoom_in")} onclick={() => editor.zoomAt(1.25)}><Icon d="M5 12h14M12 5v14" /></button>
      <span class="sep"></span>
      {#if !compact}
        <button
          title={t("editor.text")}
          class:on={!!editor.ocrBlocks}
          disabled={!editor.doc || editor.busy}
          onclick={() => editor.showText()}
        >
          <Icon d="M4 7V5h16v2M12 5v14M9 19h6" />
        </button>
        <button title={t("editor.redact")} disabled={!editor.doc || editor.busy} onclick={() => editor.autoRedact()}>
          <Icon d="M4 6h16M4 12h10M4 18h7M17 13l4 4m0-4l-4 4" />
        </button>
        <span class="sep"></span>
      {/if}
      <button title="{t('editor.copy')} (⌘C)" aria-label={t("editor.copy")} onclick={() => editor.copy()} disabled={!editor.doc}>
        <Icon d="M9 9h10v10H9zM5 15V5h10" />
      </button>
      <button title="{t('editor.save')} (⌘S)" aria-label={t("editor.save")} onclick={() => editor.save()} disabled={!editor.doc}>
        <Icon d="M12 4v11M7 10l5 5 5-5M5 19h14" />
      </button>
      {#if compact}
        <Dropdown align="right" title={t("editor.more")}>
          {#snippet label()}<Icon d="M5 12h.5M12 12h.5M19 12h.5" />{/snippet}
          {#snippet children(close)}
            <button
              class="item"
              disabled={!editor.doc || editor.busy}
              onclick={() => {
                editor.showText();
                close();
              }}
            >
              {t("editor.text")}
            </button>
            <button
              class="item"
              disabled={!editor.doc || editor.busy}
              onclick={() => {
                editor.autoRedact();
                close();
              }}
            >
              {t("editor.redact")}
            </button>
            <button
              class="item"
              onclick={() => {
                editor.layersPanel = !editor.layersPanel;
                close();
              }}
            >
              {t("layers.title")}
              {#if editor.layersPanel}<span>✓</span>{/if}
            </button>
            <button
              class="item"
              onclick={() => {
                editor.settingsOpen = true;
                close();
              }}
            >
              {t("editor.settings")} <kbd>⌘,</kbd>
            </button>
          {/snippet}
        </Dropdown>
      {:else}
        <button
          title={t("layers.title")}
          class:on={editor.layersPanel}
          onclick={() => (editor.layersPanel = !editor.layersPanel)}
        >
          <Icon d="M12 3l9 5-9 5-9-5zM3 13l9 5 9-5" />
        </button>
        <button title={t("editor.settings")} onclick={() => (editor.settingsOpen = true)}>
          <Icon d="M12 9a3 3 0 110 6 3 3 0 010-6zM19 12l2-1-1-3-2 .5-1.5-1.5.5-2-3-1-1 2h-2l-1-2-3 1 .5 2L6 8.5 4 8l-1 3 2 1v0l-2 1 1 3 2-.5L7.5 17l-.5 2 3 1 1-2h2l1 2 3-1-.5-2 1.5-1.5 2 .5 1-3z" />
        </button>
      {/if}
    </div>
  </header>

  <main>
    <Toolbar />
    {#if editor.doc}
      <Canvas />
    {:else}
      <div class="start">
        <h1>FMShot</h1>
        <p>{t("start.hint")}</p>
        <div class="start-buttons">
          <button onclick={() => api.startCapture("region")}>{t("capture.region")} <kbd>⇧⌘2</kbd></button>
          <button onclick={() => api.startCapture("window")}>{t("capture.window")} <kbd>⌥⌘3</kbd></button>
          <button onclick={() => api.startCapture("screen")}>{t("capture.screen")} <kbd>⇧⌘1</kbd></button>
          <button onclick={() => api.startCapture("scroll")}>{t("capture.scroll")} <kbd>⌥⌘4</kbd></button>
        </div>
      </div>
    {/if}
    {#if editor.layersPanel && editor.doc}
      <LayersPanel />
    {/if}
  </main>

  <GalleryStrip />

  {#if editor.galleryOpen}
    <Gallery />
  {/if}

  {#if editor.settingsOpen}
    <Settings />
  {/if}

  {#if editor.message}
    <div class="toast" role="status">{editor.message}</div>
  {/if}
</div>

<style>
  .editor {
    display: grid;
    /* minmax(0, …): Inhalt darf die Spalte nicht über das Fenster hinaus weiten. */
    grid-template-columns: minmax(0, 1fr);
    grid-template-rows: auto 1fr auto;
    height: 100vh;
    overflow: hidden;
  }
  header {
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    grid-template-areas: "capture props actions";
    align-items: center;
    column-gap: var(--space-4);
    row-gap: 6px;
    padding: 6px var(--space-3);
    background: var(--surface);
    border-bottom: 1px solid var(--border);
    min-height: 44px;
  }
  header.two-rows {
    grid-template-areas:
      "capture . actions"
      "props props props";
  }
  .capture {
    grid-area: capture;
  }
  .actions {
    grid-area: actions;
  }
  header :global(.props) {
    grid-area: props;
  }
  .capture,
  .actions {
    display: flex;
    align-items: center;
    gap: 2px;
  }
  header button {
    display: grid;
    place-items: center;
    min-width: 30px;
    height: 30px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--text);
    font: inherit;
    cursor: pointer;
  }
  header button:hover:not(:disabled) {
    background: var(--hover);
  }
  header button:disabled {
    opacity: 0.35;
    cursor: default;
  }
  header button.zoom {
    min-width: 52px;
    font-variant-numeric: tabular-nums;
  }
  header button.on {
    background: var(--hover);
  }
  .sep {
    width: 1px;
    height: 20px;
    background: var(--border);
    margin: 0 6px;
  }
  main {
    display: flex;
    min-height: 0;
  }
  main > :global(.canvas) {
    flex: 1;
  }
  .start {
    flex: 1;
    display: grid;
    place-content: center;
    text-align: center;
    gap: var(--space-3);
    background: var(--canvas-bg);
  }
  .start h1 {
    margin: 0;
    font-size: 32px;
  }
  .start p {
    margin: 0;
    color: var(--text-muted);
  }
  .start-buttons {
    display: flex;
    gap: var(--space-3);
    justify-content: center;
    margin-top: var(--space-3);
  }
  .start-buttons button {
    padding: 10px 16px;
    border-radius: 8px;
    border: 1px solid var(--border);
    background: var(--surface);
    color: var(--text);
    font: inherit;
    cursor: pointer;
  }
  kbd {
    margin-left: 6px;
    color: var(--text-muted);
    font-family: inherit;
  }
  .toast {
    position: fixed;
    left: 50%;
    bottom: 112px;
    transform: translateX(-50%);
    padding: 8px 16px;
    border-radius: 8px;
    background: rgb(30 30 30 / 0.92);
    color: #fff;
    box-shadow: 0 4px 16px rgb(0 0 0 / 0.3);
    z-index: 30;
    max-width: 80vw;
  }
</style>
