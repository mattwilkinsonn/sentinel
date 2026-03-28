import {
  Navigation,
  Shield,
  Skull,
  Target,
  Trophy,
  UserPlus,
  Zap,
} from "lucide-solid";
import { type Component, createSignal, For, onCleanup } from "solid-js";
import { Dynamic } from "solid-js/web";
import type { RawEvent, ThreatProfile } from "./types";

type SentinelFeedProps = {
  events: RawEvent[];
  profiles: ThreatProfile[];
  names?: Record<string, string>;
  systems?: Record<string, string>;
};

type Lookups = {
  n: (id: unknown) => string;
  sys: (id: unknown) => string;
};

type EventDisplay = {
  icon: Component<{ size?: number; class?: string }>;
  color: string;
  borderColor: string;
  format: (data: Record<string, unknown>, l: Lookups) => string;
};

const eventConfig: Record<string, EventDisplay> = {
  kill: {
    icon: Skull,
    color: "text-accent-red",
    borderColor: "var(--color-accent-red)",
    format: (d, l) =>
      `${l.n(d.killer_character_id)} killed ${l.n(d.target_item_id)}`,
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
      `${l.n(d.character_id)} jumped to ${l.sys(d.solar_system_id)}`,
  },
  score_change: {
    icon: Shield,
    color: "text-accent-gold",
    borderColor: "var(--color-accent-gold)",
    format: (d, l) => `${l.n(d.character_id)} score → ${d.new_score ?? "?"}`,
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
    color: "text-accent-orange",
    borderColor: "var(--color-accent-orange)",
    format: (d, l) => `${l.n(d.character_id)} blocked at gate`,
  },
  new_character: {
    icon: UserPlus,
    color: "text-text-primary",
    borderColor: "var(--color-text-primary)",
    format: (d, l) => `New pilot: ${l.n(d.character_id)}`,
  },
};

function timeAgo(timestampMs: number): string {
  if (!timestampMs) return "";
  const diff = Date.now() - timestampMs;
  if (diff < 10_000) return "just now";
  if (diff < 60_000) return `${Math.floor(diff / 1000)}s ago`;
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
  return `${Math.floor(diff / 86_400_000)}d ago`;
}

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
    <div class="glass-card p-4">
      <h4
        class="text-sm tracking-wider flex items-center gap-2 text-text-secondary"
        style="margin-bottom:1.25rem"
      >
        <div class="live-dot" />
        LIVE INTEL
      </h4>
      <div class="flex flex-col gap-1">
        <For each={props.events.slice(0, 15)}>
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
