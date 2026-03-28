import { Clock, MapPin, Shield, Skull, Target } from "lucide-solid";
import { createSignal, For, Show } from "solid-js";
import { LoadingState } from "../LoadingState";
import type { ThreatProfile } from "../types";

type KillsViewProps = {
  profiles: ThreatProfile[];
  onSelect: (id: number) => void;
  selectedId: number | null;
  loading?: boolean;
};

export function KillsView(props: KillsViewProps) {
  const byKills = () =>
    [...props.profiles].sort((a, b) => b.kill_count - a.kill_count);
  const byRecent = () =>
    [...props.profiles].sort((a, b) => b.recent_kills_24h - a.recent_kills_24h);
  const totalKills = () =>
    props.profiles.reduce((sum, p) => sum + p.kill_count, 0);
  const totalRecent = () =>
    props.profiles.reduce((sum, p) => sum + p.recent_kills_24h, 0);

  const [recentExpanded, setRecentExpanded] = createSignal<number | null>(null);
  const [alltimeExpanded, setAlltimeExpanded] = createSignal<number | null>(
    null,
  );

  return (
    <div>
      <h3 class="text-lg tracking-wider" style="margin-bottom:1rem">
        KILL STATISTICS
        <span class="text-text-muted text-sm ml-2">
          {totalKills()} total · {totalRecent()} last 24h
        </span>
      </h3>

      <LoadingState
        loading={props.loading ?? false}
        hasData={props.profiles.length > 0}
        loadingText="Loading kill statistics..."
        emptyText="No kill data yet. Waiting for combat events..."
      />

      <Show when={props.profiles.length > 0}>
        {/* Most Active (24H) */}
        <div class="glass-card p-5 mb-6">
          <div class="flex items-center gap-2 mb-4">
            <Clock size={16} class="text-accent-red" />
            <h4 class="text-sm tracking-wider text-accent-red">
              MOST ACTIVE (24H)
            </h4>
          </div>
          <div class="flex flex-col gap-2">
            <For each={byRecent().slice(0, 10)}>
              {(profile, i) => {
                const isExp = () =>
                  recentExpanded() === profile.character_item_id;
                return (
                  <ExpandableRow
                    profile={profile}
                    rank={i() + 1}
                    isExpanded={isExp()}
                    onClick={() =>
                      setRecentExpanded(
                        isExp() ? null : profile.character_item_id,
                      )
                    }
                    statLabel="kills 24h"
                    statValue={profile.recent_kills_24h}
                    statColor="text-accent-red"
                    extra={
                      <span class="text-xs text-text-muted">
                        ({profile.kill_count} total)
                      </span>
                    }
                  />
                );
              }}
            </For>
          </div>
        </div>

        {/* All-Time Kills */}
        <div class="glass-card p-5">
          <div class="flex items-center gap-2 mb-4">
            <Skull size={16} class="text-accent-gold" />
            <h4 class="text-sm tracking-wider text-accent-gold">
              ALL-TIME KILLS
            </h4>
          </div>
          <div class="flex flex-col gap-2">
            <For each={byKills().slice(0, 10)}>
              {(profile, i) => {
                const isExp = () =>
                  alltimeExpanded() === profile.character_item_id;
                const kd = () =>
                  profile.death_count > 0
                    ? (profile.kill_count / profile.death_count).toFixed(1)
                    : profile.kill_count.toFixed(1);
                return (
                  <ExpandableRow
                    profile={profile}
                    rank={i() + 1}
                    isExpanded={isExp()}
                    onClick={() =>
                      setAlltimeExpanded(
                        isExp() ? null : profile.character_item_id,
                      )
                    }
                    statLabel="kills"
                    statValue={profile.kill_count}
                    statColor="text-accent-gold"
                    extra={
                      <>
                        <span class="text-xs text-text-muted">K/D {kd()}</span>
                        <span class="text-xs text-text-muted">
                          {profile.death_count} deaths
                        </span>
                      </>
                    }
                  />
                );
              }}
            </For>
          </div>
        </div>
      </Show>
    </div>
  );
}

import type { JSX } from "solid-js";

function ExpandableRow(props: {
  profile: ThreatProfile;
  rank: number;
  isExpanded: boolean;
  onClick: () => void;
  statLabel: string;
  statValue: number;
  statColor: string;
  extra?: JSX.Element;
}) {
  const p = props.profile;
  const kd = () =>
    p.death_count > 0
      ? (p.kill_count / p.death_count).toFixed(1)
      : p.kill_count.toFixed(1);
  const lastKillAgo = () => {
    if (!p.last_kill_timestamp) return "Never";
    const diff = Date.now() - p.last_kill_timestamp;
    if (diff < 60_000) return "Just now";
    if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
    if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
    return `${Math.floor(diff / 86_400_000)}d ago`;
  };

  return (
    <button
      type="button"
      class={`rounded border transition-all cursor-pointer bg-transparent p-0 w-full text-left ${
        props.isExpanded
          ? "border-accent-cyan"
          : "border-border-default hover:border-border-hover"
      }`}
      onClick={props.onClick}
    >
      <div class="flex items-center gap-3 p-3">
        <span class="text-text-muted text-xs w-6 text-right font-bold">
          #{props.rank}
        </span>
        <span class="text-sm text-text-primary flex-1">
          {p.name || `Pilot #${p.character_item_id}`}
        </span>
        <span class={`${props.statColor} font-bold text-lg`}>
          {props.statValue}
        </span>
        <span class="text-xs text-text-muted">{props.statLabel}</span>
        {props.extra}
      </div>

      <Show when={props.isExpanded}>
        <div class="border-t border-border-default px-3 pb-3 pt-3">
          <div class="grid grid-cols-3 gap-3">
            <StatItem
              icon={Skull}
              label="KILLS"
              value={p.kill_count.toString()}
              color="text-accent-red"
            />
            <StatItem
              icon={Shield}
              label="DEATHS"
              value={p.death_count.toString()}
              color="text-text-secondary"
            />
            <StatItem
              icon={Skull}
              label="K/D RATIO"
              value={kd()}
              color="text-accent-gold"
            />
            <StatItem
              icon={Target}
              label="BOUNTIES"
              value={p.bounty_count.toString()}
              color="text-accent-cyan"
            />
            <StatItem
              icon={MapPin}
              label="SYSTEMS"
              value={p.systems_visited.toString()}
              color="text-accent-purple"
            />
            <StatItem
              icon={Clock}
              label="LAST KILL"
              value={lastKillAgo()}
              color="text-text-muted"
            />
          </div>
          {p.last_seen_system && (
            <div class="mt-2 text-xs text-text-muted">
              Last seen:{" "}
              <span class="text-text-secondary">{p.last_seen_system_name || p.last_seen_system}</span>
            </div>
          )}
        </div>
      </Show>
    </button>
  );
}

function StatItem(props: {
  icon: typeof Skull;
  label: string;
  value: string;
  color: string;
}) {
  return (
    <div class="flex items-center gap-2">
      <props.icon size={14} class={props.color} />
      <div>
        <div class="text-xs text-text-muted">{props.label}</div>
        <div class={`text-sm font-bold ${props.color}`}>{props.value}</div>
      </div>
    </div>
  );
}
