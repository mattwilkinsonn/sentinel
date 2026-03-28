import { describe, it, expect, vi } from "vitest";
import { render, screen } from "@solidjs/testing-library";
import { StatsBar } from "../StatsBar";

describe("StatsBar", () => {
  const mockStats = {
    total_tracked: 42,
    avg_score: 6500,
    kills_24h: 18,
    top_system: "J-1042",
    events_per_min: 7,
  };

  it("renders all five stat cards", () => {
    render(() => (
      <StatsBar stats={mockStats} activeView="leaderboard" onStatClick={() => {}} />
    ));

    expect(screen.getByText("TRACKED")).toBeTruthy();
    expect(screen.getByText("AVG SCORE")).toBeTruthy();
    expect(screen.getByText("EVENTS/MIN")).toBeTruthy();
    expect(screen.getByText("KILLS 24H")).toBeTruthy();
    expect(screen.getByText("TOP SYSTEM")).toBeTruthy();
  });

  it("displays correct values", () => {
    render(() => (
      <StatsBar stats={mockStats} activeView="leaderboard" onStatClick={() => {}} />
    ));

    expect(screen.getByText("42")).toBeTruthy();
    expect(screen.getByText("65.00")).toBeTruthy();
    expect(screen.getByText("18")).toBeTruthy();
    expect(screen.getByText("J-1042")).toBeTruthy();
  });

  it("shows dash when no top system", () => {
    render(() => (
      <StatsBar
        stats={{ ...mockStats, top_system: "", events_per_min: 0 }}
        activeView="leaderboard"
        onStatClick={() => {}}
      />
    ));

    expect(screen.getByText("—")).toBeTruthy();
  });

  it("calls onStatClick with correct view", () => {
    const onClick = vi.fn();
    render(() => (
      <StatsBar stats={mockStats} activeView="leaderboard" onStatClick={onClick} />
    ));

    screen.getByText("TRACKED").closest("button")!.click();
    expect(onClick).toHaveBeenCalledWith("tracked");

    screen.getByText("KILLS 24H").closest("button")!.click();
    expect(onClick).toHaveBeenCalledWith("kills");

    screen.getByText("TOP SYSTEM").closest("button")!.click();
    expect(onClick).toHaveBeenCalledWith("systems");
  });
});
