<script lang="ts">
  import { onMount } from "svelte";
  import { commands, type UpdateInfo } from "$lib/bindings";
  import { t } from "$lib/i18n";

  /** Wie oft im Hintergrund nach Updates gesucht wird. */
  const CHECK_INTERVAL_MS = 6 * 60 * 60 * 1000;

  let update = $state<UpdateInfo | null>(null);
  let installing = $state(false);
  let error = $state<string | null>(null);
  let dismissed = $state(false);

  async function check() {
    const result = await commands.checkForUpdate();
    // Fehler beim Prüfen (offline, noch kein Release) still ignorieren.
    if (result.status === "ok") update = result.data;
  }

  async function install() {
    installing = true;
    error = null;
    const result = await commands.installUpdate();
    // Bei Erfolg startet die App neu – hier kommen wir nur bei Fehlern an.
    if (result.status === "error") error = result.error;
    installing = false;
  }

  onMount(() => {
    // Im Entwicklungsmodus gibt es keine passenden Releases.
    if (import.meta.env.DEV) return;
    check();
    const timer = setInterval(check, CHECK_INTERVAL_MS);
    return () => clearInterval(timer);
  });
</script>

{#if update && !dismissed}
  <div class="banner" role="status">
    <span>{t("update.available", { version: update.version })}</span>
    {#if error}
      <span class="error">{t("update.failed", { error })}</span>
    {/if}
    <button class="primary" onclick={install} disabled={installing}>
      {installing ? t("update.installing") : t("update.install")}
    </button>
    <button onclick={() => (dismissed = true)} disabled={installing}>
      {t("update.later")}
    </button>
  </div>
{/if}

<style>
  .banner {
    position: fixed;
    left: var(--space-4);
    right: var(--space-4);
    bottom: var(--space-4);
    display: flex;
    align-items: center;
    gap: var(--space-3);
    padding: var(--space-3) var(--space-4);
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: var(--radius);
    box-shadow: 0 4px 16px rgb(0 0 0 / 0.12);
  }
  .banner span:first-child {
    flex: 1;
  }
  .error {
    color: var(--accent);
  }
  button {
    font: inherit;
    padding: var(--space-1) var(--space-3);
    border-radius: 6px;
    border: 1px solid var(--border);
    background: transparent;
    color: var(--text);
    cursor: pointer;
  }
  button.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: white;
  }
  button:disabled {
    opacity: 0.6;
    cursor: default;
  }
</style>
