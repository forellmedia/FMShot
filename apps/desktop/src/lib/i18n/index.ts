// Übersetzungen der Oberfläche. Texte nie fest im Code – immer über t().
import de from "./de.json";
import en from "./en.json";

export type Locale = "de" | "en";
export type MessageKey = keyof typeof de;

const messages: Record<Locale, Record<MessageKey, string>> = { de, en };

/** Systemsprache, Fallback Englisch. */
export function detectLocale(): Locale {
  return navigator.language.toLowerCase().startsWith("de") ? "de" : "en";
}

let current: Locale = detectLocale();

export function setLocale(locale: Locale) {
  current = locale;
}

export function t(key: MessageKey): string {
  return messages[current][key] ?? messages.en[key] ?? key;
}
