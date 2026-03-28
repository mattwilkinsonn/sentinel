import { render, screen } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import { StatsBar } from "../StatsBar";
import type { ThreatProfile } from "../types";

describe("StatsBar", () => {
  const mockStats = {
    total_tracked: 42,
    avg_score: 6500,
    kills_24h: 18,
    top_system: "J-1042",
    total_events: 7,
  };

  const mockProfiles: ThreatProfile[] = [
    {
      character_item_id: 1,
      name: "Test",
      threat_score: 8500,
      kill_count: 10,
      death_count: 2,
      bounty_count: 0,
      last_kill_timestamp: 0,
      last_seen_system: "",
      last_seen_system_name: "",
      tribe_id: "",
      tribe_name: "",
      titles: [],
      threat_tier: "CRITICAL",
      recent_kills_24h: 3,
      systems_visited: 1,
    },
  ];

  it("renders all six stat cards", () => {
    render(() => (
      <StatsBar
        stats={mockStats}
        profiles={mockProfiles}
        newPilotCount={5}
        activeView="leaderboard"
        onStatClick={() => {}}
      />
    ));

    expect(screen.getByText("EVENTS 24H")).toBeTruthy();
    expect(screen.getByText("KILLS 24H")).toBeTruthy();
    expect(screen.getByText("ACTIVE THREATS")).toBeTruthy();
    expect(screen.getByText("TOP SYSTEM")).toBeTruthy();
    expect(screen.getByText("TRACKED")).toBeTruthy();
    expect(screen.getByText("NEW PILOTS 24H")).toBeTruthy();
  });

  it("displays correct values", () => {
    render(() => (
      <StatsBar
        stats={mockStats}
        profiles={mockProfiles}
        newPilotCount={5}
        activeView="leaderboard"
        onStatClick={() => {}}
      />
    ));

    expect(screen.getByText("42")).toBeTruthy();
    // 1 profile with score 8500 > 2500 threshold
    expect(screen.getByText("1")).toBeTruthy();
    expect(screen.getByText("18")).toBeTruthy();
    expect(screen.getByText("J-1042")).toBeTruthy();
  });

  it("shows dash when no top system", () => {
    render(() => (
      <StatsBar
        stats={{ ...mockStats, top_system: "", total_events: 0 }}
        profiles={[]}
        newPilotCount={0}
        activeView="leaderboard"
        onStatClick={() => {}}
      />
    ));

    expect(screen.getByText("—")).toBeTruthy();
  });

  it("calls onStatClick with correct view", () => {
    const onClick = vi.fn();
    render(() => (
      <StatsBar
        stats={mockStats}
        profiles={mockProfiles}
        newPilotCount={5}
        activeView="leaderboard"
        onStatClick={onClick}
      />
    ));

    screen.getByText("TRACKED").closest("button")?.click();
    expect(onClick).toHaveBeenCalledWith("tracked");

    screen.getByText("KILLS 24H").closest("button")?.click();
    expect(onClick).toHaveBeenCalledWith("kills");

    screen.getByText("TOP SYSTEM").closest("button")?.click();
    expect(onClick).toHaveBeenCalledWith("systems");
  });
});
