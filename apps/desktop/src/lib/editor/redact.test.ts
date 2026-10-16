import { describe, expect, it } from "vitest";
import { findSensitive } from "./redact";

const block = (text: string) => ({ text, x: 0, y: 0, width: text.length * 10, height: 20 });

describe("Auto-Schwärzen", () => {
  it("findet E-Mail und IBAN, nicht normalen Text", () => {
    expect(findSensitive([block("Kontakt: max.muster@example.com")]).length).toBe(1);
    expect(findSensitive([block("IBAN CH93 0076 2011 6238 5295 7")]).length).toBe(1);
    expect(findSensitive([block("Fehler 404 – Datei nicht gefunden")]).length).toBe(0);
  });

  it("Bereich liegt über dem Treffer", () => {
    const text = "Mail: a.b@c.de";
    const [r] = findSensitive([block(text)]);
    expect(r.x).toBeGreaterThanOrEqual(50);
    expect(r.x + r.width).toBeLessThanOrEqual(text.length * 10 + 5);
  });
});
