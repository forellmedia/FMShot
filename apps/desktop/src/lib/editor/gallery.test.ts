import { describe, expect, it } from "vitest";
import { fitInRow, groupByDay, periodRange, startOfDay } from "./gallery";

// Freitag, 16. Oktober 2026, 15:30 Ortszeit.
const now = new Date(2026, 9, 16, 15, 30).getTime();
const at = (day: number, hour = 12) => new Date(2026, 9, day, hour).getTime();
const words = { today: "Heute", yesterday: "Gestern", unknown: "?" };

describe("Zeiträume", () => {
  it("heute, gestern, letzte 7 Tage", () => {
    expect(periodRange("today", now)).toEqual({ fromMs: at(16, 0), toMs: null });
    expect(periodRange("yesterday", now)).toEqual({ fromMs: at(15, 0), toMs: at(16, 0) });
    expect(periodRange("week", now).fromMs).toBe(at(10, 0));
    expect(periodRange("all", now)).toEqual({ fromMs: null, toMs: null });
  });

  it("eigener Zeitraum schliesst den letzten Tag ein", () => {
    const r = periodRange("custom", now, { from: "2026-10-01", to: "2026-10-03" });
    expect(r).toEqual({ fromMs: at(1, 0), toMs: at(4, 0) });
    expect(periodRange("custom", now, { from: "", to: "2026-10-03" }).fromMs).toBeNull();
  });
});

describe("Nach Tagen gruppieren", () => {
  it("Heute, Gestern, dann Datum – Reihenfolge bleibt", () => {
    const items = [{ createdAt: at(16, 9) }, { createdAt: at(16, 8) }, { createdAt: at(15) }, { createdAt: at(1) }];
    const groups = groupByDay(items, now, "de-CH", words);
    expect(groups.map((g) => [g.label, g.items.length])).toEqual([
      ["Heute", 2],
      ["Gestern", 1],
      ["Donnerstag, 1. Oktober 2026", 1],
    ]);
    expect(groups[0].key).toBe(String(startOfDay(now)));
  });
});

describe("Filter in einer Zeile", () => {
  const w = [40, 100, 100, 100, 100];

  it("alle, wenn sie passen", () => {
    expect(fitInRow(w, 500, 6, 60)).toEqual([0, 1, 2, 3, 4]);
  });

  it("sonst so viele, dass „Mehr…“ daneben passt", () => {
    // 40 + 6 + 100 + 6 + 100 = 252; dazu 6 + 60 = 318 ≤ 330
    expect(fitInRow(w, 330, 6, 60)).toEqual([0, 1, 2]);
  });

  it("der gewählte Filter bleibt sichtbar", () => {
    expect(fitInRow(w, 330, 6, 60, 4)).toEqual([0, 1, 4]);
  });
});
