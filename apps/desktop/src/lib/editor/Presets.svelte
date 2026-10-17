<script lang="ts">
  // Stil-Vorlagen: aktuellen Stil merken und mit einem Klick anwenden.
  // Gespeichert pro Gerät (localStorage) – es sind Bequemlichkeiten.

  import { t } from "$lib/i18n";
  import { toCss, type LayerKind } from "./model";
  import { editor, type Style } from "./state.svelte";
  import Dropdown from "./Dropdown.svelte";

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
    // Ältere Vorlagen kennen nicht alle Felder: mit dem aktuellen Stil auffüllen.
    const saved = p as Partial<Style> & { textBackground?: unknown };
    editor.style = {
      ...editor.style,
      ...saved,
      textBackground: typeof saved.textBackground === "object" ? saved.textBackground : editor.style.textBackground,
    };
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
  }
</script>

<Dropdown variant="outline">
  {#snippet label()}{t("presets.title")} ▾{/snippet}
  {#snippet children(close)}
    {#each presets as p, i (i)}
      <div class="row">
        <button
          class="item"
          onclick={() => {
            apply(p);
            close();
          }}
        >
          <span class="swatch">
            <span class="dot" style:background={toCss(p.color)}></span>
            {Math.round(p.strokeWidth)} · {Math.round(p.fontSize)}pt{p.bold ? " · B" : ""}
          </span>
        </button>
        <button class="del" aria-label={t("presets.delete")} onclick={() => remove(i)}>✕</button>
      </div>
    {:else}
      <p class="empty">{t("presets.empty")}</p>
    {/each}
    <hr />
    <button class="item" onclick={saveCurrent}><span class="save">{t("presets.save")}</span></button>
  {/snippet}
</Dropdown>

<style>
  .row {
    display: flex;
  }
  .swatch {
    display: flex;
    align-items: center;
    gap: 8px;
  }
  .dot {
    width: 14px;
    height: 14px;
    border-radius: 50%;
    border: 1px solid rgb(0 0 0 / 0.2);
  }
  .del {
    border: none;
    background: none;
    color: var(--text-muted);
    font: inherit;
    cursor: pointer;
    padding: 6px 8px;
    border-radius: 5px;
  }
  .del:hover {
    background: var(--hover);
  }
  hr {
    border: none;
    border-top: 1px solid var(--border);
    margin: 4px 0;
  }
  .save {
    color: var(--selection);
  }
  .empty {
    color: var(--text-muted);
    font-size: 12px;
    margin: 6px 8px;
  }
</style>
