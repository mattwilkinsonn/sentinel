import { Navigation, Shield, Skull, Target, Trophy, Zap } from "lucide-solid";
import { type Component, createSignal, For, onCleanup, Show } from "solid-js";
import { Dynamic } from "solid-js/web";
import { LoadingState } from "../LoadingState";
import { Tooltip } from "../Tooltip";
import type { RawEvent, ThreatProfile } from "../types";

type FeedViewProps = {
  /** Pre-filtered events (name-resolution filter applied by the parent). */
  events: RawEvent[];
  profiles: ThreatProfile[];
  /** character_item_id → name lookup for event formatting. */
  names?: Record<string, string>;
  /** system_id → display name lookup. */
  systems?: Record<string, string>;
  loading?: boolean;
};

/** ID-to-name resolution helpers passed into event format functions. */
type Lookups = {
  /** Resolves a character ID to a display name; returns `"?"` for nullish input. */
  n: (id: unknown) => string;
  /** Resolves a system ID to a display name; falls back to the raw ID string. */
  sys: (id: unknown) => string;
};

/** Full rendering descriptor for one event type in the expanded feed view. */
type EventDisplay = {
  icon: Component<{ size?: number; class?: string }>;
  /** Tailwind text-colour utility applied to the icon and message text. */
  color: string;
  /** CSS colour value used for the left accent border on each event row. */
  borderColor: string;
  /** Generates the one-line human-readable summary for a given event payload. */
  format: (data: Record<string, unknown>, l: Lookups) => string;
};

/**
 * Full event-type registry for the FeedView. Compared to the sidebar
 * `SentinelFeed`, this version includes `score_change` and `gate_blocked`
 * entries with more descriptive format strings. Unknown types fall back to
 * the `kill` entry at render time.
 */
