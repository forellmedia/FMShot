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
  /* Schwebende Kapsel neben der Leinwand (Glas). */
  .tools {
    align-self: center;
    display: flex;
    flex-direction: column;
    gap: 2px;
    max-height: 100%;
    box-sizing: border-box;
    padding: 5px;
    border-radius: 24px;
    background: var(--glass);
    border: 1px solid var(--glass-edge);
    box-shadow: var(--glass-shadow);
    backdrop-filter: blur(24px) saturate(180%);
    -webkit-backdrop-filter: blur(24px) saturate(180%);
    overflow-y: auto;
    scrollbar-width: none;
  }
  button {
    flex: none;
    display: grid;
    place-items: center;
    width: 38px;
    height: 38px;
    border: none;
    border-radius: 999px;
    background: transparent;
    color: var(--text);
    cursor: pointer;
  }
  button:hover {
    background: var(--hover);
  }
  /* Gewählt: helle Glasblase mit blauem Symbol. */
  button.active {
    background: var(--glass-strong);
    color: var(--active);
    box-shadow: 0 2px 8px rgb(30 40 90 / 0.18), inset 0 1px 0 rgb(255 255 255 / 0.9);
  }
</style>
