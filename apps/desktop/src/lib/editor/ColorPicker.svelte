<script lang="ts">
  // Farbe als kleiner Knopf mit Farbpunkt; aufgeklappt Farbfelder, eigene
  // Farbe und auf Wunsch „ohne“.

  import { t } from "$lib/i18n";
  import Dropdown from "./Dropdown.svelte";
  import { fromHex, sameColor, toCss, toHex, type Color } from "./model";

  let {
    value,
    colors,
    title,
    noneLabel,
    onpick,
  }: {
    value: Color | null;
    colors: Color[];
    title: string;
    /** Gesetzt = „ohne“ wählbar (mit diesem Text). */
    noneLabel?: string;
    onpick: (c: Color | null) => void;
  } = $props();
</script>

<Dropdown variant="outline" {title}>
  {#snippet label()}<span class="dot" class:none={!value} style:background={value ? toCss(value) : null}></span> ▾{/snippet}
  {#snippet children(close)}
    <div class="grid" role="radiogroup" aria-label={title}>
      {#if noneLabel}
        <button
          class="swatch none"
          class:on={!value}
          title={noneLabel}
          aria-label={noneLabel}
          onclick={() => {
            onpick(null);
            close();
          }}
        ></button>
      {/if}
      {#each colors as c (toHex(c) + c.a)}
        <button
          class="swatch"
          class:on={!!value && sameColor(c, value)}
          style:background={toCss(c)}
          aria-label={toHex(c)}
          onclick={() => {
            onpick(c);
            close();
          }}
        ></button>
      {/each}
    </div>
    <label class="custom">
      {t("props.custom_color")}
      <input
        type="color"
        value={toHex(value ?? colors[0])}
        onchange={(e) => onpick(fromHex(e.currentTarget.value, value?.a ?? 255))}
      />
    </label>
  {/snippet}
</Dropdown>

<style>
  .dot {
    width: 16px;
    height: 16px;
    border-radius: 50%;
    border: 1px solid rgb(0 0 0 / 0.25);
  }
  .grid {
    display: grid;
    grid-template-columns: repeat(5, 26px);
    gap: 8px;
    padding: 4px;
  }
  .swatch {
    width: 26px;
    height: 26px;
    border-radius: 50%;
    border: 1px solid rgb(0 0 0 / 0.2);
    padding: 0;
    cursor: pointer;
  }
  .swatch.on {
    outline: 2px solid var(--selection);
    outline-offset: 2px;
  }
  .none,
  .dot.none {
    background: linear-gradient(135deg, transparent 45%, #e53935 45%, #e53935 55%, transparent 55%), var(--surface);
  }
  .custom {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    margin-top: 8px;
    padding: 6px 4px 0;
    border-top: 1px solid var(--border);
    color: var(--text-muted);
  }
  input[type="color"] {
    width: 32px;
    height: 24px;
    padding: 0;
    border: none;
    background: none;
  }
</style>
