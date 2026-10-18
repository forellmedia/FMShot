<script lang="ts">
  // Ein Video aus der Bibliothek abspielen. Bearbeiten (Schnitt, Webcam,
  // Zoom) folgt mit dem Video-Editor (ADR 0011, Etappe 4).

  import { api, fileUrl, type CaptureInfo } from "$lib/api";
  import { t } from "$lib/i18n";
  import { editor } from "./state.svelte";

  let { capture }: { capture: CaptureInfo } = $props();

  let src = $state("");
  let poster = $state("");

  $effect(() => {
    const path = capture.videoPath;
    if (path) fileUrl(path).then((u) => (src = u));
    fileUrl(capture.originalPath).then((u) => (poster = u));
  });

  async function copy() {
    try {
      await api.copyVideo(capture.id);
      editor.flash(t("video.copied"));
    } catch (e) {
      editor.flash(String(e));
    }
  }

  async function saveAs() {
    try {
      const path = await api.saveVideoAs(capture.id);
      if (path) editor.flash(t("video.saved", { path }));
    } catch (e) {
      editor.flash(String(e));
    }
  }
</script>

<div class="video">
  {#key src}
    <!-- svelte-ignore a11y_media_has_caption -->
    <video {src} {poster} controls preload="metadata"></video>
  {/key}
  <div class="buttons">
    <button onclick={copy}>{t("video.copy")}</button>
    <button onclick={saveAs}>{t("video.save_as")}</button>
    <button onclick={() => api.revealCapture(capture.id)}>{t("video.reveal")}</button>
  </div>
</div>

<style>
  .video {
    flex: 1;
    min-width: 0;
    display: flex;
    flex-direction: column;
    align-items: center;
    justify-content: center;
    gap: var(--space-4);
    padding: var(--space-6);
    background: var(--canvas-bg);
  }

  video {
    max-width: 100%;
    max-height: calc(100% - 56px);
    border-radius: var(--radius);
    background: #000;
    box-shadow: 0 4px 24px rgb(0 0 0 / 0.25);
  }

  .buttons {
    display: flex;
    gap: var(--space-2);
  }

  button {
    font: inherit;
    font-size: 13px;
    color: var(--text);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 6px;
    padding: 6px 12px;
    cursor: pointer;
  }

  button:hover {
    background: var(--hover);
  }
</style>
