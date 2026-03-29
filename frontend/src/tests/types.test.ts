// Tests for pure utility functions in types.ts: threat tier classification and colour mapping
import { describe, expect, it } from "vitest";
import { getThreatColor, getThreatColorClass, getThreatTier } from "../types";

// Verify exact boundary scores for each tier: LOW ≤2500, MODERATE 2501–5000, HIGH 5001–7500, CRITICAL >7500
describe("getThreatTier", () => {
  it("returns LOW for 0", () => {
    expect(getThreatTier(0)).toBe("LOW");
  });

  it("returns LOW for 2500", () => {
    expect(getThreatTier(2500)).toBe("LOW");
  });

  it("returns MODERATE for 2501", () => {
    expect(getThreatTier(2501)).toBe("MODERATE");
  });

  it("returns MODERATE for 5000", () => {
    expect(getThreatTier(5000)).toBe("MODERATE");
  });

  it("returns HIGH for 5001", () => {
    expect(getThreatTier(5001)).toBe("HIGH");
  });

  it("returns HIGH for 7500", () => {
    expect(getThreatTier(7500)).toBe("HIGH");
  });

  it("returns CRITICAL for 7501", () => {
    expect(getThreatTier(7501)).toBe("CRITICAL");
  });

  it("returns CRITICAL for 10000", () => {
    expect(getThreatTier(10000)).toBe("CRITICAL");
  });
});

describe("getThreatColor", () => {
  it("returns green for LOW", () => {
    expect(getThreatColor("LOW")).toBe("var(--color-threat-low)");
  });

  it("returns yellow for MODERATE", () => {
    expect(getThreatColor("MODERATE")).toBe("var(--color-threat-moderate)");
  });

  it("returns orange for HIGH", () => {
    expect(getThreatColor("HIGH")).toBe("var(--color-threat-high)");
  });

  it("returns red for CRITICAL", () => {
    expect(getThreatColor("CRITICAL")).toBe("var(--color-threat-critical)");
  });
});

describe("getThreatColorClass", () => {
  it("maps each tier to a tailwind class", () => {
    expect(getThreatColorClass("LOW")).toBe("text-threat-low");
    expect(getThreatColorClass("MODERATE")).toBe("text-threat-moderate");
    expect(getThreatColorClass("HIGH")).toBe("text-threat-high");
    expect(getThreatColorClass("CRITICAL")).toBe("text-threat-critical");
  });
});
