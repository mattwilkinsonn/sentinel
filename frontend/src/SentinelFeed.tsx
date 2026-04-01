import { Navigation, Shield, Skull, Target, Trophy, Zap } from "lucide-solid";
import type { JSX } from "solid-js";
import { type Component, createSignal, For, onCleanup } from "solid-js";
import { Dynamic } from "solid-js/web";
import type { RawEvent, ThreatProfile } from "./types";
import { getThreatColor, getThreatTier } from "./types";

type SentinelFeedProps = {
  /** Pre-filtered events to display (caller handles the name-resolution filter). */
  events: RawEvent[];
  profiles: ThreatProfile[];
  /** character_item_id → name; used for ID→name resolution before falling back to profiles. */
  names?: Record<string, string>;
  /** system_id → display name */
  systems?: Record<string, string>;
};

/**
 * Convenience closures passed into each `EventDisplay.format` function so they
 * can resolve raw IDs to display names without closing over the full props object.
 */
type Lookups = {
  /** Resolves a character ID (number or string) to a display name. Returns `"?"` for nullish input. */
  n: (id: unknown) => string;
  /** Resolves a system ID to a display name. Falls back to the raw ID string. */
  sys: (id: unknown) => string;
};

/** Rendering configuration for a single event type in the live feed. */
type EventDisplay = {
  icon: Component<{ size?: number; class?: string }>;
  /** Tailwind text-colour class applied to the message and icon. */
  color: string;
  /** CSS colour value used for the left accent border of each feed entry. */
  borderColor: string;
  /** Produces the human-readable summary line for one event payload. */
  format: (data: Record<string, unknown>, l: Lookups) => JSX.Element;
};

/**
 * Maps each known `event_type` string to its display configuration.
 * Unknown event types fall back to the `kill` entry at render time.
 */
const eventConfig: Record<string, EventDisplay> = {
  kill: {
    icon: Skull,
    color: "text-accent-red",
    borderColor: "var(--color-accent-red)",
    format: (d, l) =>
      d.killed_by_structure
        ? d.killer_character_id != null
          ? `${l.n(d.killer_character_id)}'s ${d.structure_name ?? "Structure"} killed ${l.n(d.target_item_id)}`
          : `${d.structure_name ?? "A structure"} killed ${l.n(d.target_item_id)}`
        : `${l.n(d.killer_character_id)} killed ${l.n(d.target_item_id)}`,
  },
  structure_destroyed: {
    icon: Zap,
    color: "text-accent-orange",
    borderColor: "var(--color-accent-orange)",
    format: (d, l) =>
      d.killed_by_structure
        ? d.killer_character_id != null
          ? `${l.n(d.killer_character_id)}'s ${d.structure_name ?? "Structure"} destroyed a structure`
          : `${d.structure_name ?? "A structure"} destroyed a structure`
        : `${l.n(d.killer_character_id)} destroyed a structure`,
  },
  bounty_posted: {
    icon: Target,
    color: "text-accent-cyan",
    borderColor: "var(--color-accent-cyan)",
    format: (d, l) =>
      d.poster_id
        ? `${l.n(d.poster_id)} posted bounty on ${l.n(d.target_item_id)}`
        : `Bounty posted on ${l.n(d.target_item_id)}`,
  },
  bounty_removed: {
    icon: Target,
    color: "text-text-muted",
    borderColor: "var(--color-text-muted)",
    format: (d, l) =>
      d.poster_id
        ? `${l.n(d.poster_id)} removed bounty on ${l.n(d.target_item_id)}`
        : `Bounty removed from ${l.n(d.target_item_id)}`,
  },
  jump: {
    icon: Navigation,
    color: "text-accent-purple",
    borderColor: "var(--color-accent-purple)",
    format: (d, l) =>
      d.source_gate && d.dest_gate
        ? `${l.n(d.character_id)} jumped ${d.source_gate} → ${d.dest_gate}`
        : `${l.n(d.character_id)} used smart gate`,
  },
  score_change: {
    icon: Shield,
    color: "text-accent-gold",
    borderColor: "var(--color-accent-gold)",
    format: (d, l) => {
      const score = d.new_score as number | null | undefined;
      const delta = d.delta as number | null | undefined;
      const oldScore = score != null && delta != null ? score - delta : null;
      const verb = delta != null && delta >= 0 ? "▲" : "▼";
      const verbColor =
        delta != null && delta >= 0
          ? "var(--color-accent-red)"
          : "var(--color-accent-green)";
      const fmt = (v: number) => (v / 100).toFixed(2);
      const colorFor = (v: number) => getThreatColor(getThreatTier(v));
      return (
        <span>
          {l.n(d.character_id)} score{" "}
          <span style={{ color: verbColor }}>{verb}</span>{" "}
          <span style={oldScore != null ? { color: colorFor(oldScore) } : {}}>
            {oldScore != null ? fmt(oldScore) : "?"}
          </span>
          {" → "}
          <span style={score != null ? { color: colorFor(score) } : {}}>
            {score != null ? fmt(score) : "?"}
          </span>
        </span>
      );
    },
  },
  bounty_stacked: {
    icon: Target,
    color: "text-accent-blue",
    borderColor: "var(--color-accent-blue)",
    format: (d, l) =>
      d.contributor_id
        ? `${l.n(d.contributor_id)} stacked on ${l.n(d.target_item_id)}`
        : `Bounty stacked on ${l.n(d.target_item_id)}`,
  },
  bounty_claimed: {
    icon: Trophy,
    color: "text-accent-green",
    borderColor: "var(--color-accent-green)",
    format: (d, l) =>
      `${l.n(d.hunter_id)} claimed bounty on ${l.n(d.target_item_id)}`,
  },
  gate_blocked: {
    icon: Zap,
    color: "text-text-primary",
    borderColor: "var(--color-text-primary)",
    format: (d, l) => `${l.n(d.character_id)} blocked at gate`,
  },
};

