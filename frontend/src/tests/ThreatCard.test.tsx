// Tests for ThreatCard: the detail panel rendered when a pilot is selected from the leaderboard
import { render, screen } from "@solidjs/testing-library";
import { describe, expect, it, vi } from "vitest";
import { ThreatCard } from "../ThreatCard";
import type { ThreatProfile } from "../types";

const mockProfile: ThreatProfile = {
  character_item_id: 88401,
  name: "Vex Nightburn",
  threat_score: 8500,
  kill_count: 247,
  death_count: 12,
  bounty_count: 5,
  last_kill_timestamp: Date.now() - 1_800_000,
  last_seen_system: "J-1042",
  last_seen_system_name: "Jita",
  tribe_id: "",
  tribe_name: "REAP",
  titles: ["Apex Predator", "Most Wanted"],
  threat_tier: "CRITICAL",
  recent_kills_24h: 8,
  recent_deaths_24h: 0,
  systems_visited: 15,
};

describe("ThreatCard", () => {
  it("renders character name", () => {
    render(() => <ThreatCard profile={mockProfile} onClose={() => {}} />);
    expect(screen.getByText("Vex Nightburn")).toBeTruthy();
  });

  it("shows CRITICAL tier for score 8500", () => {
    render(() => <ThreatCard profile={mockProfile} onClose={() => {}} />);
    expect(screen.getByText("CRITICAL THREAT")).toBeTruthy();
  });

  it("displays score as out of 100", () => {
    // Raw score 8500 is displayed normalised to a 0–100 scale (8500 / 100 = 85.00)
    render(() => <ThreatCard profile={mockProfile} onClose={() => {}} />);
    expect(screen.getByText("85.00")).toBeTruthy();
  });

  it("shows kill and death counts", () => {
    render(() => <ThreatCard profile={mockProfile} onClose={() => {}} />);
    expect(screen.getByText("247")).toBeTruthy();
    expect(screen.getByText("12")).toBeTruthy();
  });

  it("computes K/D ratio", () => {
    render(() => <ThreatCard profile={mockProfile} onClose={() => {}} />);
    expect(screen.getByText("20.58")).toBeTruthy();
  });

  it("shows bounty count", () => {
    render(() => <ThreatCard profile={mockProfile} onClose={() => {}} />);
    expect(screen.getByText("5")).toBeTruthy();
  });

  it("shows systems visited", () => {
    render(() => <ThreatCard profile={mockProfile} onClose={() => {}} />);
    expect(screen.getByText("15")).toBeTruthy();
  });

  it("shows last seen system", () => {
    render(() => <ThreatCard profile={mockProfile} onClose={() => {}} />);
    expect(screen.getByText("Jita")).toBeTruthy();
  });

  it("shows relative last kill time", () => {
    render(() => <ThreatCard profile={mockProfile} onClose={() => {}} />);
    expect(screen.getByText("30m ago")).toBeTruthy();
  });

  it("calls onClose when X is clicked", () => {
    const onClose = vi.fn();
    render(() => <ThreatCard profile={mockProfile} onClose={onClose} />);

    // Find the close button (contains X icon)
    const buttons = screen.getAllByRole("button");
    buttons[0].click();
    expect(onClose).toHaveBeenCalled();
  });

  it("handles zero deaths gracefully", () => {
    // K/D with 0 deaths should not divide-by-zero; displayed as kill_count (50 / 1 = 50.00)
    const profile = { ...mockProfile, death_count: 0, kill_count: 50 };
    render(() => <ThreatCard profile={profile} onClose={() => {}} />);
    expect(screen.getByText("50.00")).toBeTruthy();
  });

  it("shows Never for no kills", () => {
    const profile = { ...mockProfile, last_kill_timestamp: 0 };
    render(() => <ThreatCard profile={profile} onClose={() => {}} />);
    expect(screen.getByText("Never")).toBeTruthy();
  });
});
