// Tauri hat keinen Node-Server – SvelteKit läuft als SPA (adapter-static).
// Siehe https://svelte.dev/docs/kit/single-page-apps
import { api } from "$lib/api";
import { setLocale } from "$lib/i18n";

export const ssr = false;

/** Sprache aus den Einstellungen setzen, bevor etwas angezeigt wird. */
export async function load() {
  try {
    const settings = await api.getSettings();
    if (settings.language === "de" || settings.language === "en") setLocale(settings.language);
  } catch {
    // Ohne Einstellungen: Systemsprache.
  }
}
