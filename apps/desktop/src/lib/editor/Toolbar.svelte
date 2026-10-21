<script lang="ts">
  import { t } from "$lib/i18n";
  import { editor } from "./state.svelte";
  import Icon from "./Icon.svelte";
  import { isActive, TOOLS, VIDEO_TOOLS } from "./tools";

  // Im Video gibt es nur Werkzeuge, die ohne das Bild darunter auskommen.
  const tools = $derived(
    TOOLS.filter((t) => !t.hidden && (!editor.current?.videoPath || VIDEO_TOOLS.includes(t.id))),
  );
</script>

<nav class="tools" aria-label={t("editor.tools")}>
  {#each tools as tool (tool.id)}
    <button
      class:active={isActive(tool, editor.tool)}
      title="{t(tool.label)} ({tool.key.toUpperCase()})"
      aria-label={t(tool.label)}
      onclick={() => editor.setTool(tool.id === "blur" ? editor.style.obscure : tool.id)}
    >
      <Icon name={tool.icon} />
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
    background: var(--active);
    color: var(--on-active);
  }
</style>
