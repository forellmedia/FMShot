<script lang="ts">
  // Kontextabhängige Eigenschaften: wirken auf die ausgewählte Ebene und
  // werden als Stil für neue Ebenen gemerkt.

  import { t } from "$lib/i18n";
  import { editor } from "./state.svelte";
  import { canRotate, fromHex, PALETTE, rgb, sameColor, toCss, toHex, type Color, type Fill, type LayerKind } from "./model";
  import type { ToolId } from "./tools";
  import Presets from "./Presets.svelte";

  /** Breite, die die Leiste ohne Scrollen bräuchte (für die Kopfleiste). */
  let { contentWidth = $bindable(0) }: { contentWidth?: number } = $props();

  /** Welche Art wird gerade bearbeitet – Auswahl oder Werkzeug? */
  const kind = $derived<string>(editor.selected?.kind.type ?? toolKind(editor.tool));

  function toolKind(tool: ToolId): string {
    return tool === "select" ? "" : tool;
  }

  const ratio = $derived(editor.ratio);
  const sel = $derived(editor.selected);

  const has = (...types: string[]) => types.includes(kind);
  const hasStroke = $derived(has("arrow", "line", "rectangle", "ellipse", "pen", "magnifier"));
  const hasText = $derived(has("text", "callout"));

  const currentColor = $derived.by((): Color => {
    const k = sel?.kind;
    if (!k) return kind === "highlight" ? editor.style.highlightColor : editor.style.color;
    switch (k.type) {
      case "highlight":
        return k.color;
      case "text":
        return k.style.color;
      case "callout":
      case "step":
        return k.fill;
      default:
        return "stroke" in k ? k.stroke.color : editor.style.color;
    }
  });

  function setColor(c: Color) {
    if (kind === "highlight") editor.style.highlightColor = c;
    else editor.style.color = c;
    if (!sel) return;
    editor.updateKind(sel.id, (k): LayerKind => {
      switch (k.type) {
        case "highlight":
          return { ...k, color: c };
        case "text":
          return { ...k, style: { ...k.style, color: c } };
        case "callout":
        case "step":
          return { ...k, fill: c };
        case "rectangle":
        case "ellipse":
          return { ...k, stroke: { ...k.stroke, color: c }, fill: k.fill ? c : null };
        default:
          return "stroke" in k ? ({ ...k, stroke: { ...k.stroke, color: c } } as LayerKind) : k;
      }
    });
  }

  const strokeWidth = $derived(sel && "stroke" in sel.kind ? sel.kind.stroke.width / ratio : editor.style.strokeWidth);

  function setStrokeWidth(pt: number) {
    editor.style.strokeWidth = pt;
    if (sel && "stroke" in sel.kind) {
      editor.updateKind(sel.id, (k) => ("stroke" in k ? ({ ...k, stroke: { ...k.stroke, width: pt * ratio } } as LayerKind) : k));
    }
  }

  const filled = $derived(sel && (sel.kind.type === "rectangle" || sel.kind.type === "ellipse") ? !!sel.kind.fill : editor.style.fill);

  function toggleFill() {
    editor.style.fill = !filled;
    if (sel && (sel.kind.type === "rectangle" || sel.kind.type === "ellipse")) {
      const on = !filled;
      editor.updateKind(sel.id, (k) =>
        k.type === "rectangle" || k.type === "ellipse" ? { ...k, fill: on ? k.stroke.color : null } : k,
      );
    }
  }

  const textStyle = $derived(sel && (sel.kind.type === "text" || sel.kind.type === "callout") ? sel.kind.style : null);
  const fontSize = $derived(textStyle ? textStyle.size / ratio : editor.style.fontSize);
  const bold = $derived(textStyle ? textStyle.bold : editor.style.bold);
  const textBg = $derived(textStyle ? !!textStyle.background : editor.style.textBackground);

  function patchText(patch: (s: NonNullable<typeof textStyle>) => NonNullable<typeof textStyle>) {
    if (!sel) return;
    editor.updateKind(sel.id, (k) => (k.type === "text" || k.type === "callout" ? ({ ...k, style: patch(k.style) } as LayerKind) : k));
  }

  function setFontSize(pt: number) {
    editor.style.fontSize = pt;
    patchText((s) => ({ ...s, size: pt * ratio }));
  }

  function toggleBold() {
    editor.style.bold = !bold;
    patchText((s) => ({ ...s, bold: !bold }));
  }

  function toggleTextBg() {
    editor.style.textBackground = !textBg;
    patchText((s) => ({ ...s, background: textBg ? null : { r: 255, g: 255, b: 255, a: 230 } }));
  }

  const effectValue = $derived.by(() => {
    const k = sel?.kind;
    if (k?.type === "blur") return k.radius / ratio;
    if (k?.type === "pixelate") return k.block / ratio;
    if (k?.type === "spotlight") return k.dim * 100;
    if (kind === "blur") return editor.style.blurRadius;
    if (kind === "pixelate") return editor.style.pixelBlock;
    return editor.style.spotlightDim * 100;
  });

  function setEffect(v: number) {
    if (kind === "blur") editor.style.blurRadius = v;
    else if (kind === "pixelate") editor.style.pixelBlock = v;
    else editor.style.spotlightDim = v / 100;
    if (!sel) return;
    editor.updateKind(sel.id, (k) => {
      if (k.type === "blur") return { ...k, radius: v * ratio };
      if (k.type === "pixelate") return { ...k, block: v * ratio };
      if (k.type === "spotlight") return { ...k, dim: v / 100 };
      return k;
    });
  }

  /** Drehung: eine Ebene absolut, mehrere gemeinsam um die eingegebene Gradzahl. */
  const multi = $derived(!sel && editor.editableSelection.length > 1);
  const rotatable = $derived(sel ? !sel.locked && canRotate(sel.kind) : multi);

  function onRotation(e: Event & { currentTarget: HTMLInputElement }) {
    const v = +e.currentTarget.value;
    if (Number.isFinite(v)) editor.setRotation(v);
    if (multi) e.currentTarget.value = "0";
  }

  function setOpacity(v: number) {
    if (sel) editor.updateLayer(sel.id, (l) => (l.opacity = v / 100));
  }

  /** Verläufe für „Hintergrund“. */
  const BACKGROUNDS: Fill[] = [
    { type: "linear_gradient", from: rgb(0x6a, 0x5a, 0xf9), to: rgb(0xf9, 0x5a, 0x9a), angle: 135 },
    { type: "linear_gradient", from: rgb(0x22, 0xc1, 0xc3), to: rgb(0xfd, 0xbb, 0x2d), angle: 135 },
    { type: "linear_gradient", from: rgb(0x0f, 0x20, 0x27), to: rgb(0x2c, 0x53, 0x64), angle: 135 },
    { type: "linear_gradient", from: rgb(0xff, 0x9a, 0x9e), to: rgb(0xfa, 0xd0, 0xc4), angle: 135 },
    { type: "solid", color: rgb(0xf2, 0xf2, 0xf5) },
    { type: "solid", color: rgb(0x1c, 0x1c, 0x1e) },
  ];

  function fillCss(f: Fill): string {
    return f.type === "solid"
      ? toCss(f.color)
      : `linear-gradient(${f.angle + 90}deg, ${toCss(f.from)}, ${toCss(f.to)})`;
  }

  let padding = $state(48);

  const imageLayer = $derived(sel?.kind.type === "image" ? sel.kind : null);

  function setRadius(pt: number) {
    if (sel) editor.updateKind(sel.id, (k) => (k.type === "image" ? { ...k, corner_radius: pt * ratio } : k));
  }

  function toggleShadow() {
    if (!sel) return;
    editor.updateKind(sel.id, (k) =>
      k.type === "image"
        ? { ...k, shadow: k.shadow ? null : { color: rgb(0, 0, 0, 90), blur: 20 * ratio, offset_y: 6 * ratio } }
        : k,
    );
  }

  function resetCrop() {
    const img = editor.doc?.layers.find((l) => l.kind.type === "image" && l.kind.asset.source === "original");
    if (img && img.kind.type === "image") {
      const b = img.kind.bounds;
      editor.commit((d) => (d.artboard.rect = { ...b }));
    }
  }
