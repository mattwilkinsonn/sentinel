import { render, screen } from "@solidjs/testing-library";
import { describe, expect, it } from "vitest";
import { SentinelFeed } from "../SentinelFeed";
import type { RawEvent, ThreatProfile } from "../types";

const defaults: ThreatProfile = {
  character_item_id: 0,
  name: "",
  threat_score: 0,
  kill_count: 0,
  death_count: 0,
  bounty_count: 0,
  last_kill_timestamp: 0,
  last_seen_system: "",
  last_seen_system_name: "",
  tribe_id: "",
  tribe_name: "",
  titles: [],
  threat_tier: "LOW",
  recent_kills_24h: 0,
  systems_visited: 0,
};

const mockProfiles: ThreatProfile[] = [
  {
    ...defaults,
    character_item_id: 123,
    name: "Vex Nightburn",
    threat_score: 8000,
    kill_count: 100,
    death_count: 10,
    bounty_count: 3,
    recent_kills_24h: 5,
    systems_visited: 3,
  },
  {
    ...defaults,
    character_item_id: 456,
    name: "Kira Ashfall",
    threat_score: 5000,
    kill_count: 50,
    death_count: 20,
    bounty_count: 1,
    recent_kills_24h: 2,
    systems_visited: 5,
  },
  {
    ...defaults,
    character_item_id: 789,
    name: "Dread Solaris",
    threat_score: 3000,
    kill_count: 30,
    death_count: 15,
    recent_kills_24h: 1,
    systems_visited: 8,
  },
];

describe("SentinelFeed", () => {
  it("shows waiting message when no events", () => {
    render(() => <SentinelFeed events={[]} profiles={[]} />);
    expect(screen.getByText("Waiting for events...")).toBeTruthy();
  });

  it("renders kill events with character names", () => {
    const events: RawEvent[] = [
      {
        event_type: "kill",
        timestamp_ms: Date.now() - 30_000,
        data: { killer_character_id: 123, target_item_id: 456 },
      },
    ];

    render(() => <SentinelFeed events={events} profiles={mockProfiles} />);
    expect(screen.getByText("Vex Nightburn killed Kira Ashfall")).toBeTruthy();
  });

  it("renders jump events with character name and system", () => {
    const events: RawEvent[] = [
      {
        event_type: "jump",
        timestamp_ms: Date.now() - 120_000,
        data: {
          character_id: 789,
          source_gate: "ALPHA-1",
          dest_gate: "BETA-2",
        },
      },
    ];

    render(() => <SentinelFeed events={events} profiles={mockProfiles} />);
    expect(
      screen.getByText("Dread Solaris jumped ALPHA-1 → BETA-2"),
    ).toBeTruthy();
  });

  it("renders bounty posted events with name", () => {
    const events: RawEvent[] = [
      {
        event_type: "bounty_posted",
        timestamp_ms: Date.now() - 60_000,
        data: { target_item_id: 456 },
      },
    ];

    render(() => <SentinelFeed events={events} profiles={mockProfiles} />);
    expect(screen.getByText("Bounty posted on Kira Ashfall")).toBeTruthy();
  });

  it("falls back to Pilot # for unknown characters", () => {
    const events: RawEvent[] = [
      {
        event_type: "kill",
        timestamp_ms: Date.now() - 30_000,
        data: { killer_character_id: 999, target_item_id: 456 },
      },
    ];

    render(() => <SentinelFeed events={events} profiles={mockProfiles} />);
    expect(screen.getByText("Pilot #999 killed Kira Ashfall")).toBeTruthy();
  });

  it("shows LIVE INTEL FEED header", () => {
    render(() => <SentinelFeed events={[]} profiles={[]} />);
    expect(screen.getByText("LIVE INTEL")).toBeTruthy();
  });

  it("limits display to 50 events", () => {
    const events: RawEvent[] = Array.from({ length: 60 }, (_, i) => ({
      event_type: "kill",
      timestamp_ms: Date.now() - i * 1000,
      data: { killer_character_id: i, target_item_id: i + 100 },
    }));

    render(() => <SentinelFeed events={events} profiles={[]} />);
    // Sidebar caps at 50 items
    const items = screen.getAllByText(/killed/);
    expect(items.length).toBe(50);
  });

  it("shows relative timestamps", () => {
    const events: RawEvent[] = [
      {
        event_type: "kill",
        timestamp_ms: Date.now() - 1_000,
        data: { killer_character_id: 1, target_item_id: 2 },
      },
    ];

    render(() => <SentinelFeed events={events} profiles={[]} />);
    expect(screen.getByText("just now")).toBeTruthy();
  });
});
