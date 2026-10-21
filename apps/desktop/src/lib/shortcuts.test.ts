import { afterEach, describe, expect, it } from "vitest";
import de from "$lib/i18n/de.json";
import en from "$lib/i18n/en.json";
import { setLocale } from "$lib/i18n";
import { formatShortcut } from "./shortcuts";

const MAC_SYMBOLS = /[⌘⌥⇧⌃⌫⌦]|\bCmd\b|\bCommand\b/;

describe("Kürzel je Plattform", () => {
  afterEach(() => setLocale("de"));

  it("macOS zeigt Symbole in der üblichen Reihenfolge", () => {
    expect(formatShortcut("Primary+Shift+Z", "mac")).toBe("⇧⌘Z");
    expect(formatShortcut("Primary+Alt+G", "mac")).toBe("⌥⌘G");
    expect(formatShortcut("Primary+Backspace", "mac")).toBe("⌘⌫");
  });

  it("Windows und Linux zeigen übersetzte Namen, nie Mac-Symbole", () => {
    setLocale("de");
    expect(formatShortcut("Primary+Shift+Z", "windows")).toBe("Strg+Umschalt+Z");
    expect(formatShortcut("Primary+Alt+G", "linux")).toBe("Strg+Alt+G");
    expect(formatShortcut("Delete", "windows")).toBe("Entf");
    setLocale("en");
    expect(formatShortcut("Primary+Shift+Z", "windows")).toBe("Ctrl+Shift+Z");
    for (const spec of ["Primary+Z", "Primary+Shift+S", "Primary+Alt+Shift+G", "Secondary+1", "Primary+,"]) {
      for (const platform of ["windows", "linux"] as const) {
        expect(formatShortcut(spec, platform)).not.toMatch(MAC_SYMBOLS);
      }
    }
  });

  it("Übersetzungen enthalten keine fest eingebauten Mac-Kürzel", () => {
    // Nur Texte, die ausschliesslich auf macOS erscheinen, dürfen Symbole haben.
    const macOnly = new Set(["settings.replace_system", "settings.replace_system_done"]);
    for (const [lang, table] of Object.entries({ de, en })) {
      for (const [key, text] of Object.entries(table)) {
        if (macOnly.has(key)) continue;
        expect(text, `${lang}:${key}`).not.toMatch(MAC_SYMBOLS);
      }
    }
  });
});
