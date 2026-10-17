<script lang="ts" generics="T extends string | number | null">
  // Auswahl aus wenigen Möglichkeiten als kleiner Knopf mit Symbol der
  // aktuellen Wahl; aufgeklappt alle Möglichkeiten mit Symbol und Namen.

  import type { Snippet } from "svelte";
  import Dropdown from "./Dropdown.svelte";

  let {
    value,
    options,
    title,
    icon,
    onpick,
  }: {
    value: T;
    options: { value: T; label: string }[];
    title: string;
    /** Symbol einer Möglichkeit (im Knopf und in der Liste). */
    icon: Snippet<[T]>;
    onpick: (v: T) => void;
  } = $props();
</script>

<Dropdown variant="outline" {title}>
  {#snippet label()}{@render icon(value)} ▾{/snippet}
  {#snippet children(close)}
    {#each options as o (o.value)}
      <button
        class="item"
        role="menuitemradio"
        aria-checked={o.value === value}
        onclick={() => {
          onpick(o.value);
          close();
        }}
      >
        <span class="choice">{@render icon(o.value)} {o.label}</span>
        {#if o.value === value}<span>✓</span>{/if}
      </button>
    {/each}
  {/snippet}
</Dropdown>

<style>
  .choice {
    display: flex;
    align-items: center;
    gap: 10px;
  }
</style>
