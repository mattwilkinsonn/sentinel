import { For, Show } from "solid-js";
import { LoadingState } from "../LoadingState";
import type { ThreatProfile } from "../types";
import { getThreatColor, getThreatColorClass, getThreatTier } from "../types";

type TrackedViewProps = {
  profiles: ThreatProfile[];
  newPilotCount: number;
  onSelect: (id: number) => void;
  onViewPilots: () => void;
  loading?: boolean;
};

export function TrackedView(props: TrackedViewProps) {
  const sevenDaysAgo = () => Date.now() - 7 * 86_400_000;

  const active = () =>
    props.profiles.filter(
      (p) =>
        p.last_kill_timestamp >= sevenDaysAgo() ||
        p.kill_count > 0 ||
        p.death_count > 0 ||
        p.bounty_count > 0,
    );

  const sorted = () =>
    [...active()].sort((a, b) => b.threat_score - a.threat_score);

  const tierCounts = () => {
    const counts = { LOW: 0, MODERATE: 0, HIGH: 0, CRITICAL: 0 };
    for (const p of active()) {
      counts[getThreatTier(p.threat_score)]++;
    }
    return counts;
  };

  return (
    <div>
      <div class="flex items-center justify-between" style="margin-bottom:1rem">
        <h3 class="text-lg tracking-wider">
          ACTIVE PILOTS
          <span class="text-text-muted text-sm ml-2">
            {active().length} of {props.profiles.length} tracked
          </span>
        </h3>
        <button
          type="button"
          class="flex items-center gap-2 px-3 py-1.5 rounded text-xs bg-transparent border border-border-default text-text-secondary hover:text-text-primary hover:border-border-hover transition-all"
          onClick={props.onViewPilots}
        >
          NEW PILOTS
          <span class="text-text-muted">({props.newPilotCount})</span>
        </button>
      </div>

      <LoadingState
        loading={props.loading ?? false}
        hasData={props.profiles.length > 0}
        loadingText="Loading tracked characters..."
        emptyText="No tracked characters yet. Waiting for on-chain events..."
      />

      <Show when={props.profiles.length > 0}>
        {/* Tier breakdown */}
        <div class="grid grid-cols-4 gap-3 mb-6">
          {(["CRITICAL", "HIGH", "MODERATE", "LOW"] as const).map((tier) => (
            <div class="glass-card p-3 text-center">
              <div class={`text-2xl font-bold ${getThreatColorClass(tier)}`}>
                {tierCounts()[tier]}
              </div>
              <div class="text-xs text-text-muted">{tier}</div>
              <div
                class="h-1 rounded mt-2"
                style={{ background: getThreatColor(tier) }}
              />
            </div>
          ))}
        </div>

        {/* Full list */}
        <div class="flex flex-col gap-1">
          <For each={sorted()}>
            {(profile) => {
              const tier = () => getThreatTier(profile.threat_score);
              const color = () => getThreatColor(tier());
              const colorClass = () => getThreatColorClass(tier());

              return (
                <button
                  type="button"
                  onClick={() => props.onSelect(profile.character_item_id)}
                  class="glass-card p-3 w-full text-left bg-transparent flex items-center gap-3"
                >
                  <div
                    class="w-2 h-2 rounded-full shrink-0"
                    style={{ background: color() }}
                  />
                  <span class="text-sm text-text-primary flex-1">
                    {profile.name || `Pilot #${profile.character_item_id}`}
                  </span>
                  <span
                    class={`badge ${colorClass()}`}
                    style={{ background: `${color()}15` }}
                  >
                    {tier()}
                  </span>
                  <span class={`text-sm font-bold ${colorClass()}`}>
                    {(profile.threat_score / 100).toFixed(2)}
                  </span>
                  <span class="text-xs text-text-muted">
                    {profile.kill_count}K / {profile.death_count}D
                  </span>
                </button>
              );
            }}
          </For>
        </div>
      </Show>
    </div>
  );
}