/**
 * Converts a millisecond timestamp to a short relative-time string ("just now",
 * "5m ago", etc.). Returns an empty string for falsy input (e.g. timestamp 0).
 */
function timeAgo(timestampMs: number): string {
  if (!timestampMs) return "";
  const diff = Date.now() - timestampMs;
  if (diff < 10_000) return "just now";
  if (diff < 60_000) return `${Math.floor(diff / 1000)}s ago`;
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
  return `${Math.floor(diff / 86_400_000)}d ago`;
}

/**
 * Compact sidebar feed showing up to 50 of the most recent intel events.
 * Timestamps are refreshed every 5 seconds via an interval tick signal so
 * relative ages stay current without re-fetching data.
 * Events newer than 5 seconds receive the `event-new` highlight class.
 */
export function SentinelFeed(props: SentinelFeedProps) {
  // Tick every 5s to keep relative timestamps fresh
  const [tick, setTick] = createSignal(0);
  const timer = setInterval(() => setTick((t) => t + 1), 5_000);
  onCleanup(() => clearInterval(timer));

  const lookups: Lookups = {
    n: (id: unknown): string => {
      if (id == null) return "?";
      const str = String(id);
      if (props.names?.[str]) return props.names[str];
      const numId = Number(id);
      const profile = props.profiles.find((p) => p.character_item_id === numId);
      return profile?.name || `Pilot #${numId}`;
    },
    sys: (id: unknown): string => {
      if (id == null) return "?";
      const str = String(id);
      return props.systems?.[str] || str || "?";
    },
  };

  return (
    <div class="glass-card p-4 h-full flex flex-col overflow-hidden">
      <h4
        class="text-sm tracking-wider flex items-center gap-2 text-text-secondary shrink-0"
        style="margin-bottom:1.25rem"
      >
        <div class="live-dot" />
        LIVE INTEL
      </h4>
      <div class="flex flex-col gap-1 overflow-hidden flex-1 min-h-0">
        <For
          each={props.events
            .filter(
              (e) =>
                e.event_type !== "score_change" ||
                Math.abs(
                  ((e.data as Record<string, unknown>).delta as number) ?? 0,
                ) >= 500,
            )
            .slice(0, 50)}
        >
          {(event) => {
            const config = () =>
              eventConfig[event.event_type] ?? eventConfig.kill;
            const message = () =>
              config().format(event.data as Record<string, unknown>, lookups);
            const age = () => {
              tick();
              return timeAgo(event.timestamp_ms);
            };
            const isNew = () => {
              tick();
              return Date.now() - event.timestamp_ms < 5_000;
            };

            return (
              <div
                class={`flex items-start gap-2 text-xs py-2 px-2 rounded ${isNew() ? "event-new" : ""}`}
                style={`border-left:2px solid ${config().borderColor}`}
              >
                <Dynamic
                  component={config().icon}
                  size={12}
                  class={`shrink-0 mt-0.5 ${config().color}`}
                />
                <div class="min-w-0">
                  <span class={config().color}>{message()}</span>
                  <div class="text-text-muted mt-0.5">{age()}</div>
                </div>
              </div>
            );
          }}
        </For>
        {props.events.length === 0 && (
          <p class="text-text-muted text-xs">Waiting for events...</p>
        )}
      </div>
    </div>
  );
}