</script>

<div class="props">
  <div class="inner" bind:clientWidth={contentWidth}>
    <Presets />
    {#if kind && !["image", "crop", "blur", "pixelate", "spotlight", "background"].includes(kind)}
      <div class="group" role="radiogroup" aria-label={t("props.color")}>
        {#each PALETTE as c (toHex(c))}
          <button
            class="swatch"
            class:on={sameColor(c, currentColor)}
            style:background={toCss(c)}
            aria-label={toHex(c)}
            onclick={() => setColor(c)}
          ></button>
        {/each}
        <input
          type="color"
          value={toHex(currentColor)}
          title={t("props.custom_color")}
          onchange={(e) => setColor(fromHex(e.currentTarget.value, currentColor.a))}
        />
      </div>
    {/if}

    {#if hasStroke}
      <label class="group">
        {t("props.stroke")}
        <input type="range" min="1" max="20" step="1" value={strokeWidth} oninput={(e) => setStrokeWidth(+e.currentTarget.value)} />
        <span class="value">{Math.round(strokeWidth)}</span>
      </label>
    {/if}

    {#if has("rectangle", "ellipse")}
      <button class="toggle" class:on={filled} onclick={toggleFill}>{t("props.fill")}</button>
    {/if}

    {#if hasText}
      <label class="group">
        {t("props.font_size")}
        <input type="number" min="6" max="200" value={Math.round(fontSize)} onchange={(e) => setFontSize(+e.currentTarget.value)} />
      </label>
      <button class="toggle" class:on={bold} onclick={toggleBold}><b>B</b></button>
      {#if kind === "text"}
        <button class="toggle" class:on={textBg} onclick={toggleTextBg}>{t("props.text_background")}</button>
      {/if}
    {/if}

    {#if has("blur", "pixelate", "spotlight")}
      <label class="group">
        {kind === "spotlight" ? t("props.dim") : t("props.strength")}
        <input
          type="range"
          min={kind === "spotlight" ? 10 : 2}
          max={kind === "spotlight" ? 90 : 40}
          value={effectValue}
          oninput={(e) => setEffect(+e.currentTarget.value)}
        />
      </label>
    {/if}

    {#if kind === "crop" || editor.tool === "crop"}
      <span class="hint">{t("props.crop_hint")}</span>
      <button class="toggle" onclick={resetCrop}>{t("props.crop_reset")}</button>
      <button class="toggle" onclick={() => editor.fitArtboardToContent()}>{t("props.fit_content")}</button>
    {/if}

    {#if editor.tool === "background" && !sel}
      <div class="group">
        {#each BACKGROUNDS as f, i (i)}
          <button class="swatch big" style:background={fillCss(f)} aria-label="Hintergrund {i + 1}" onclick={() => editor.applyBackground(f, padding)}></button>
        {/each}
      </div>
      <label class="group">
        {t("props.padding")}
        <input type="range" min="8" max="160" bind:value={padding} />
      </label>
      <button class="toggle" onclick={() => editor.removeBackground()}>{t("props.background_remove")}</button>
    {/if}

    {#if imageLayer}
      <label class="group">
        {t("props.radius")}
        <input type="range" min="0" max="40" value={imageLayer.corner_radius / ratio} oninput={(e) => setRadius(+e.currentTarget.value)} />
      </label>
      <button class="toggle" class:on={!!imageLayer.shadow} onclick={toggleShadow}>{t("props.shadow")}</button>
    {/if}

    {#if sel && !sel.locked}
      <label class="group">
        {t("props.opacity")}
        <input type="range" min="10" max="100" value={Math.round(sel.opacity * 100)} oninput={(e) => setOpacity(+e.currentTarget.value)} />
      </label>
    {/if}

    {#if rotatable}
      <label class="group" title={multi ? t("props.rotate_by_hint") : undefined}>
        {multi ? t("props.rotate_by") : t("props.rotation")}
        <input type="number" step="1" min="-360" max="360" value={sel ? Math.round(sel.rotation) : 0} onchange={onRotation} />°
      </label>
      {#if sel && sel.rotation}
        <button class="toggle" onclick={() => editor.setRotation(0)}>{t("props.rotation_reset")}</button>
      {/if}
    {/if}

    {#if editor.canGroup}
      <button class="toggle" title="⌘G" onclick={() => editor.groupSelection()}>{t("props.group")}</button>
    {/if}
    {#if editor.canUngroup}
      <button class="toggle" title="⇧⌘G" onclick={() => editor.ungroupSelection()}>{t("props.ungroup")}</button>
    {/if}

    {#if multi}
      <span class="hint">{t("props.selected_count", { count: String(editor.selectedLayers.length) })}</span>
    {:else if !kind && editor.tool !== "background"}
      <span class="hint">{t("props.select_hint")}</span>
    {/if}
  </div>
</div>

<style>
  .props {
    min-width: 0;
    overflow-x: auto;
    scrollbar-width: thin;
  }
  .inner {
    display: flex;
    align-items: center;
    gap: var(--space-4);
    width: max-content;
  }
  .group {
    display: flex;
    align-items: center;
    gap: 6px;
    white-space: nowrap;
    color: var(--text-muted);
  }
  .swatch {
    width: 20px;
    height: 20px;
    border-radius: 50%;
    border: 1px solid rgb(0 0 0 / 0.2);
    padding: 0;
    cursor: pointer;
  }
  .swatch.big {
    width: 26px;
    height: 26px;
    border-radius: 6px;
  }
  .swatch.on {
    outline: 2px solid var(--selection);
    outline-offset: 2px;
  }
  input[type="color"] {
    width: 26px;
    height: 24px;
    padding: 0;
    border: none;
    background: none;
  }
  input[type="range"] {
    width: 90px;
  }
  input[type="number"] {
    width: 54px;
  }
  .value {
    width: 18px;
    color: var(--text);
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
  .toggle.on {
    background: var(--accent);
    border-color: var(--accent);
    color: #fff;
  }
  .hint {
    color: var(--text-muted);
    white-space: nowrap;
  }
</style>
