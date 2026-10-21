<script lang="ts">
  // Symbol aus $lib/icons (Phosphor, Duotone): Umriss in der Textfarbe, die
  // Fläche dahinter getönt. Tönung über `--icon-tone` (Standard: Textfarbe).
  import { ICONS, type IconName } from "$lib/icons";

  let { name, size = 18 }: { name: IconName; size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 256 256" fill="currentColor" aria-hidden="true">
  {@html ICONS[name]}
</svg>

<style>
  svg {
    flex: none;
  }
  svg :global(.tone) {
    fill: var(--icon-tone, currentColor);
    opacity: var(--icon-tone-opacity, 0.2);
  }
</style>
