<script lang="ts">
  import { t } from "$lib/i18n";
</script>

<main>
  <h1>{t("app.title")}</h1>
  <p>{t("app.placeholder")}</p>
</main>

<style>
  main {
    display: grid;
    place-content: center;
    height: 100%;
    text-align: center;
    gap: var(--space-2);
  }
  h1 {
    margin: 0;
    font-size: 28px;
  }
  p {
    margin: 0;
    color: var(--text-muted);
  }
</style>
