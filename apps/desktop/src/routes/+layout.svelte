<script lang="ts">
  import "$lib/styles/tokens.css";
  import UpdateBanner from "$lib/UpdateBanner.svelte";

  let { children } = $props();
</script>

{@render children()}
<UpdateBanner />
