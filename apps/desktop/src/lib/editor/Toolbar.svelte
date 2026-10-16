<script lang="ts">
  import { t } from "$lib/i18n";
  import { editor } from "./state.svelte";
  import Icon from "./Icon.svelte";
  import { TOOLS } from "./tools";
</script>

<nav class="tools" aria-label={t("editor.tools")}>
  {#each TOOLS as tool (tool.id)}
    <button
      class:active={editor.tool === tool.id}
      title="{t(tool.label)} ({tool.key.toUpperCase()})"
      aria-label={t(tool.label)}
      onclick={() => {
        editor.tool = tool.id;
        if (tool.id !== "select") editor.selectedId = null;
      }}
    >
      <Icon d={tool.icon} />
    </button>
  {/each}
</nav>

<style>
  .tools {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: var(--space-2) 6px;
    background: var(--surface);
    border-right: 1px solid var(--border);
    overflow-y: auto;
  }
  button {
    display: grid;
    place-items: center;
    width: 36px;
    height: 34px;
    border: none;
    border-radius: 7px;
    background: transparent;
    color: var(--text);
    cursor: pointer;
  }
  button:hover {
    background: var(--hover);
  }
  button.active {
    background: var(--accent);
    color: #fff;
  }
</style>
