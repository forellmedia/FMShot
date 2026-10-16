import { describe, expect, it } from "vitest";
import { snap } from "./snap";

describe("Einrasten", () => {
  it("rastet an der Mitte der Zeichenfläche ein", () => {
    const board = { x: 0, y: 0, width: 200, height: 100 };
    const moving = { x: 77, y: 10, width: 40, height: 20 }; // Mitte bei 97
    const s = snap(moving, [board], 5);
    expect(s.dx).toBe(3);
    expect(s.guides.x).toEqual([100]);
  });

  it("ausserhalb der Toleranz nichts", () => {
    const s = snap({ x: 50, y: 50, width: 10, height: 10 }, [{ x: 0, y: 0, width: 10, height: 10 }], 2);
    expect(s).toEqual({ dx: 0, dy: 0, guides: { x: [], y: [] } });
  });
});
