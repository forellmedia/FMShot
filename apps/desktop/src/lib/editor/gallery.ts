// Galerie: Zeiträume und Gruppierung nach Tagen (ohne Oberfläche, testbar).

export type Period = "all" | "today" | "yesterday" | "week" | "month" | "year" | "custom";

export const PERIODS: Period[] = ["all", "today", "yesterday", "week", "month", "year", "custom"];

/** Mitternacht (lokale Zeit) des Tages von `ms`, plus `days` Tage. */
export function startOfDay(ms: number, days = 0): number {
  const d = new Date(ms);
  d.setHours(0, 0, 0, 0);
  d.setDate(d.getDate() + days);
  return d.getTime();
}

/**
 * Zeitraum als [von, bis) in Unix-Millisekunden; `null` = offen.
 * `custom` nimmt die Tage `from`/`to` (Format `YYYY-MM-DD`, beide inklusive).
 */
export function periodRange(
  period: Period,
  now: number,
  custom: { from: string; to: string } = { from: "", to: "" },
): { fromMs: number | null; toMs: number | null } {
  const today = startOfDay(now);
  switch (period) {
    case "today":
      return { fromMs: today, toMs: null };
    case "yesterday":
      return { fromMs: startOfDay(now, -1), toMs: today };
    case "week":
      return { fromMs: startOfDay(now, -6), toMs: null };
    case "month":
      return { fromMs: startOfDay(now, -29), toMs: null };
    case "year": {
      const d = new Date(now);
      return { fromMs: new Date(d.getFullYear(), 0, 1).getTime(), toMs: null };
    }
    case "custom": {
      const day = (s: string) => (s ? new Date(`${s}T00:00:00`).getTime() : null);
      const from = day(custom.from);
      const to = day(custom.to);
      return { fromMs: from, toMs: to == null ? null : startOfDay(to, 1) };
    }
    default:
      return { fromMs: null, toMs: null };
  }
}

export type DayGroup<T> = { key: string; label: string; items: T[] };

/**
 * Aufnahmen nach Tag gruppieren (Reihenfolge bleibt). Überschriften:
 * „Heute“, „Gestern“, sonst das Datum in der Sprache `locale`.
 */
export function groupByDay<T extends { createdAt: number | null }>(
  items: T[],
  now: number,
  locale: string,
  words: { today: string; yesterday: string; unknown: string },
): DayGroup<T>[] {
  const today = startOfDay(now);
  const yesterday = startOfDay(now, -1);
  const format = new Intl.DateTimeFormat(locale, { weekday: "long", day: "numeric", month: "long", year: "numeric" });
  const groups: DayGroup<T>[] = [];
  for (const item of items) {
    const day = item.createdAt == null ? null : startOfDay(item.createdAt);
    const key = day == null ? "?" : String(day);
    let group = groups[groups.length - 1];
    if (!group || group.key !== key) {
      const label =
        day == null ? words.unknown : day === today ? words.today : day === yesterday ? words.yesterday : format.format(day);
      group = { key, label, items: [] };
      groups.push(group);
    }
    group.items.push(item);
  }
  return groups;
}

/**
 * Welche Filter-Knöpfe in eine Zeile passen (Indizes, in Reihenfolge).
 * Passen alle, kommen alle; sonst so viele, dass daneben noch „Mehr…“
 * (`moreWidth`) Platz hat. Der gewählte Knopf (`keep`) bleibt immer sichtbar.
 */
export function fitInRow(widths: number[], available: number, gap: number, moreWidth: number, keep: number | null = null): number[] {
  const all = widths.map((_, i) => i);
  const total = widths.reduce((sum, w) => sum + w, 0) + gap * Math.max(0, widths.length - 1);
  if (total <= available) return all;
  let room = available - moreWidth - gap;
  if (keep != null && keep < widths.length) room -= widths[keep] + gap;
  const shown: number[] = [];
  for (const i of all) {
    if (i === keep) continue;
    if (widths[i] > room) break;
    shown.push(i);
    room -= widths[i] + gap;
  }
  if (keep != null && keep < widths.length) shown.push(keep);
  return shown.sort((a, b) => a - b);
}
