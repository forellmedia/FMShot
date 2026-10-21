<script lang="ts">
  import "$lib/styles/tokens.css";
  import { page } from "$app/state";
  import { onMount } from "svelte";
  import { on } from "$lib/api";
  import { isTextField } from "$lib/textField";
  import UpdateBanner from "$lib/UpdateBanner.svelte";

  let { children } = $props();

  // Sprache in den Einstellungen geändert: jedes Fenster lädt neu.
  onMount(() => {
    let stop: (() => void) | undefined;
    on("language-changed", () => location.reload()).then((f) => (stop = f));
    return () => stop?.();
  });

  // Das Standardmenü des WebViews („Neu laden“, „Element untersuchen“) hat
  // in der App nichts zu suchen; eigene Menüs öffnen die Bereiche selbst.
  // Textfelder behalten ihr Menü (Kopieren, Einsetzen). Beim Entwickeln bleibt
  // es zum Untersuchen.
  function onContextMenu(e: MouseEvent) {
    if (import.meta.env.DEV) return;
    if (isTextField(e.target)) return;
    e.preventDefault();
  }
</script>

<svelte:window oncontextmenu={onContextMenu} />

{@render children()}
<!-- Tray- und Einstellungsfenster zeigen keine Hinweise; Updates zeigt das Hauptfenster. -->
{#if !page.url.pathname.startsWith("/tray") && !page.url.pathname.startsWith("/settings")}
  <UpdateBanner />
{/if}
