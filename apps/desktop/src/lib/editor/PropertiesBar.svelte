<script lang="ts">
  // Kontextabhängige Eigenschaften: wirken auf die ausgewählte Ebene und
  // werden als Stil für neue Ebenen gemerkt.

  import { t } from "$lib/i18n";
  import { formatShortcut } from "$lib/shortcuts";
  import { editor } from "./state.svelte";
  import {
    BACKGROUNDS,
    canRotate,
    PALETTE,
    rgb,
    toCss,
    type ArrowHead,
    type Color,
    type Corner,
    type Fill,
    type LayerKind,
    type LineStyle,
    type TextAlign,
    type VerticalAlign,
  } from "./model";
  import type { ToolId } from "./tools";
  import ChoicePicker from "./ChoicePicker.svelte";
  import ColorPicker from "./ColorPicker.svelte";
  import Dropdown from "./Dropdown.svelte";
  import NumberPicker from "./NumberPicker.svelte";
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
  const hasLineStyle = $derived(has("arrow", "line", "rectangle", "ellipse"));

  // --- Linienart und Pfeilspitze ---------------------------------------------

  const LINE_STYLES: LineStyle[] = ["solid", "dashed", "dotted", "double"];
  const lineStyle = $derived(sel && "stroke" in sel.kind ? sel.kind.stroke.style : editor.style.lineStyle);

  function setLineStyle(v: LineStyle) {
    editor.style.lineStyle = v;
    if (sel && "stroke" in sel.kind) {
      editor.updateKind(sel.id, (k) => ("stroke" in k ? ({ ...k, stroke: { ...k.stroke, style: v } } as LayerKind) : k));
    }
  }

  const HEAD_SIZES = [
    { size: 1, label: "arrow.small", short: "S" },
    { size: 1.6, label: "arrow.medium", short: "M" },
    { size: 2.3, label: "arrow.large", short: "L" },
  ] as const;
  /** Nächstliegende Stufe (ältere Pfeile haben Zwischenwerte). */
  const headStep = $derived(
    HEAD_SIZES.reduce((a, b) => (Math.abs(b.size - arrowHeadSize) < Math.abs(a.size - arrowHeadSize) ? b : a)).size,
  );
  const arrowHead = $derived(sel?.kind.type === "arrow" ? sel.kind.head : editor.style.arrowHead);
  const arrowHeadSize = $derived(sel?.kind.type === "arrow" ? sel.kind.head_size : editor.style.arrowHeadSize);

  function setArrow(patch: { head?: ArrowHead; head_size?: number }) {
    if (patch.head) editor.style.arrowHead = patch.head;
    if (patch.head_size) editor.style.arrowHeadSize = patch.head_size;
    if (sel?.kind.type === "arrow") editor.updateKind(sel.id, (k) => (k.type === "arrow" ? { ...k, ...patch } : k));
  }

  // --- Nummern ---------------------------------------------------------------

  const stepSel = $derived(sel?.kind.type === "step" ? sel.kind : null);
  const firstStep = $derived.by(() => {
    const l = editor.doc?.layers.find((l) => l.kind.type === "step");
    return l?.kind.type === "step" ? l.kind : null;
  });
  const stepSize = $derived(
    stepSel ? stepSel.radius / ratio : firstStep ? firstStep.radius / ratio : editor.style.stepRadius,
  );

  /** Nummern sind immer gleich gross: wirkt auf alle. */
  function setStepSize(pt: number) {
    editor.style.stepRadius = pt;
    editor.setStepRadius(pt * ratio);
  }

  /** Ausgewählte Nummer: ihre Zahl; sonst die Startzahl für neue. */
  function setStepNumber(n: number) {
    if (!Number.isFinite(n) || n < 0) return;
    n = Math.round(n);
    if (sel && stepSel) editor.updateKind(sel.id, (k) => (k.type === "step" ? { ...k, number: n } : k));
    else editor.style.stepStart = n;
  }

  // --- Spotlight und Lupe ----------------------------------------------------

  const spotSel = $derived(sel?.kind.type === "spotlight" ? sel.kind : null);
  const spotRadius = $derived(spotSel ? spotSel.corner_radius / ratio : editor.style.spotlightRadius);
  const spotFeather = $derived(spotSel ? spotSel.feather / ratio : editor.style.spotlightFeather);

  function setSpotlight(patch: { radius?: number; feather?: number }) {
    if (patch.radius != null) editor.style.spotlightRadius = patch.radius;
    if (patch.feather != null) editor.style.spotlightFeather = patch.feather;
    if (!sel) return;
    editor.updateKind(sel.id, (k) =>
      k.type === "spotlight"
        ? {
            ...k,
            corner_radius: patch.radius != null ? patch.radius * ratio : k.corner_radius,
            feather: patch.feather != null ? patch.feather * ratio : k.feather,
          }
        : k,
    );
  }

  const HANDLES: (Corner | null)[] = [null, "top_left", "top_right", "bottom_left", "bottom_right"];
  const HANDLE_ICON: Record<Corner, string> = {
    top_left: "↖",
    top_right: "↗",
    bottom_left: "↙",
    bottom_right: "↘",
  };
  const handle = $derived(sel?.kind.type === "magnifier" ? sel.kind.handle : editor.style.magnifierHandle);

  function setHandle(h: Corner | null) {
    editor.style.magnifierHandle = h;
    if (sel?.kind.type === "magnifier") editor.updateKind(sel.id, (k) => (k.type === "magnifier" ? { ...k, handle: h } : k));
  }

  // --- Unkenntlich machen ----------------------------------------------------

  /** Weichzeichnen ⇄ Verpixeln – für das Werkzeug und eine ausgewählte Ebene. */
  function setObscure(mode: "blur" | "pixelate") {
    editor.style.obscure = mode;
    if (editor.tool === "blur" || editor.tool === "pixelate") editor.tool = mode;
    if (!sel) return;
    editor.updateKind(sel.id, (k): LayerKind => {
      if (k.type !== "blur" && k.type !== "pixelate") return k;
      return mode === "blur"
        ? { type: "blur", bounds: k.bounds, radius: editor.style.blurRadius * ratio }
        : { type: "pixelate", bounds: k.bounds, block: editor.style.pixelBlock * ratio };
    });
  }

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
  const textBg = $derived(textStyle ? textStyle.background : editor.style.textBackground);
  const textRadius = $derived(
    textStyle ? (textStyle.corner_radius ?? textStyle.size * 0.25) / ratio : editor.style.textRadius,
  );
  const textAlign = $derived(textStyle ? textStyle.align : editor.style.textAlign);
  const textValign = $derived(textStyle ? textStyle.valign : editor.style.textValign);
  const ALIGNS: TextAlign[] = ["left", "center", "right"];
  const VALIGNS: VerticalAlign[] = ["top", "middle", "bottom"];
  const TEXT_BACKGROUNDS: Color[] = [rgb(255, 255, 255, 235), rgb(0x21, 0x21, 0x21, 235), ...PALETTE.slice(0, 6)];

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

  function setTextBg(c: Color | null) {
    editor.style.textBackground = c;
    patchText((s) => ({ ...s, background: c }));
  }

  function setTextRadius(pt: number) {
    editor.style.textRadius = pt;
    patchText((s) => ({ ...s, corner_radius: pt * ratio }));
  }

  function setTextAlign(align: TextAlign, valign: VerticalAlign) {
    editor.style.textAlign = align;
    editor.style.textValign = valign;
    patchText((s) => ({ ...s, align, valign }));
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


  function fillCss(f: Fill): string {
    return f.type === "solid"
      ? toCss(f.color)
      : `linear-gradient(${f.angle + 90}deg, ${toCss(f.from)}, ${toCss(f.to)})`;
  }

  let padding = $state(48);

  const imageLayer = $derived(sel?.kind.type === "image" ? sel.kind : null);

  function setRadius(pt: number, id = sel?.id) {
    if (id != null) editor.updateKind(id, (k) => (k.type === "image" ? { ...k, corner_radius: pt * ratio } : k));
  }

  /** Die Aufnahme selbst (für „Präsentation“). */
  const original = $derived(
    editor.doc?.layers.find((l) => l.kind.type === "image" && l.kind.asset.source === "original") ?? null,
  );
  const presentationImage = $derived(original?.kind.type === "image" ? original.kind : null);

  function toggleShadow(id = sel?.id) {
    if (id == null) return;
    editor.updateKind(id, (k) =>
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

{#snippet lineIcon(v: LineStyle)}
  <svg width="22" height="12" viewBox="0 0 22 12" aria-hidden="true">
    {#if v === "double"}
      <path d="M2 3.5h18M2 8.5h18" stroke="currentColor" stroke-width="1.6" />
    {:else}
      <path
        d="M3 6h16"
        stroke="currentColor"
        stroke-width="2.2"
        stroke-linecap="round"
        stroke-dasharray={v === "dashed" ? "4 4" : v === "dotted" ? "0.1 4" : undefined}
      />
    {/if}
  </svg>
{/snippet}

{#snippet headIcon(h: ArrowHead)}
  <svg width="22" height="12" viewBox="0 0 22 12" aria-hidden="true">
    {#if h === "filled"}
      <path d="M2 6h12" stroke="currentColor" stroke-width="2" /><path d="M12 1.5L20 6l-8 4.5z" fill="currentColor" />
    {:else}
      <path d="M2 6h17M13 1.5L19.5 6 13 10.5" fill="none" stroke="currentColor" stroke-width="2" stroke-linecap="round" stroke-linejoin="round" />
    {/if}
  </svg>
{/snippet}

{#snippet sizeIcon(size: number)}
  <b class="size">{HEAD_SIZES.find((h) => h.size === size)?.short}</b>
{/snippet}

{#snippet handleIcon(h: Corner | null)}
  <span class="glyph">{h ? HANDLE_ICON[h] : "○"}</span>
{/snippet}

{#snippet alignIcon(align: TextAlign, valign: VerticalAlign)}
  <svg width="18" height="18" viewBox="0 0 18 18" aria-hidden="true">
    <rect x="0.5" y="0.5" width="17" height="17" rx="3" fill="none" stroke="currentColor" stroke-opacity="0.35" />
    {#each [0, 1] as i (i)}
      {@const w = i === 0 ? 10 : 7}
      {@const x = align === "left" ? 3 : align === "center" ? 9 - w / 2 : 15 - w}
      {@const y = (valign === "top" ? 4 : valign === "middle" ? 7.5 : 11) + i * 3}
      <path d="M{x} {y}h{w}" stroke="currentColor" stroke-width="1.6" stroke-linecap="round" />
    {/each}
  </svg>
{/snippet}

<div class="props">
  <div class="inner" bind:clientWidth={contentWidth}>
    <Presets />
    {#if kind && !["image", "crop", "blur", "pixelate", "spotlight", "background"].includes(kind)}
      <ColorPicker value={currentColor} colors={PALETTE} title={t("props.color")} onpick={(c) => c && setColor(c)} />
    {/if}

    {#if hasStroke}
      <NumberPicker name={t("props.stroke")} value={strokeWidth} min={1} max={20} onchange={setStrokeWidth} />
    {/if}

    {#if hasLineStyle}
      <ChoicePicker
        value={lineStyle}
        options={LINE_STYLES.map((v) => ({ value: v, label: t(`line.${v}`) }))}
        title={t("props.line_style")}
        icon={lineIcon}
        onpick={setLineStyle}
      />
    {/if}

    {#if kind === "arrow"}
      <ChoicePicker
        value={arrowHead}
        options={(["filled", "open"] as const).map((h) => ({ value: h, label: t(`arrow.${h}`) }))}
        title={t("props.arrow_head")}
        icon={headIcon}
        onpick={(h) => setArrow({ head: h })}
      />
      <ChoicePicker
        value={headStep}
        options={HEAD_SIZES.map((h) => ({ value: h.size, label: t(h.label) }))}
        title={t("props.arrow_head")}
        icon={sizeIcon}
        onpick={(size) => setArrow({ head_size: size })}
      />
    {/if}

    {#if has("rectangle", "ellipse")}
      <button class="toggle" class:on={filled} onclick={toggleFill}>{t("props.fill")}</button>
    {/if}

    {#if kind === "step"}
      <NumberPicker name={t("props.step_size")} value={stepSize} min={8} max={40} onchange={setStepSize} />
      <NumberPicker
        name={stepSel ? t("props.step_number") : t("props.step_start")}
        value={stepSel ? stepSel.number : editor.style.stepStart}
        min={0}
        max={99}
        onchange={setStepNumber}
      />
    {/if}

    {#if kind === "magnifier"}
      <ChoicePicker
        value={handle}
        options={HANDLES.map((h) => ({ value: h, label: t(`handle.${h ?? "none"}`) }))}
        title={t("props.handle")}
        icon={handleIcon}
        onpick={setHandle}
      />
    {/if}

    {#if hasText}
      <NumberPicker name={t("props.font_size")} value={fontSize} min={6} max={120} onchange={setFontSize} />
      <button class="toggle" class:on={bold} onclick={toggleBold}><b>B</b></button>
      {#if kind === "text"}
        <Dropdown variant="outline" title={t("props.text_align")}>
          {#snippet label()}{@render alignIcon(textAlign, textValign)} ▾{/snippet}
          {#snippet children(close)}
            <div class="align-grid" role="radiogroup" aria-label={t("props.text_align")}>
              {#each VALIGNS as v (v)}
                {#each ALIGNS as a (a)}
                  <button
                    class:on={a === textAlign && v === textValign}
                    aria-label="{t(`align.${a}`)}, {t(`valign.${v}`)}"
                    title="{t(`align.${a}`)}, {t(`valign.${v}`)}"
                    onclick={() => {
                      setTextAlign(a, v);
                      close();
                    }}
                  >
                    {@render alignIcon(a, v)}
                  </button>
                {/each}
              {/each}
            </div>
          {/snippet}
        </Dropdown>
        <ColorPicker
          value={textBg}
          colors={TEXT_BACKGROUNDS}
          title={t("props.text_background")}
          noneLabel={t("props.text_background_none")}
          onpick={setTextBg}
        />
        {#if textBg}
          <NumberPicker name={t("props.radius")} value={textRadius} min={0} max={30} onchange={setTextRadius} />
        {/if}
      {/if}
    {/if}

    {#if has("blur", "pixelate")}
      <div class="seg" role="radiogroup" aria-label={t("tool.obscure")}>
        {#each ["blur", "pixelate"] as const as m (m)}
          <button class="text" class:on={kind === m} onclick={() => setObscure(m)}>{t(`obscure.${m}`)}</button>
        {/each}
      </div>
    {/if}

    {#if has("blur", "pixelate", "spotlight")}
      <NumberPicker
        name={kind === "spotlight" ? t("props.dim") : t("props.strength")}
        value={effectValue}
        min={kind === "spotlight" ? 10 : 2}
        max={kind === "spotlight" ? 90 : 40}
        unit={kind === "spotlight" ? "%" : ""}
        onchange={setEffect}
      />
    {/if}

    {#if kind === "spotlight"}
      <NumberPicker name={t("props.radius")} value={spotRadius} min={0} max={40} onchange={(v) => setSpotlight({ radius: v })} />
      <NumberPicker name={t("props.feather")} value={spotFeather} min={0} max={40} onchange={(v) => setSpotlight({ feather: v })} />
    {/if}

    {#if kind === "crop" || editor.tool === "crop"}
      <span class="hint">{t("props.crop_hint")}</span>
      <button class="toggle" onclick={resetCrop}>{t("props.crop_reset")}</button>
      <button class="toggle" onclick={() => editor.fitArtboardToContent()}>{t("props.fit_content")}</button>
    {/if}

    {#if editor.tool === "background" && !sel}
      <div class="group">
        {#each BACKGROUNDS as f, i (i)}
          <button class="swatch big" style:background={fillCss(f)} aria-label="{t('tool.background')} {i + 1}" onclick={() => editor.applyBackground(f, padding)}></button>
        {/each}
      </div>
      <NumberPicker
        name={t("props.padding")}
        value={padding}
        min={8}
        max={160}
        step={4}
        onchange={(v) => {
          padding = v;
          const bg = editor.doc?.artboard.background;
          if (bg) editor.applyBackground(bg, padding);
        }}
      />
      {#if original && presentationImage}
        <NumberPicker
          name={t("props.radius")}
          value={presentationImage.corner_radius / ratio}
          min={0}
          max={40}
          onchange={(v) => setRadius(v, original.id)}
        />
        <button class="toggle" class:on={!!presentationImage.shadow} onclick={() => toggleShadow(original.id)}>{t("props.shadow")}</button>
      {/if}
      <button class="toggle" onclick={() => editor.removeBackground()}>{t("props.background_remove")}</button>
    {/if}

    {#if imageLayer}
      <NumberPicker name={t("props.radius")} value={imageLayer.corner_radius / ratio} min={0} max={40} onchange={(v) => setRadius(v)} />
      <button class="toggle" class:on={!!imageLayer.shadow} onclick={() => toggleShadow()}>{t("props.shadow")}</button>
    {/if}

    {#if sel && !sel.locked}
      <NumberPicker name={t("props.opacity")} value={sel.opacity * 100} min={10} max={100} step={5} unit="%" onchange={setOpacity} />
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
      <button class="toggle" title={formatShortcut("Primary+G")} onclick={() => editor.groupSelection()}>{t("props.group")}</button>
    {/if}
    {#if editor.canUngroup}
      <button class="toggle" title={formatShortcut("Primary+Shift+G")} onclick={() => editor.ungroupSelection()}>{t("props.ungroup")}</button>
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
  .seg {
    display: flex;
    flex: none;
    border: 1px solid var(--border);
    border-radius: 6px;
    overflow: hidden;
  }
  .seg button {
    display: grid;
    place-items: center;
    min-width: 30px;
    height: 24px;
    padding: 0 6px;
    border: none;
    background: transparent;
    color: var(--text);
    font: inherit;
    cursor: pointer;
  }
  .seg button + button {
    border-left: 1px solid var(--border);
  }
  .seg button:hover {
    background: var(--hover);
  }
  .seg button.on {
    background: var(--active);
    color: var(--on-active);
  }
  .swatch.big {
    width: 26px;
    height: 26px;
    border-radius: 6px;
  }
  input[type="number"] {
    width: 54px;
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
    background: var(--active);
    border-color: var(--active);
    color: var(--on-active);
  }
  .size {
    min-width: 12px;
    text-align: center;
  }
  .glyph {
    min-width: 14px;
    text-align: center;
  }
  .align-grid {
    display: grid;
    grid-template-columns: repeat(3, 32px);
    gap: 4px;
    padding: 2px;
  }
  .align-grid button {
    display: grid;
    place-items: center;
    height: 32px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--text);
    cursor: pointer;
  }
  .align-grid button:hover {
    background: var(--hover);
  }
  .align-grid button.on {
    background: var(--active);
    color: var(--on-active);
  }
  .hint {
    color: var(--text-muted);
    white-space: nowrap;
  }
</style>
