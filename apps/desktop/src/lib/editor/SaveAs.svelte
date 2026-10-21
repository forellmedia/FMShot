<script lang="ts">
  // „Speichern als …“ (⇧⌘S): Format wählen, danach fragt der Speichern-Dialog
  // nach dem Ort. Tasten: P / J wählen direkt, Enter den markierten Knopf.

  import { onMount } from "svelte";
  import { api } from "$lib/api";
  import { t } from "$lib/i18n";
  import { editor } from "./state.svelte";

  const FORMATS = [
    { id: "png", label: "PNG", hint: "editor.save_as_png", key: "p" },
    { id: "jpg", label: "JPG", hint: "editor.save_as_jpg", key: "j" },
  ] as const;
  type Format = (typeof FORMATS)[number]["id"];

  let buttons: Record<string, HTMLButtonElement> = {};

  onMount(() => {
    buttons.png?.focus();
    // Das Format aus den Einstellungen vorwählen.
    api.getSettings().then((s) => buttons[s.save_format ?? "png"]?.focus());
  });

  function pick(format: Format) {
    editor.saveAsOpen = false;
    void editor.exportAs(format);
  }

  function onKey(e: KeyboardEvent) {
    if (e.key === "Escape") {
      e.preventDefault();
      editor.saveAsOpen = false;
      return;
    }
    if (e.metaKey || e.ctrlKey || e.altKey) return;
    const f = FORMATS.find((f) => f.key === e.key.toLowerCase());
    if (f) {
      e.preventDefault();
      pick(f.id);
    }
  }
</script>

<svelte:window onkeydown={onKey} />

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="backdrop" onclick={() => (editor.saveAsOpen = false)}>
  <!-- svelte-ignore a11y_click_events_have_key_events -->
  <div class="dialog" role="dialog" aria-modal="true" aria-label={t("editor.save_as")} tabindex="-1" onclick={(e) => e.stopPropagation()}>
    <h2>{t("editor.save_as")}</h2>
    {#each FORMATS as f (f.id)}
      <button bind:this={buttons[f.id]} onclick={() => pick(f.id)}>
        <span class="label">{f.label}</span>
        <span class="hint">{t(f.hint)}</span>
        <kbd>{f.key.toUpperCase()}</kbd>
      </button>
    {/each}
  </div>
</div>

<style>
  .backdrop {
    position: fixed;
    inset: 0;
    z-index: 25;
    display: grid;
    place-items: center;
    background: rgb(0 0 0 / 0.25);
  }
  .dialog {
    display: flex;
    flex-direction: column;
    gap: 6px;
    width: min(320px, calc(100vw - 32px));
    padding: var(--space-3);
    border: 1px solid var(--border);
    border-radius: 12px;
    background: var(--surface);
    box-shadow: 0 12px 40px rgb(0 0 0 / 0.25);
    outline: none;
  }
  h2 {
    margin: 0 0 4px;
    font-size: 14px;
  }
  button {
    display: grid;
    grid-template-columns: 48px 1fr auto;
    align-items: center;
    gap: var(--space-2);
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: transparent;
    color: var(--text);
    font: inherit;
    text-align: left;
    cursor: pointer;
  }
  button:hover {
    background: var(--hover);
  }
  button:focus-visible,
  button:focus {
    border-color: var(--accent);
    box-shadow: 0 0 0 1px var(--accent);
    outline: none;
  }
  .label {
    font-weight: 600;
  }
  .hint {
    color: var(--text-muted);
    font-size: 13px;
  }
  kbd {
    color: var(--text-muted);
    font-family: inherit;
    font-size: 12px;
  }
</style>
