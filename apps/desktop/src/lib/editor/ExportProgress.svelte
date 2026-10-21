<script lang="ts">
  // Fortschritt beim Zusammensetzen eines Videos oder GIFs: gross über dem
  // unscharfen Standbild. Erscheint nur auf Wunsch (Kopieren, Sichern, GIF,
  // Einzelbild) und direkt nach einer Aufnahme, wenn kopiert wird.

  import { t } from "$lib/i18n";
  import { player } from "./video.svelte";

  const percent = $derived(Math.round((player.exporting ?? 0) * 100));
  const label = $derived(t(player.exportKind === "gif" ? "video.creating_gif" : "video.creating"));
  // Ring: Umfang eines Kreises mit Radius 44.
  const CIRCUMFERENCE = 2 * Math.PI * 44;

  // Abbrechen erst nach Rückfrage: „Abbrechen“ bzw. Esc fragt, die Arbeit
  // läuft derweil weiter. Esc in der Frage = weiter erstellen.
  let asking = $state(false);
  let keepButton = $state<HTMLButtonElement>();

  $effect(() => {
    if (asking) keepButton?.focus();
  });

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      e.stopPropagation();
      asking = !asking;
    }
  }

  function cancel() {
    asking = false;
    player.cancelExport();
  }
</script>

<svelte:window onkeydown={onKey} />

<div class="backdrop">
  <div class="card" role="status" aria-live="polite" aria-label="{label} {percent} %">
    <div class="ring" class:waiting={percent === 0}>
      <svg viewBox="0 0 100 100" aria-hidden="true">
        <circle class="track" cx="50" cy="50" r="44" />
        <circle
          class="fill"
          cx="50"
          cy="50"
          r="44"
          stroke-dasharray={CIRCUMFERENCE}
          stroke-dashoffset={CIRCUMFERENCE * (1 - percent / 100)}
        />
      </svg>
      <span class="percent">{percent}<small>%</small></span>
    </div>
    {#if asking}
      <span class="label" role="alert">
        {t(player.exportKind === "gif" ? "video.cancel_confirm_gif" : "video.cancel_confirm")}
      </span>
      <div class="buttons">
        <button bind:this={keepButton} onclick={() => (asking = false)}>
          {t("video.cancel_no")} <kbd>Esc</kbd>
        </button>
        <button class="danger" onclick={cancel}>{t("video.cancel_yes")}</button>
      </div>
    {:else}
      <span class="label">{label}</span>
      <button onclick={() => (asking = true)}>{t("video.cancel")} <kbd>Esc</kbd></button>
    {/if}
  </div>
</div>

<style>
  /* Das Video dahinter (Standbild) unscharf und abgedunkelt. */
  .backdrop {
    position: absolute;
    inset: 0;
    z-index: 15;
    display: grid;
    place-items: center;
    background: rgb(0 0 0 / 0.3);
    backdrop-filter: blur(18px) saturate(0.9);
    -webkit-backdrop-filter: blur(18px) saturate(0.9);
  }
  .card {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: var(--space-3);
    color: #fff;
    text-shadow: 0 1px 8px rgb(0 0 0 / 0.4);
    font-variant-numeric: tabular-nums;
  }
  .ring {
    position: relative;
    width: 168px;
    height: 168px;
  }
  svg {
    width: 100%;
    height: 100%;
    transform: rotate(-90deg);
  }
  circle {
    fill: none;
    stroke-width: 6;
  }
  .track {
    stroke: rgb(255 255 255 / 0.22);
  }
  .fill {
    stroke: #fff;
    stroke-linecap: round;
    transition: stroke-dashoffset 0.3s ease-out;
  }
  /* Vor dem ersten Fortschritt: ein Stück Ring dreht sich. */
  .waiting svg {
    animation: spin 1.1s linear infinite;
  }
  .waiting .fill {
    stroke-dashoffset: 210;
  }
  @keyframes spin {
    from {
      transform: rotate(-90deg);
    }
    to {
      transform: rotate(270deg);
    }
  }
  .percent {
    position: absolute;
    inset: 0;
    display: grid;
    grid-auto-flow: column;
    place-content: center;
    align-items: baseline;
    font-size: 44px;
    font-weight: 600;
    letter-spacing: -0.02em;
  }
  .percent small {
    font-size: 20px;
    font-weight: 500;
    margin-left: 2px;
  }
  .label {
    font-size: 15px;
    font-weight: 500;
  }
  button {
    padding: 6px 14px;
    border: 1px solid rgb(255 255 255 / 0.35);
    border-radius: 8px;
    background: rgb(255 255 255 / 0.12);
    color: #fff;
    font: inherit;
    cursor: pointer;
  }
  button:hover {
    background: rgb(255 255 255 / 0.22);
  }
  button:focus-visible {
    outline: 2px solid #fff;
    outline-offset: 2px;
  }
  .buttons {
    display: flex;
    gap: var(--space-2);
  }
  /* Abbrechen verwirft die bisherige Arbeit: rot hinterlegt. */
  .danger {
    border-color: rgb(255 95 87 / 0.7);
    background: rgb(255 69 58 / 0.55);
    text-shadow: none;
  }
  .danger:hover {
    background: rgb(255 69 58 / 0.75);
  }
  kbd {
    margin-left: 4px;
    opacity: 0.7;
    font-family: inherit;
    font-size: 12px;
  }
  @media (prefers-reduced-motion: reduce) {
    .waiting svg {
      animation: none;
    }
  }
</style>
