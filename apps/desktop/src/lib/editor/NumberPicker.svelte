<script lang="ts">
  // Zahlenwert als kleiner Knopf „Stärke 3 ▾“; aufgeklappt mit − / + und Regler.
  // Spart Platz in der Eigenschaftsleiste gegenüber einem offenen Regler.

  import Dropdown from "./Dropdown.svelte";

  let {
    name,
    value,
    min,
    max,
    step = 1,
    unit = "",
    onchange,
  }: {
    name: string;
    value: number;
    min: number;
    max: number;
    step?: number;
    unit?: string;
    onchange: (v: number) => void;
  } = $props();

  const shown = $derived(Math.round(value));

  function set(v: number) {
    onchange(Math.min(max, Math.max(min, Math.round(v / step) * step)));
  }
</script>

<Dropdown variant="outline" title={name}>
  {#snippet label()}<span class="name">{name}</span> <b>{shown}{unit}</b> ▾{/snippet}
  {#snippet children()}
    <div class="picker">
      <button class="step" aria-label="−" disabled={value <= min} onclick={() => set(value - step)}>−</button>
      <span class="value">{shown}{unit}</span>
      <button class="step" aria-label="+" disabled={value >= max} onclick={() => set(value + step)}>+</button>
    </div>
    <input
      type="range"
      aria-label={name}
      {min}
      {max}
      {step}
      {value}
      oninput={(e) => set(+e.currentTarget.value)}
    />
  {/snippet}
</Dropdown>

<style>
  .name {
    color: var(--text-muted);
  }
  b {
    font-weight: 600;
    font-variant-numeric: tabular-nums;
  }
  .picker {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-3);
    margin-bottom: 6px;
  }
  .step {
    width: 32px;
    height: 28px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: transparent;
    color: var(--text);
    font: inherit;
    font-size: 16px;
    cursor: pointer;
  }
  .step:hover:not(:disabled) {
    background: var(--hover);
  }
  .step:disabled {
    opacity: 0.35;
    cursor: default;
  }
  .value {
    font-variant-numeric: tabular-nums;
    font-weight: 600;
  }
  input[type="range"] {
    width: 100%;
  }
</style>
