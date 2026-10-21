<script lang="ts">
  import "$lib/styles/tokens.css";
  import { page } from "$app/state";
  import { onMount } from "svelte";
  import { on } from "$lib/api";
  import UpdateBanner from "$lib/UpdateBanner.svelte";

  let { children } = $props();

  // Sprache in den Einstellungen geändert: jedes Fenster lädt neu.
  onMount(() => {
    let stop: (() => void) | undefined;
    on("language-changed", () => location.reload()).then((f) => (stop = f));
    return () => stop?.();
  });
</script>

{@render children()}
<!-- Tray- und Einstellungsfenster zeigen keine Hinweise; Updates zeigt das Hauptfenster. -->
{#if !page.url.pathname.startsWith("/tray") && !page.url.pathname.startsWith("/settings")}
  <UpdateBanner />
{/if}
