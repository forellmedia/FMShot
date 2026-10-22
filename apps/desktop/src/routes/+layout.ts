// Tauri hat keinen Node-Server – SvelteKit läuft als SPA (adapter-static).
// Siehe https://svelte.dev/docs/kit/single-page-apps
import { api, isTauri } from "$lib/api";
import { installErrorLogging } from "$lib/errorLog";
import { setLocale } from "$lib/i18n";
import { currentPlatform } from "$lib/shortcuts";

export const ssr = false;

installErrorLogging();

// Glas nur unter macOS (Vibrancy der Fenster); im Browser mit Ersatz-Hintergrund.
if (currentPlatform() === "mac") {
  document.documentElement.classList.add("glass");
  if (!isTauri) document.documentElement.classList.add("glass-preview");
}

/** Sprache aus den Einstellungen setzen, bevor etwas angezeigt wird. */
export async function load() {
  try {
    const settings = await api.getSettings();
    if (settings.language === "de" || settings.language === "en") setLocale(settings.language);
  } catch {
    // Ohne Einstellungen: Systemsprache.
  }
}