const eventConfig: Record<string, EventDisplay> = {
  kill: {
    icon: Skull,
    color: "text-accent-red",
    borderColor: "var(--color-accent-red)",
    format: (d, l) =>
      d.killed_by_structure
        ? `${l.n(d.killer_character_id)}'s ${d.structure_name ?? "Structure"} killed ${l.n(d.target_item_id)}`
        : `${l.n(d.killer_character_id)} killed ${l.n(d.target_item_id)}`,
  },
  structure_destroyed: {
    icon: Zap,
    color: "text-accent-orange",
    borderColor: "var(--color-accent-orange)",
    format: (d, l) =>
      d.killed_by_structure
        ? `${l.n(d.killer_character_id)}'s ${d.structure_name ?? "Structure"} destroyed a structure`
        : `${l.n(d.killer_character_id)} destroyed a structure`,
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
  bounty_stacked: {
    icon: Target,
    color: "text-accent-blue",
    borderColor: "var(--color-accent-blue)",
    format: (d, l) =>
      d.contributor_id
        ? `${l.n(d.contributor_id)} added to bounty on ${l.n(d.target_item_id)}`
        : `Bounty stacked on ${l.n(d.target_item_id)}`,
  },
  bounty_claimed: {
    icon: Trophy,
    color: "text-accent-green",
    borderColor: "var(--color-accent-green)",
    format: (d, l) =>
      `${l.n(d.hunter_id)} claimed bounty on ${l.n(d.target_item_id)}`,
  },
  score_change: {
    icon: Shield,
    color: "text-accent-gold",
    borderColor: "var(--color-accent-gold)",
    format: (d, l) =>
      `${l.n(d.character_id)} threat score updated to ${d.new_score ?? "?"}`,
  },
  gate_blocked: {
    icon: Zap,
    color: "text-text-primary",
    borderColor: "var(--color-text-primary)",
    format: (d, l) =>
      `${l.n(d.character_id)} blocked at gate — threat too high`,
  },
};

/**
 * Display order for the filter chip row above the event list. Each entry
 * maps to a key in `eventConfig` and carries singular/plural labels and a
 * tooltip explaining the event type to the user.
 */
const EVENT_ORDER = [
  {
    key: "kill",
    singular: "kill",
    plural: "kills",
    tooltip:
      "Player killed another player in combat. Click to filter events by kills.",
  },
  {
    key: "structure_destroyed",
    singular: "structure destroyed",
    plural: "structures destroyed",
    tooltip:
      "A structure was destroyed (SSU, gate, etc). Click to filter events by structure kills.",
  },
  {
    key: "jump",
    singular: "gate jump",
    plural: "gate jumps",
    tooltip:
      "Player used a Smart Gate to travel between systems. Click to filter events by gate jumps.",
  },
  {
    key: "bounty_posted",
    singular: "bounty posted",
    plural: "bounties posted",
    tooltip:
      "A reward was placed on a target's head. Click to filter events by bounty postings.",
  },
  {
    key: "bounty_stacked",
    singular: "bounty stacked",
    plural: "bounties stacked",
    tooltip:
      "Additional reward added to an existing bounty. Click to filter events by bounty stacks.",
  },
  {
    key: "bounty_removed",
    singular: "bounty removed",
    plural: "bounties removed",
    tooltip:
      "A bounty was cancelled or a contribution withdrawn. Click to filter events by bounty removals.",
  },
  {
    key: "bounty_claimed",
    singular: "bounty claimed",
    plural: "bounties claimed",
    tooltip:
      "A hunter killed their target and collected the reward. Click to filter events by bounty claims.",
  },
  {
    key: "score_change",
    singular: "score change",
    plural: "score changes",
    tooltip:
      "A pilot's threat score was recalculated based on recent activity. Click to filter events by score changes.",
  },
  {
    key: "gate_blocked",
    singular: "gate block",
    plural: "gate blocks",
    tooltip:
      "A high-threat pilot was denied passage through a sentinel-controlled gate. Click to filter events by gate blocks.",
  },
] as const;

/**
 * Returns a short relative-time string ("just now", "42s ago", "3m ago", etc.).
 * Resolves at second granularity for the first minute, then minute/hour/day.
 * Returns an empty string for a falsy timestamp.
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

/** Formats a millisecond timestamp as a locale-aware HH:MM:SS string for the event list column. */
function formatTime(timestampMs: number): string {
  if (!timestampMs) return "";
  const d = new Date(timestampMs);
  return d.toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

/**
 * Full-page intel feed with event-type filter chips and expandable event rows.
 * Displays up to 200 events per filter. Clicking a chip toggles filtering by
 * that event type; clicking again clears the filter. Clicking an event row
 * expands a raw-data detail panel showing all payload fields.
 *
 * Timestamps are refreshed every 5 seconds; events less than 5 seconds old
 * receive the `event-new` highlight class.
 */
export function FeedView(props: FeedViewProps) {
  const [filter, setFilter] = createSignal<string | null>(null);
  const [expandedIdx, setExpandedIdx] = createSignal<number | null>(null);
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

  const eventCounts = () => {
    const counts: Record<string, number> = {};
    for (const e of props.events) {
      counts[e.event_type] = (counts[e.event_type] || 0) + 1;
    }
    return counts;
  };

  const filteredEvents = () => {
    const f = filter();
    if (!f) return props.events;
    return props.events.filter((e) => e.event_type === f);
  };

  return (
    <div>
      <h3
        class="text-lg tracking-wider flex items-center gap-2"
        style="margin-bottom:1rem"
      >
        <div class="live-dot" />
        INTEL FEED
        <span class="text-text-muted text-sm">
          {(() => {
            const dayAgo = Date.now() - 86_400_000;
            const recent = props.events.filter(
              (e) => e.timestamp_ms >= dayAgo,
            ).length;
            const shown = filter()
              ? filteredEvents().length
              : props.events.length;
            const label = filter()
              ? `${shown} ${filter()?.replace("_", " ")}`
              : `${shown} total`;
            return `${label} · ${recent} last 24h`;
          })()}
        </span>
      </h3>

      {/* Filter cards */}
      <div
        class="grid grid-cols-3 lg:grid-cols-9 gap-2"
        style="margin-bottom:1.5rem"
      >
        {EVENT_ORDER.map((item) => {
          const config = eventConfig[item.key];
          const count = () => eventCounts()[item.key] || 0;
          const isActive = () => filter() === item.key;

          return (
            <Tooltip text={item.tooltip}>
              <button
                type="button"
                onClick={() => setFilter(isActive() ? null : item.key)}
                class={`glass-card p-2 flex flex-col items-center justify-center gap-1 bg-transparent transition-all w-full ${
                  isActive() ? "border-accent-cyan" : "border-border-default"
                }`}
                style="height:6.5rem"
              >
                <Dynamic
                  component={config.icon}
                  size={14}
                  class={config.color}
                />
                <span class={`text-lg font-bold ${config.color}`}>
                  {count()}
                </span>
                <span class="text-xs text-text-muted text-center">
                  {count() === 1 ? item.singular : item.plural}
                </span>
              </button>
            </Tooltip>
          );
        })}
      </div>

      {/* Event list */}
      <div class="flex flex-col gap-1">
        <For each={filteredEvents().slice(0, 200)}>
          {(event, i) => {
            const config = () =>
              eventConfig[event.event_type] ?? eventConfig.kill;
            const message = () =>
              config().format(event.data as Record<string, unknown>, lookups);
            const isNew = () => {
              tick();
              return Date.now() - event.timestamp_ms < 5_000;
            };
            const age = () => {
              tick();
              return timeAgo(event.timestamp_ms);
            };
            const isExpanded = () => expandedIdx() === i();

            return (
              <button
                type="button"
                class={`glass-card cursor-pointer transition-all bg-transparent border p-0 w-full text-left ${isNew() ? "event-new" : ""} ${isExpanded() ? "border-accent-cyan" : ""}`}
                style={`border-left:3px solid ${config().borderColor}`}
                onClick={() => setExpandedIdx(isExpanded() ? null : i())}
              >
                <div class="p-3 flex items-center gap-3">
                  <Dynamic
                    component={config().icon}
                    size={16}
                    class={`shrink-0 ${config().color}`}
                  />
                  <span class={`flex-1 text-sm ${config().color}`}>
                    {message()}
                  </span>
                  <span class="text-xs text-text-muted shrink-0">
                    {formatTime(event.timestamp_ms)}
                  </span>
                  <span class="text-xs text-text-muted shrink-0">{age()}</span>
                </div>
                <Show when={isExpanded()}>
                  <div class="border-t border-border-default px-3 pb-3 pt-2">
                    <div class="grid grid-cols-2 gap-2 text-xs">
                      {Object.entries(
                        event.data as Record<string, unknown>,
                      ).map(([key, val]) => {
                        const displayVal =
                          typeof val === "number"
                            ? (props.profiles.find(
                                (p) => p.character_item_id === val,
                              )?.name ?? String(val))
                            : String(val ?? "—");
                        return (
                          <div>
                            <span class="text-text-muted">
                              {key.replace(/_/g, " ")}:{" "}
                            </span>
                            <span class="text-text-primary">{displayVal}</span>
                          </div>
                        );
                      })}
                      <div>
                        <span class="text-text-muted">type: </span>
                        <span class={config().color}>
                          {event.event_type.replace(/_/g, " ")}
                        </span>
                      </div>
                      <div>
                        <span class="text-text-muted">time: </span>
                        <span class="text-text-primary">
                          {new Date(event.timestamp_ms).toLocaleString()}
                        </span>
                      </div>
                    </div>
                  </div>
                </Show>
              </button>
            );
          }}
        </For>
        <LoadingState
          loading={props.loading ?? false}
          hasData={filteredEvents().length > 0}
          loadingText="Connecting to event stream..."
          emptyText={
            filter()
              ? `No ${filter()?.replace("_", " ")} events yet`
              : "Waiting for events..."
          }
        />
      </div>
    </div>
  );
}
