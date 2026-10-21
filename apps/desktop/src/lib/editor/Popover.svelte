<script lang="ts">
  // Kleines Popover an einem Element, z. B. die Stichwörter einer Kachel.
  // Liegt fest über allem (nichts schneidet es ab), zeigt mit einem Pfeil auf
  // sein Element und bleibt im Fenster: darunter, sonst darüber. Schliesst bei
  // Klick daneben, Escape, Scrollen und Grössenänderung.

  import type { Snippet } from "svelte";

  let {
    anchor,
    label,
    onclose,
    onpointerenter,
    onpointerleave,
    children,
  }: {
    anchor: HTMLElement;
    label: string;
    onclose: () => void;
    onpointerenter?: () => void;
    onpointerleave?: () => void;
    children: Snippet;
  } = $props();

  /** Abstand zum Element und zum Fensterrand. */
  const GAP = 8;
  const MARGIN = 8;

  let box = $state<HTMLDivElement | null>(null);
  let pos = $state({ left: 0, top: 0, arrow: 0, above: false, ready: false });

  // Erst messen, dann zeigen – sonst springt es beim Öffnen.
  $effect(() => {
    if (!box) return;
    const a = anchor.getBoundingClientRect();
    const { offsetWidth: w, offsetHeight: h } = box;
    const center = a.left + a.width / 2;
    const left = Math.min(Math.max(center - w / 2, MARGIN), window.innerWidth - w - MARGIN);
    const above = a.bottom + GAP + h > window.innerHeight - MARGIN && a.top - GAP - h >= MARGIN;
    pos = {
      left,
      top: above ? a.top - GAP - h : a.bottom + GAP,
      arrow: Math.min(Math.max(center - left, 12), w - 12),
      above,
      ready: true,
    };
  });

  $effect(() => {
    // Scrollen irgendwo (auch in inneren Bereichen) löst es vom Element.
    const onScroll = (e: Event) => {
      if (!box?.contains(e.target as Node)) onclose();
    };
    // Escape schliesst nur das Popover, nicht auch die Galerie dahinter.
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Escape") return;
      e.stopPropagation();
      onclose();
    };
    document.addEventListener("scroll", onScroll, true);
    window.addEventListener("keydown", onKey, true);
    return () => {
      document.removeEventListener("scroll", onScroll, true);
      window.removeEventListener("keydown", onKey, true);
    };
  });

  function onPointerDown(e: PointerEvent) {
    const target = e.target as Node;
    if (!box?.contains(target) && !anchor.contains(target)) onclose();
  }
</script>

<svelte:window onpointerdown={onPointerDown} onresize={onclose} />

<div
  bind:this={box}
  class="popover"
  class:above={pos.above}
  class:ready={pos.ready}
  role="dialog"
  tabindex="-1"
  aria-label={label}
  style:left="{pos.left}px"
  style:top="{pos.top}px"
  style:--arrow="{pos.arrow}px"
  {onpointerenter}
  {onpointerleave}
>
  {@render children()}
</div>

<style>
  .popover {
    position: fixed;
    z-index: 50;
    max-width: min(280px, calc(100vw - 16px));
    padding: 10px;
    background: var(--surface);
    color: var(--text);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 10px 30px rgb(0 0 0 / 0.22);
    visibility: hidden;
    opacity: 0;
    transform: translateY(-3px);
    transition:
      opacity 0.12s ease,
      transform 0.12s ease;
  }
  .popover.above {
    transform: translateY(3px);
  }
  .popover.ready {
    visibility: visible;
    opacity: 1;
    transform: none;
  }
  /* Pfeil: gedrehtes Quadrat mit Rand auf zwei Seiten. */
  .popover::before {
    content: "";
    position: absolute;
    top: -6px;
    left: calc(var(--arrow) - 5px);
    width: 10px;
    height: 10px;
    background: var(--surface);
    border-left: 1px solid var(--border);
    border-top: 1px solid var(--border);
    transform: rotate(45deg);
  }
  .popover.above::before {
    top: auto;
    bottom: -6px;
    transform: rotate(225deg);
  }
</style>
