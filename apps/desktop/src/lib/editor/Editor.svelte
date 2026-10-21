<script lang="ts">
  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import { t } from "$lib/i18n";
  import { currentPlatform, formatShortcut, withShortcut } from "$lib/shortcuts";
  import Canvas from "./Canvas.svelte";
  import CaptureMenu from "./CaptureMenu.svelte";
  import Dropdown from "./Dropdown.svelte";
  import ExportProgress from "./ExportProgress.svelte";
  import Gallery from "./Gallery.svelte";
  import { editor } from "./state.svelte";
  import GalleryStrip from "./GalleryStrip.svelte";
  import Icon from "./Icon.svelte";
  import LayersPanel from "./LayersPanel.svelte";
  import PropertiesBar from "./PropertiesBar.svelte";
  import SaveAs from "./SaveAs.svelte";
  import Toolbar from "./Toolbar.svelte";
  import Timeline from "./Timeline.svelte";
  import { player } from "./video.svelte";
  import { toolByKey } from "./tools";

  // Video geöffnet: Spuren laden; anderes Bild: Wiedergabe beenden.
  $effect(() => {
    const cap = editor.current;
    if (cap?.videoPath) void player.load(cap);
    else player.unload();
  });

  onMount(() => {
    editor.init();
    // Zum Testen im Browser: Zustand von aussen erreichbar.
    if (import.meta.env.DEV) (window as unknown as { __grabmark: typeof editor }).__grabmark = editor;
  });

  /** Text mit globalem Kürzel, sofern belegt. */
  function withHotkey(label: string, action: string) {
    const key = editor.hotkey(action);
    return key ? `${label} (${key})` : label;
  }

  function isTyping(e: KeyboardEvent) {
    const el = e.target as HTMLElement | null;
    return !!el && (el.tagName === "INPUT" || el.tagName === "TEXTAREA" || el.isContentEditable);
  }

  /**
   * Offene Aufnahme löschen wie im Finder: ⌘⌫ auf macOS, Entf sonst. Entf
   * löscht zuerst gewählte Ebenen; die Zeitleiste kann es auch für sich
   * beanspruchen (Klick löschen) – darum erst nach allen Tasten-Handlern
   * entscheiden. Gibt zurück, ob die Taste damit erledigt ist.
   */
  function deleteCaptureKey(e: KeyboardEvent, key: string): boolean {
    const id = editor.current?.id;
    if (!id || editor.editingTextId != null || e.altKey || e.shiftKey) return false;
    if (currentPlatform() === "mac") {
      if (!(e.metaKey && key === "backspace")) return false;
      e.preventDefault();
      void editor.deleteCapture(id);
      return true;
    }
    if (key !== "delete" || e.ctrlKey || e.metaKey || editor.editableSelection.length) return false;
    setTimeout(() => {
      if (!e.defaultPrevented && editor.current?.id === id) void editor.deleteCapture(id);
    });
    return true;
  }

  function onKeyDown(e: KeyboardEvent) {
    if (isTyping(e) || editor.galleryOpen || editor.saveAsOpen || player.exporting != null) return;
    const modKey = e.metaKey || e.ctrlKey;
    if (modKey && e.key === ",") {
      e.preventDefault();
      void api.openMain("settings");
      return;
    }
    if (!editor.doc) return;
    const mod = e.metaKey || e.ctrlKey;
    const key = e.key.toLowerCase();
    if (deleteCaptureKey(e, key)) return;
    if (mod) {
      const actions: Record<string, () => void> = {
        z: () => (e.shiftKey ? editor.redo() : editor.undo()),
        y: () => editor.redo(),
        c: () => void editor.copyShortcut(),
        d: () => editor.duplicate(),
        a: () => editor.selectAll(),
        g: () => (e.shiftKey ? editor.ungroupSelection() : editor.groupSelection()),
        s: () => editor.saveShortcut(e.shiftKey),
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
  // Innenabstand und Spaltenabstände (2 × 12) der Kopfleiste; mit Glas liegen
  // links die Fensterknöpfe von macOS darüber.
  const headerPadding = document.documentElement.classList.contains("glass") ? 84 + 8 : 2 * 8;
  const twoRows = $derived(captureWidth + actionsWidth + propsWidth + headerPadding + 2 * 12 > headerWidth);
</script>

<svelte:window onkeydown={onKeyDown} />

<div class="editor">
  <header class:two-rows={twoRows} bind:clientWidth={headerWidth} data-tauri-drag-region>
    <div class="capture" bind:clientWidth={captureWidth}>
      <button title={withHotkey(t("gallery.title"), "open_gallery")} aria-label={t("gallery.title")} onclick={() => (editor.galleryOpen = true)}>
        <Icon name="gallery" />
      </button>
      <span class="sep"></span>
      <button title={t("capture.region")} onclick={() => api.startCapture("region")}>
        <Icon name="region" />
      </button>
      <button title={t("capture.window")} onclick={() => api.startCapture("window")}>
        <Icon name="window" />
      </button>
      <button title={t("capture.screen")} onclick={() => api.startCapture("screen")}>
        <Icon name="screen" />
      </button>
      <button title={t("capture.scroll")} onclick={() => api.startCapture("scroll")}>
        <Icon name="scroll" />
      </button>
      <button title={t("capture.text")} onclick={() => api.startCapture("text")}>
        <Icon name="ocr" />
      </button>
    </div>
    <PropertiesBar bind:contentWidth={propsWidth} />
    <div class="actions" bind:clientWidth={actionsWidth}>
      <button title={withShortcut(t("editor.undo"), "Primary+Z")} disabled={!editor.canUndo} onclick={() => editor.undo()}>
        <Icon name="undo" />
      </button>
      <button title={withShortcut(t("editor.redo"), "Primary+Shift+Z")} disabled={!editor.canRedo} onclick={() => editor.redo()}>
        <Icon name="redo" />
      </button>
      <span class="sep"></span>
      <button title={t("editor.zoom_out")} onclick={() => editor.zoomAt(0.8)}><Icon name="zoomOut" /></button>
      <button class="zoom" title={withShortcut(t("editor.zoom_fit"), "Primary+0")} onclick={() => editor.fit()}>{zoomPercent}%</button>
      <button title={t("editor.zoom_in")} onclick={() => editor.zoomAt(1.25)}><Icon name="zoomIn" /></button>
      <span class="sep"></span>
      {#if !compact}
        <button
          title={t("editor.text")}
          class:on={!!editor.ocrBlocks}
          disabled={!editor.doc || editor.busy}
          onclick={() => editor.showText()}
        >
          <Icon name="ocr" />
        </button>
        <button title={t("editor.redact")} disabled={!editor.doc || editor.busy} onclick={() => editor.autoRedact()}>
          <Icon name="redact" />
        </button>
        <span class="sep"></span>
      {/if}
      <button title={withShortcut(t("editor.copy"), "Primary+C")} aria-label={t("editor.copy")} onclick={() => editor.copy()} disabled={!editor.doc}>
        <Icon name="copy" />
      </button>
      <button title={withShortcut(t("editor.save"), "Primary+S")} aria-label={t("editor.save")} onclick={() => editor.save()} disabled={!editor.doc}>
        <Icon name="save" />
      </button>
      {#if compact}
        <Dropdown align="right" title={t("editor.more")}>
          {#snippet label()}<Icon name="more" />{/snippet}
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
                void api.openMain("settings");
                close();
              }}
            >
              {t("editor.settings")} <kbd>{formatShortcut("Primary+,")}</kbd>
            </button>
          {/snippet}
        </Dropdown>
      {:else}
        <button
          title={t("layers.title")}
          class:on={editor.layersPanel}
          onclick={() => (editor.layersPanel = !editor.layersPanel)}
        >
          <Icon name="layers" />
        </button>
        <button title={t("editor.settings")} onclick={() => api.openMain("settings")}>
          <Icon name="settings" />
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
          <h1>Grabmark</h1>
          <p>{t("start.hint")}</p>
          <div class="start-buttons">
            <button onclick={() => api.startCapture("region")}>{t("capture.region")} <kbd>{editor.hotkey("capture_region")}</kbd></button>
            <button onclick={() => api.startCapture("window")}>{t("capture.window")} <kbd>{editor.hotkey("capture_window")}</kbd></button>
            <button onclick={() => api.startCapture("screen")}>{t("capture.screen")} <kbd>{editor.hotkey("capture_screen")}</kbd></button>
            <button onclick={() => api.startCapture("scroll")}>{t("capture.scroll")} <kbd>{editor.hotkey("capture_scroll")}</kbd></button>
          </div>
        </div>
    {/if}
    {#if editor.layersPanel && editor.doc}
      <LayersPanel />
    {/if}
    {#if player.exporting != null && player.exporting >= 0}
      <ExportProgress />
    {/if}
  </main>
  {#if editor.current?.videoPath && editor.doc}
    <Timeline />
  {/if}

  <GalleryStrip />
  <CaptureMenu />

  {#if editor.galleryOpen}
    <Gallery />
  {/if}

  {#if editor.saveAsOpen}
    <SaveAs />
  {/if}

  {#if editor.message}
    <div class="toast" role="status">{editor.message}</div>
  {/if}
</div>

<style>
  /* Glas: Leinwand und Bildliste liegen als Flächen auf dem durchscheinenden
     Fensterhintergrund, die Leisten schweben als Kapseln darüber. */
  .editor {
    display: grid;
    /* minmax(0, …): Inhalt darf die Spalte nicht über das Fenster hinaus weiten. */
    grid-template-columns: minmax(0, 1fr) 216px;
    grid-template-rows: auto 1fr auto;
    column-gap: var(--space-2);
    row-gap: var(--space-2);
    height: 100vh;
    padding: 0 var(--space-2) var(--space-2);
    box-sizing: border-box;
    overflow: hidden;
  }
  .editor > :global(.side) {
    grid-column: 2;
    grid-row: 1 / 4;
    margin-top: var(--space-2);
  }
  .editor > :global(.timeline) {
    grid-column: 1;
  }
  header {
    grid-column: 1;
    display: grid;
    grid-template-columns: auto minmax(0, 1fr) auto;
    grid-template-areas: "capture props actions";
    align-items: center;
    column-gap: var(--space-3);
    row-gap: 6px;
    padding: var(--space-2) 0 0 var(--space-2);
    min-height: 44px;
  }
  /* Platz für die Fensterknöpfe von macOS (Titelleiste über dem Inhalt). */
  :global(.glass) header {
    padding-left: 84px;
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
    justify-self: center;
    max-width: 100%;
    box-sizing: border-box;
    padding: 3px 12px;
  }
  .capture,
  .actions,
  header :global(.props) {
    display: flex;
    align-items: center;
    gap: 2px;
    padding: 3px;
    border-radius: 999px;
    background: var(--glass);
    border: 1px solid var(--glass-edge);
    box-shadow: var(--glass-shadow);
    backdrop-filter: blur(24px) saturate(180%);
    -webkit-backdrop-filter: blur(24px) saturate(180%);
  }
  header button {
    display: grid;
    place-items: center;
    min-width: 32px;
    height: 32px;
    border: none;
    border-radius: 999px;
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
    grid-column: 1;
    position: relative;
    display: flex;
    gap: var(--space-2);
    min-height: 0;
  }
  main > :global(.canvas),
  .start {
    flex: 1;
    border-radius: var(--radius-panel);
    overflow: hidden;
  }
  .start {
    display: grid;
    place-content: center;
    text-align: center;
    gap: var(--space-3);
    background: var(--glass);
    border: 1px solid var(--glass-edge);
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
    border-radius: 999px;
    border: 1px solid var(--glass-edge);
    background: var(--glass-strong);
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
    bottom: 32px;
    transform: translateX(-50%);
    padding: 8px 16px;
    border-radius: 999px;
    background: rgb(30 30 30 / 0.92);
    color: #fff;
    box-shadow: 0 4px 16px rgb(0 0 0 / 0.3);
    z-index: 30;
    max-width: 80vw;
  }
</style>
