<script lang="ts">
  import "$lib/styles/tokens.css";
  import { page } from "$app/state";
  import UpdateBanner from "$lib/UpdateBanner.svelte";

  let { children } = $props();
</script>

{@render children()}
<!-- Das Tray-Fenster ist zu klein für Hinweise; Updates zeigt das Hauptfenster. -->
{#if !page.url.pathname.startsWith("/tray")}
  <UpdateBanner />
{/if}
