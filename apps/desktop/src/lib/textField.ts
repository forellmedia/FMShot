/** Feld zum Tippen (Text, Suche, Zahl …) – dort bleibt das Menü des WebViews
 * zum Kopieren und Einsetzen. Kästchen, Schieber und Knöpfe zählen nicht. */
export function isTextField(target: EventTarget | null): boolean {
  if (!(target instanceof HTMLElement)) return false;
  if (target.isContentEditable || target instanceof HTMLTextAreaElement) return true;
  return (
    target instanceof HTMLInputElement &&
    ["text", "search", "email", "url", "tel", "number", "password"].includes(target.type)
  );
}
