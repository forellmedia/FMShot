<script lang="ts">
  // Fortschritt beim Zusammensetzen eines Videos oder GIFs, über dem Video.
  // Erscheint nur auf Wunsch (Kopieren, Sichern, GIF, Einzelbild).

  import { t } from "$lib/i18n";
  import { player } from "./video.svelte";

  const percent = $derived(Math.round((player.exporting ?? 0) * 100));
  const label = $derived(
    t(player.exportKind === "gif" ? "video.exporting_gif" : "video.exporting", { percent: String(percent) }),
  );

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      player.cancelExport();
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="backdrop">
  <div class="card" role="status" aria-live="polite">
    <span>{label}</span>
    <div class="bar" role="progressbar" aria-valuemin="0" aria-valuemax="100" aria-valuenow={percent}>
      <div class="fill" style:width="{percent}%"></div>
    </div>
    <button onclick={() => player.cancelExport()}>{t("video.cancel")} <kbd>Esc</kbd></button>
  </div>
</div>

<style>
  .backdrop {
    position: absolute;
    inset: 0;
    z-index: 15;
    display: grid;
    place-items: center;
    background: rgb(0 0 0 / 0.35);
  }
  .card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-3);
    width: min(320px, calc(100% - 32px));
    padding: var(--space-4);
    border: 1px solid var(--border);
    border-radius: 12px;
    background: var(--surface);
    box-shadow: 0 12px 40px rgb(0 0 0 / 0.25);
    font-variant-numeric: tabular-nums;
  }
  .bar {
    width: 100%;
    height: 6px;
    border-radius: 3px;
    background: var(--hover);
    overflow: hidden;
  }
  .fill {
    height: 100%;
    background: var(--accent);
    transition: width 0.2s;
  }
  button {
    padding: 5px 12px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: transparent;
    color: var(--text);
    font: inherit;
    cursor: pointer;
  }
  button:hover {
    background: var(--hover);
  }
  kbd {
    margin-left: 4px;
    color: var(--text-muted);
    font-family: inherit;
    font-size: 12px;
  }
</style>
