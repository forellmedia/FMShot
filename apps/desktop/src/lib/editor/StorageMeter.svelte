<script lang="ts">
  // Belegter Speicher unten in der Bildliste und in der Galerie: Bibliothek
  // und Auto-Export mit ihrer Grenze. Ab 80 % gelb, darüber rot. Ein Klick
  // öffnet „Speicher aufräumen“.

  import { onMount } from "svelte";
  import { api, on, type StorageSummary } from "$lib/api";
  import { formatBytes, share } from "$lib/bytes";
  import { locale, t } from "$lib/i18n";
  import { editor } from "./state.svelte";

  let { compact = false }: { compact?: boolean } = $props();

  let summary = $state<StorageSummary | null>(null);

  async function refresh() {
    try {
      summary = await api.storageSummary();
    } catch (e) {
      console.warn(`Speicher: ${e}`);
    }
  }

  onMount(() => {
    void refresh();
    const stops: (() => void)[] = [];
    for (const event of ["library-changed", "capture-saved"]) {
      void on(event, () => refresh()).then((f) => stops.push(f));
    }
    // Auch der Auto-Export wächst, ohne dass sich die Bibliothek ändert.
    const timer = setInterval(refresh, 5 * 60_000);
    return () => {
      clearInterval(timer);
      for (const stop of stops) stop();
    };
  });

  // Nach dem Aufräumen neu messen.
  $effect(() => {
    if (!editor.cleanupOpen) void refresh();
  });

  type Row = { key: "library" | "export"; label: string; bytes: number; limit: number | null };
  const rows = $derived.by((): Row[] => {
    if (!summary) return [];
    const out: Row[] = [
      { key: "library", label: t("storage.library"), bytes: summary.libraryBytes ?? 0, limit: summary.libraryLimit },
    ];
    // Ohne exportierte Dateien gibt es nichts zu zeigen.
    if ((summary.exportBytes ?? 0) > 0) {
      out.push({ key: "export", label: t("storage.export"), bytes: summary.exportBytes ?? 0, limit: summary.exportLimit });
    }
    return out;
  });

  const level = (r: Row) => {
    const s = share(r.bytes, r.limit);
    return s > 1 ? "over" : s >= 0.8 ? "near" : "ok";
  };
  const worst = $derived(rows.some((r) => level(r) === "over") ? "over" : rows.some((r) => level(r) === "near") ? "near" : "ok");
</script>

{#if summary}
  <button
    class="meter {worst}"
    class:compact
    title={t("storage.meter_hint")}
    onclick={() => (editor.cleanupOpen = true)}
  >
    {#each rows as r (r.key)}
      <span class="row {level(r)}">
        <span class="label">{r.label}</span>
        <span class="value">
          {formatBytes(r.bytes, locale())}{#if r.limit}<span class="muted"> / {formatBytes(r.limit, locale())}</span>{/if}
        </span>
        {#if r.limit}
          <span class="bar"><span style:width="{Math.min(100, share(r.bytes, r.limit) * 100)}%"></span></span>
        {/if}
      </span>
    {/each}
    {#if worst === "over"}
      <span class="action">{t("storage.clean_up")} →</span>
    {/if}
  </button>
{/if}

<style>
  .meter {
    flex: none;
    display: flex;
    flex-direction: column;
    gap: 4px;
    width: 100%;
    padding: 6px 8px;
    border: 1px solid transparent;
    border-radius: 8px;
    background: transparent;
    color: var(--text-muted);
    font: inherit;
    font-size: 11px;
    text-align: left;
    cursor: pointer;
  }
  .meter:hover {
    background: var(--hover);
  }
  .meter.over {
    border-color: color-mix(in srgb, #e53935 45%, transparent);
    background: color-mix(in srgb, #e53935 8%, transparent);
  }
  .meter:not(.compact) {
    flex-direction: row;
    flex-wrap: wrap;
    align-items: center;
    gap: 4px 16px;
    width: auto;
  }
  .row {
    display: grid;
    grid-template-columns: auto 1fr;
    align-items: baseline;
    gap: 2px 8px;
  }
  .meter:not(.compact) .row {
    grid-template-columns: auto auto 80px;
    align-items: center;
  }
  .value {
    justify-self: end;
    color: var(--text);
    font-variant-numeric: tabular-nums;
  }
  .muted {
    color: var(--text-muted);
  }
  .bar {
    grid-column: 1 / -1;
    height: 3px;
    border-radius: 2px;
    background: color-mix(in srgb, var(--text) 12%, transparent);
    overflow: hidden;
  }
  .meter:not(.compact) .bar {
    grid-column: auto;
  }
  .bar span {
    display: block;
    height: 100%;
    background: var(--text-muted);
  }
  .row.near .bar span {
    background: #f9a825;
  }
  .row.over .bar span {
    background: #e53935;
  }
  .row.over .value {
    color: #e53935;
    font-weight: 600;
  }
  .action {
    color: #e53935;
    font-weight: 600;
  }
</style>
