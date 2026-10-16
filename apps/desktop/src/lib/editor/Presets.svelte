<script lang="ts">
  // Stil-Vorlagen: aktuellen Stil merken und mit einem Klick anwenden.
  // Gespeichert pro Gerät (localStorage) – es sind Bequemlichkeiten.

  import { t } from "$lib/i18n";
  import { toCss, type LayerKind } from "./model";
  import { editor, type Style } from "./state.svelte";

  const KEY = "fmshot.presets";
  const MAX = 6;

  function load(): Style[] {
    try {
      return JSON.parse(localStorage.getItem(KEY) ?? "[]") as Style[];
    } catch {
      return [];
    }
  }

  let presets = $state<Style[]>(load());
  let open = $state(false);

  function persist() {
    try {
      localStorage.setItem(KEY, JSON.stringify(presets));
    } catch {
      // Ohne Speicher bleiben die Vorlagen nur bis zum Neustart.
    }
  }

  function saveCurrent() {
    presets = [$state.snapshot(editor.style) as Style, ...presets].slice(0, MAX);
    persist();
  }

  function remove(i: number) {
    presets = presets.filter((_, j) => j !== i);
    persist();
  }

  function apply(p: Style) {
    editor.style = { ...p };
    const sel = editor.selected;
    if (!sel || sel.locked) return;
    const r = editor.ratio;
    editor.updateKind(sel.id, (k): LayerKind => {
      if (k.type === "text" || k.type === "callout") {
        return { ...k, style: { ...k.style, size: p.fontSize * r, bold: p.bold, color: k.type === "text" ? p.color : k.style.color } } as LayerKind;
      }
      if ("stroke" in k) return { ...k, stroke: { color: p.color, width: p.strokeWidth * r } } as LayerKind;
      if (k.type === "step") return { ...k, fill: p.color };
      return k;
    });
    open = false;
  }
</script>

<div class="presets">
  <button class="toggle" onclick={() => (open = !open)}>{t("presets.title")} ▾</button>
  {#if open}
    <div class="menu" role="menu">
      {#each presets as p, i (i)}
        <div class="item">
          <button class="apply" onclick={() => apply(p)}>
            <span class="dot" style:background={toCss(p.color)}></span>
            {Math.round(p.strokeWidth)} · {Math.round(p.fontSize)}pt{p.bold ? " · B" : ""}
          </button>
          <button class="del" aria-label={t("presets.delete")} onclick={() => remove(i)}>✕</button>
        </div>
      {:else}
        <p class="empty">{t("presets.empty")}</p>
      {/each}
      <button class="save" onclick={saveCurrent}>{t("presets.save")}</button>
    </div>
  {/if}
</div>

<style>
  .presets {
    position: relative;
    flex: none;
  }
  .toggle {
    padding: 3px 10px;
    border: 1px solid var(--border);
    border-radius: 6px;
    background: transparent;
    color: var(--text);
    font: inherit;
    cursor: pointer;
    white-space: nowrap;
  }
  .menu {
    position: absolute;
    top: 30px;
    left: 0;
    z-index: 25;
    min-width: 200px;
    padding: 6px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.2);
  }
  .item {
    display: flex;
  }
  .apply,
  .del,
  .save {
    border: none;
    background: none;
    color: var(--text);
    font: inherit;
    cursor: pointer;
    padding: 6px 8px;
    border-radius: 5px;
  }
  .apply {
    flex: 1;
    text-align: left;
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .apply:hover,
  .save:hover {
    background: var(--hover);
  }
  .del {
    color: var(--text-muted);
  }
  .dot {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 1px solid rgb(0 0 0 / 0.2);
  }
  .save {
    width: 100%;
    text-align: left;
    color: var(--selection);
    border-top: 1px solid var(--border);
    margin-top: 4px;
  }
  .empty {
    color: var(--text-muted);
    font-size: 12px;
    margin: 6px 8px;
  }
</style>
