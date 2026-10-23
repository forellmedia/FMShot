// Kürzel für die Anzeige, je Plattform: macOS mit Symbolen (⇧⌘Z), Windows
// und Linux mit übersetzten Tastennamen (Strg+Umschalt+Z) – dort nie ⌘.
// Globale Kürzel formatiert das Backend auf dieselbe Weise (HotkeyInfo.label).

import { t } from "$lib/i18n";

export type Platform = "mac" | "windows" | "linux";

export function currentPlatform(): Platform {
  if (typeof navigator === "undefined") return "mac";
  const id = `${navigator.platform} ${navigator.userAgent}`;
  if (/Mac/.test(id)) return "mac";
  return /Win/.test(id) ? "windows" : "linux";
}

const MAC: Record<string, string> = {
  Secondary: "⌃",
  Alt: "⌥",
  Shift: "⇧",
  Primary: "⌘",
  Backspace: "⌫",
  Delete: "⌦",
};

/**
 * `spec` wie im Backend: Modifikatoren und Taste mit „+“ getrennt, z. B.
 * `Primary+Shift+Z`. `Primary` ist ⌘ auf macOS und Strg sonst.
 */
export function formatShortcut(spec: string, platform: Platform = currentPlatform()): string {
  const parts = spec.split("+").filter(Boolean);
  const key = parts.pop() ?? "";
  const mods = new Set(parts);
  if (platform === "mac") {
    const order = ["Secondary", "Alt", "Shift", "Primary"].filter((m) => mods.has(m));
    return order.map((m) => MAC[m]).join("") + (MAC[key] ?? key);
  }
  const names: Record<string, string> = {
    Primary: t("keys.ctrl"),
    Secondary: t("keys.win"),
    Alt: t("keys.alt"),
    Shift: t("keys.shift"),
    Backspace: t("keys.backspace"),
    Delete: t("keys.delete"),
  };
  const order = ["Primary", "Secondary", "Alt", "Shift"].filter((m) => mods.has(m));
  return [...order.map((m) => names[m]), names[key] ?? key].join("+");
}

/** Name des Dateimanagers für „Im … zeigen“: Finder, Explorer, Dateimanager. */
export function fileManager(platform: Platform = currentPlatform()): string {
  return t(platform === "mac" ? "files.finder" : platform === "windows" ? "files.explorer" : "files.manager");
}

/** Text mit Kürzel in Klammern, z. B. für Tooltips: „Rückgängig (⌘Z)“. */
export function withShortcut(label: string, spec: string): string {
  return `${label} (${formatShortcut(spec)})`;
}
