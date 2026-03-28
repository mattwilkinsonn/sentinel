import { For, Show, createSignal, onCleanup, type Component } from "solid-js";
import { Dynamic } from "solid-js/web";
import {
  Skull,
  Target,
  Navigation,
  Shield,
  Zap,
  Trophy,
  UserPlus,
} from "lucide-solid";
import type { RawEvent, ThreatProfile } from "../types";
import { Tooltip } from "../Tooltip";
import { LoadingState } from "../LoadingState";

type FeedViewProps = {
  events: RawEvent[];
  profiles: ThreatProfile[];
  loading?: boolean;
};

type EventDisplay = {
  icon: Component<{ size?: number; class?: string }>;
  color: string;
  borderColor: string;
  format: (
    data: Record<string, unknown>,
    nameOf: (id: unknown) => string,
  ) => string;
};

const eventConfig: Record<string, EventDisplay> = {
  kill: {
    icon: Skull,
    color: "text-accent-red",
    borderColor: "var(--color-accent-red)",
    format: (d, n) =>
      `${n(d.killer_character_id)} killed ${n(d.target_item_id)}`,
  },
  jump: {
    icon: Navigation,
    color: "text-accent-purple",
    borderColor: "var(--color-accent-purple)",
    format: (d, n) =>
      `${n(d.character_id)} jumped to ${d.solar_system_id ?? "?"}`,
  },
  bounty_posted: {
    icon: Target,
    color: "text-accent-cyan",
    borderColor: "var(--color-accent-cyan)",
    format: (d, n) =>
      d.poster_id
        ? `${n(d.poster_id)} posted bounty on ${n(d.target_item_id)}`
        : `Bounty posted on ${n(d.target_item_id)}`,
  },
  bounty_removed: {
    icon: Target,
    color: "text-text-muted",
    borderColor: "var(--color-text-muted)",
    format: (d, n) =>
      d.poster_id
        ? `${n(d.poster_id)} removed bounty on ${n(d.target_item_id)}`
        : `Bounty removed from ${n(d.target_item_id)}`,
  },
  bounty_stacked: {
    icon: Target,
    color: "text-accent-blue",
    borderColor: "var(--color-accent-blue)",
    format: (d, n) =>
      d.contributor_id
        ? `${n(d.contributor_id)} added to bounty on ${n(d.target_item_id)}`
        : `Bounty stacked on ${n(d.target_item_id)}`,
  },
  bounty_claimed: {
    icon: Trophy,
    color: "text-accent-green",
    borderColor: "var(--color-accent-green)",
    format: (d, n) =>
      `${n(d.hunter_id)} claimed bounty on ${n(d.target_item_id)}`,
  },
  score_change: {
    icon: Shield,
    color: "text-accent-gold",
    borderColor: "var(--color-accent-gold)",
    format: (d, n) =>
      `${n(d.character_id)} threat score updated to ${d.new_score ?? "?"}`,
  },
  gate_blocked: {
    icon: Zap,
    color: "text-accent-orange",
    borderColor: "var(--color-accent-orange)",
    format: (d, n) => `${n(d.character_id)} blocked at gate — threat too high`,
  },
  new_character: {
    icon: UserPlus,
    color: "text-text-primary",
    borderColor: "var(--color-text-primary)",
    format: (d, n) => `New pilot detected: ${n(d.character_id)}`,
  },
};

const EVENT_ORDER = [
  {
    key: "kill",
    singular: "kill",
    plural: "kills",
    tooltip: "Player killed another player in combat",
  },
  {
    key: "jump",
    singular: "jump",
    plural: "jumps",
    tooltip: "Player moved between solar systems via a gate",
  },
  {
    key: "bounty_posted",
    singular: "bounty posted",
    plural: "bounties posted",
    tooltip: "A reward was placed on a target's head",
  },
  {
    key: "bounty_stacked",
    singular: "bounty stacked",
    plural: "bounties stacked",
    tooltip: "Additional reward added to an existing bounty by a contributor",
  },
  {
    key: "bounty_removed",
    singular: "bounty removed",
    plural: "bounties removed",
    tooltip: "A bounty was cancelled or a contribution withdrawn",
  },
  {
    key: "bounty_claimed",
    singular: "bounty claimed",
    plural: "bounties claimed",
    tooltip: "A hunter killed their target and collected the reward",
  },
  {
    key: "score_change",
    singular: "score change",
    plural: "score changes",
    tooltip: "A pilot's threat score was recalculated based on recent activity",
  },
  {
    key: "gate_blocked",
    singular: "gate block",
    plural: "gate blocks",
    tooltip:
      "A high-threat pilot was denied passage through a sentinel-controlled gate",
  },
  {
    key: "new_character",
    singular: "new pilot",
    plural: "new pilots",
    tooltip: "A previously unknown pilot was detected in the threat network",
  },
] as const;

function timeAgo(timestampMs: number): string {
  if (!timestampMs) return "";
  const diff = Date.now() - timestampMs;
  if (diff < 10_000) return "just now";
  if (diff < 60_000) return `${Math.floor(diff / 1000)}s ago`;
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
  return `${Math.floor(diff / 86_400_000)}d ago`;
}

function formatTime(timestampMs: number): string {
  if (!timestampMs) return "";
  const d = new Date(timestampMs);
  return d.toLocaleTimeString([], {
    hour: "2-digit",
    minute: "2-digit",
    second: "2-digit",
  });
}

export function FeedView(props: FeedViewProps) {
  const [filter, setFilter] = createSignal<string | null>(null);
  const [expandedIdx, setExpandedIdx] = createSignal<number | null>(null);
  const [tick, setTick] = createSignal(0);
  const timer = setInterval(() => setTick((t) => t + 1), 5_000);
  onCleanup(() => clearInterval(timer));

  const nameOf = (id: unknown): string => {
    if (id == null) return "?";
    const numId = typeof id === "number" ? id : Number(id);
    const profile = props.profiles.find((p) => p.character_item_id === numId);
    return profile?.name || `Pilot #${numId}`;
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
          {filter()
            ? `${filteredEvents().length} ${filter()!.replace("_", " ")} events`
            : `${props.events.length} events`}
        </span>
      </h3>

      {/* Filter cards */}
      <div
        class="grid grid-cols-4 lg:grid-cols-9 gap-2"
        style="margin-bottom:1.5rem"
      >
        {EVENT_ORDER.map((item) => {
          const config = eventConfig[item.key];
          const count = () => eventCounts()[item.key] || 0;
          const isActive = () => filter() === item.key;

          return (
            <Tooltip text={item.tooltip}>
              <button
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
              config().format(event.data as Record<string, unknown>, nameOf);
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
              <div
                class={`glass-card cursor-pointer transition-all ${isNew() ? "event-new" : ""} ${isExpanded() ? "border-accent-cyan" : ""}`}
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
              </div>
            );
          }}
        </For>
        <LoadingState
          loading={props.loading ?? false}
          hasData={filteredEvents().length > 0}
          loadingText="Connecting to event stream..."
          emptyText={
            filter()
              ? `No ${filter()!.replace("_", " ")} events yet`
              : "Waiting for events..."
          }
        />
      </div>
    </div>
  );
}
