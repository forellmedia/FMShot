<script lang="ts">
  // Aufklappmenü der Kopfleiste. Das Menü liegt fest über allem und wird so
  // nicht vom Scroll-Bereich der Eigenschaftsleiste abgeschnitten. Schliesst
  // bei Klick daneben, mit Escape und wenn sich die Fenstergrösse ändert.
  // Einträge mit der Klasse `item` bekommen das Standard-Aussehen.

  import type { Snippet } from "svelte";

  let {
    label,
    title,
    align = "left",
    variant = "plain",
    children,
  }: {
    label: Snippet;
    title?: string;
    align?: "left" | "right";
    variant?: "plain" | "outline";
    /** Inhalt; bekommt eine Funktion zum Schliessen. */
    children: Snippet<[() => void]>;
  } = $props();

  let open = $state(false);
  let button: HTMLButtonElement;
  let menu = $state<HTMLDivElement | null>(null);
  let pos = $state({ top: 0, left: 0, right: 0 });

  function toggle() {
    if (!open) {
      const r = button.getBoundingClientRect();
      pos = { top: r.bottom + 4, left: r.left, right: window.innerWidth - r.right };
    }
    open = !open;
  }

  const close = () => (open = false);

  function onPointerDown(e: PointerEvent) {
    const target = e.target as Node;
    if (open && !menu?.contains(target) && !button.contains(target)) close();
  }

  function onKeyDown(e: KeyboardEvent) {
    if (open && e.key === "Escape") close();
  }
</script>

<svelte:window onpointerdown={onPointerDown} onkeydown={onKeyDown} onresize={close} />

<button
  bind:this={button}
  class="trigger {variant}"
  class:on={open}
  {title}
  aria-haspopup="menu"
  aria-expanded={open}
  onclick={toggle}
>
  {@render label()}
</button>
{#if open}
  <div
    bind:this={menu}
    class="menu"
    role="menu"
    style:top="{pos.top}px"
    style:left={align === "left" ? `${pos.left}px` : null}
    style:right={align === "right" ? `${pos.right}px` : null}
  >
    {@render children(close)}
  </div>
{/if}

<style>
  .trigger {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 4px;
    height: 30px;
    padding: 0 8px;
    border: none;
    border-radius: 6px;
    background: transparent;
    color: var(--text);
    font: inherit;
    white-space: nowrap;
    cursor: pointer;
  }
  .trigger.outline {
    height: auto;
    padding: 3px 10px;
    border: 1px solid var(--border);
  }
  .trigger:hover,
  .trigger.on {
    background: var(--hover);
  }
  .menu {
    position: fixed;
    z-index: 40;
    min-width: 200px;
    max-width: calc(100vw - 16px);
    padding: 6px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 8px;
    box-shadow: 0 8px 24px rgb(0 0 0 / 0.2);
  }
  .menu :global(.item) {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: var(--space-4);
    width: 100%;
    padding: 6px 8px;
    border: none;
    border-radius: 5px;
    background: none;
    color: var(--text);
    font: inherit;
    text-align: left;
    white-space: nowrap;
    cursor: pointer;
  }
  .menu :global(.item:hover:not(:disabled)) {
    background: var(--hover);
  }
  .menu :global(.item:disabled) {
    opacity: 0.35;
    cursor: default;
  }
  .menu :global(.item kbd) {
    color: var(--text-muted);
    font-family: inherit;
  }
</style>
