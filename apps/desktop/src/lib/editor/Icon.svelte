<script lang="ts">
  // Symbol aus $lib/icons (Phosphor Light) in der Textfarbe.
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
</style>
