<script lang="ts">
  // Einfaches Strich-Symbol aus einem SVG-Pfad (24×24).
  let { d, size = 18 }: { d: string; size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 24 24" aria-hidden="true">
  <path {d} fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round" />
</svg>
