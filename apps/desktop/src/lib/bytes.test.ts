import { describe, expect, it } from "vitest";
import { formatBytes, share } from "./bytes";

describe("formatBytes", () => {
  it("wählt die passende Einheit", () => {
    expect(formatBytes(0)).toBe("0 B");
    expect(formatBytes(12_345)).toBe("12 KB");
    expect(formatBytes(380_000_000)).toBe("380 MB");
    expect(formatBytes(7_040_000_000)).toBe("7,0 GB");
    expect(formatBytes(7_040_000_000, "en")).toBe("7.0 GB");
    expect(formatBytes(123_400_000_000)).toBe("123 GB");
  });
});

describe("share", () => {
  it("ohne Grenze kein Anteil", () => {
    expect(share(5, null)).toBe(0);
    expect(share(5, 10)).toBe(0.5);
  });
});
