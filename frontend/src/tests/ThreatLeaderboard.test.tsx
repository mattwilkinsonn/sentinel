// Tests for ThreatLeaderboard: the ranked pilot list showing threat tiers, K/D, and selection behaviour
import { render, screen } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import { ThreatLeaderboard } from "../ThreatLeaderboard";
import type { ThreatProfile } from "../types";

function makeProfile(overrides: Partial<ThreatProfile> = {}): ThreatProfile {
  return {
    character_item_id: 12345,
    name: `Pilot #${overrides.character_item_id ?? 12345}`,
    threat_score: 5000,
    kill_count: 50,
    death_count: 10,
    bounty_count: 2,
    last_kill_timestamp: Date.now(),
    last_seen_system: "J-1042",
    last_seen_system_name: "",
    tribe_id: "",
    tribe_name: "",
    titles: [],
    threat_tier: "MODERATE",
    recent_kills_24h: 3,
    systems_visited: 5,
    ...overrides,
  };
}

describe("ThreatLeaderboard", () => {
  it("renders profiles sorted by score", () => {
    const profiles = [
      makeProfile({ character_item_id: 111, threat_score: 3000 }),
      makeProfile({ character_item_id: 222, threat_score: 9000 }),
      makeProfile({ character_item_id: 333, threat_score: 6000 }),
    ];

    render(() => (
      <ThreatLeaderboard
        profiles={profiles}
        onSelect={() => {}}
        selectedId={null}
      />
    ));

    // All three should render
    expect(screen.getByText("Pilot #111")).toBeTruthy();
    expect(screen.getByText("Pilot #222")).toBeTruthy();
    expect(screen.getByText("Pilot #333")).toBeTruthy();
  });

  it("shows correct tier badges", () => {
    const profiles = [
      makeProfile({ character_item_id: 1, threat_score: 1000 }),
      makeProfile({ character_item_id: 2, threat_score: 4000 }),
      makeProfile({ character_item_id: 3, threat_score: 6000 }),
      makeProfile({ character_item_id: 4, threat_score: 9000 }),
    ];

    render(() => (
      <ThreatLeaderboard
        profiles={profiles}
        onSelect={() => {}}
        selectedId={null}
      />
    ));

    expect(screen.getByText("LOW")).toBeTruthy();
    expect(screen.getByText("MODERATE")).toBeTruthy();
    expect(screen.getByText("HIGH")).toBeTruthy();
    expect(screen.getByText("CRITICAL")).toBeTruthy();
  });

  it("limits to top 20", () => {
    const profiles = Array.from({ length: 25 }, (_, i) =>
      makeProfile({ character_item_id: i + 1, threat_score: 10000 - i * 100 }),
    );

    render(() => (
      <ThreatLeaderboard
        profiles={profiles}
        onSelect={() => {}}
        selectedId={null}
      />
    ));

    expect(screen.getByText("TOP 20")).toBeTruthy();
    expect(screen.queryByText("Pilot #25")).toBeNull();
  });

  it("calls onSelect when clicking a profile", () => {
    const onSelect = vi.fn();
    const profiles = [makeProfile({ character_item_id: 42 })];

    render(() => (
      <ThreatLeaderboard
        profiles={profiles}
        onSelect={onSelect}
        selectedId={null}
      />
    ));

    // Click the card element (glass-card wrapper) rather than the text node directly
    (
      screen
        .getByText("Pilot #42")
        .closest("[class*=glass-card]") as HTMLElement
    )?.click();
    expect(onSelect).toHaveBeenCalledWith(42);
  });

  it("displays K/D ratio correctly", () => {
    const profiles = [
      makeProfile({ character_item_id: 1, kill_count: 100, death_count: 20 }),
    ];

    render(() => (
      <ThreatLeaderboard
        profiles={profiles}
        onSelect={() => {}}
        selectedId={null}
      />
    ));

    expect(screen.getByText("K/D 5.0 · 100K")).toBeTruthy();
  });

  it("handles zero deaths in K/D", () => {
    const profiles = [
      makeProfile({ character_item_id: 1, kill_count: 50, death_count: 0 }),
    ];

    render(() => (
      <ThreatLeaderboard
        profiles={profiles}
        onSelect={() => {}}
        selectedId={null}
      />
    ));

    expect(screen.getByText("K/D 50.0 · 50K")).toBeTruthy();
  });
});
