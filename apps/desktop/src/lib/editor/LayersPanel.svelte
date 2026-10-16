<script lang="ts">
  import { t, type MessageKey } from "$lib/i18n";
  import { editor } from "./state.svelte";

  // Oberste Ebene zuerst – wie in Grafikprogrammen üblich.
  const layers = $derived([...(editor.doc?.layers ?? [])].reverse());

  function name(type: string): string {
    return t(`layer.${type}` as MessageKey);
  }
</script>

<aside class="panel" aria-label={t("layers.title")}>
  <h2>{t("layers.title")}</h2>
  <ul>
    {#each layers as l (l.id)}
      <li class:selected={editor.selectedId === l.id}>
        <button class="name" onclick={() => (editor.selectedId = l.id)}>
          {l.name ?? name(l.kind.type)}
          {#if l.kind.type === "step"}&nbsp;{l.kind.number}{/if}
        </button>
        <button
          class="icon"
          title={t("layers.visible")}
          class:off={!l.visible}
          onclick={() => editor.updateLayer(l.id, (x) => (x.visible = !x.visible))}>👁</button
        >
        <button
          class="icon"
          title={t("layers.locked")}
          class:off={!l.locked}
          onclick={() => editor.updateLayer(l.id, (x) => (x.locked = !x.locked))}>🔒</button
        >
        <button class="icon" title={t("layers.up")} onclick={() => editor.moveLayer(l.id, 1)}>▲</button>
        <button class="icon" title={t("layers.down")} onclick={() => editor.moveLayer(l.id, -1)}>▼</button>
      </li>
    {/each}
  </ul>
</aside>

<style>
  .panel {
    width: 230px;
    border-left: 1px solid var(--border);
    background: var(--surface);
    overflow-y: auto;
    padding: var(--space-2);
  }
  h2 {
    font-size: 12px;
    text-transform: uppercase;
    letter-spacing: 0.04em;
    color: var(--text-muted);
    margin: var(--space-1) var(--space-2) var(--space-2);
  }
  ul {
    list-style: none;
    margin: 0;
    padding: 0;
  }
  li {
    display: flex;
    align-items: center;
    border-radius: 6px;
  }
  li.selected {
    background: var(--hover);
  }
  button {
    border: none;
    background: none;
    color: var(--text);
    font: inherit;
    cursor: pointer;
  }
  .name {
    flex: 1;
    text-align: left;
    padding: 6px 8px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
  .icon {
    width: 24px;
    font-size: 11px;
    opacity: 0.8;
  }
  .icon.off {
    opacity: 0.25;
  }
</style>
