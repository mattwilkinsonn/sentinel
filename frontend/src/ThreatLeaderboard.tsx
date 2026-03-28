import { Clock, MapPin, Shield, Skull, Target } from "lucide-solid";
import { For, Show } from "solid-js";
import type { ThreatProfile } from "./types";
import {
  computeBreakdown,
  getThreatColor,
  getThreatColorClass,
  getThreatTier,
} from "./types";

type ThreatLeaderboardProps = {
  profiles: ThreatProfile[];
  onSelect: (id: number) => void;
  selectedId: number | null;
};

export function ThreatLeaderboard(props: ThreatLeaderboardProps) {
  const top20 = () => props.profiles.slice(0, 20);

  return (
    <div>
      <h3
        class="text-lg tracking-wider flex items-center gap-2"
        style="margin-bottom:1rem"
      >
        THREAT LEADERBOARD
        <span class="text-text-muted text-sm">
          TOP {Math.min(20, props.profiles.length)}
        </span>
      </h3>

      <div class="flex flex-col gap-2">
        <For each={top20()}>
          {(profile, i) => {
            const tier = () => getThreatTier(profile.threat_score);
            const color = () => getThreatColor(tier());
            const colorClass = () => getThreatColorClass(tier());
            const pct = () => (profile.threat_score / 10000) * 100;
            const isSelected = () =>
              props.selectedId === profile.character_item_id;
            const kd = () =>
              profile.death_count > 0
                ? (profile.kill_count / profile.death_count).toFixed(1)
                : profile.kill_count > 0
                  ? `${profile.kill_count}.0`
                  : "0.0";

            const lastKillAgo = () => {
              if (!profile.last_kill_timestamp) return "Never";
              const diff = Date.now() - profile.last_kill_timestamp;
              if (diff < 60_000) return "Just now";
              if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
              if (diff < 86_400_000)
                return `${Math.floor(diff / 3_600_000)}h ago`;
              return `${Math.floor(diff / 86_400_000)}d ago`;
            };

            return (
              <button
                type="button"
                class={`glass-card w-full text-left border transition-all cursor-pointer bg-transparent p-0 ${
                  isSelected()
                    ? "border-accent-cyan"
                    : "border-border-default hover:border-border-hover"
                } ${tier() === "CRITICAL" ? "neon-glow-threat" : ""}`}
                onClick={() => props.onSelect(profile.character_item_id)}
              >
                {/* Summary row */}
                <div class="flex items-center gap-3 p-3">
                  <span class="text-text-muted text-xs w-6 text-right shrink-0">
                    #{i() + 1}
                  </span>
                  <div class="flex-1 min-w-0">
                    <div class="flex items-center gap-2 mb-1 flex-wrap">
                      <span class="text-sm font-bold text-text-primary">
                        {profile.name || `Pilot #${profile.character_item_id}`}
                      </span>
                      {profile.tribe_name && (
                        <span class="text-xs text-text-muted">[{profile.tribe_name}]</span>
                      )}
                      <span
                        class={`badge ${colorClass()}`}
                        style={{ background: `${color()}15` }}
                      >
                        {tier()}
                      </span>
                      {profile.titles?.slice(0, 2).map((title) => (
                        <span
                          class="text-[10px] px-1.5 py-0.5 rounded border border-border-default text-text-muted"
                        >
                          {title}
                        </span>
                      ))}
                    </div>
                    <div style="height:6px;border-radius:3px;background:rgba(250,250,229,0.08);overflow:hidden">
                      <div
                        style={`width:${pct()}%;height:6px;border-radius:3px;background:${color()}`}
                      />
                    </div>
                  </div>
                  <div class="text-right shrink-0">
                    <div class={`text-lg font-bold ${colorClass()}`}>
                      {(profile.threat_score / 100).toFixed(2)}
                    </div>
                    <div class="text-xs text-text-muted">
                      K/D {kd()} · {profile.kill_count}K
                    </div>
                  </div>
                </div>

                {/* Expanded detail */}
                <Show when={isSelected()}>
                  <div class="border-t border-border-default px-3 pb-3 pt-3">
                    <div class="grid grid-cols-4 gap-3">
                      <StatItem
                        icon={Skull}
                        label="KILLS 24H"
                        value={profile.recent_kills_24h.toString()}
                        color="text-accent-red"
                      />
                      <StatItem
                        icon={Skull}
                        label="TOTAL KILLS"
                        value={profile.kill_count.toString()}
                        color="text-accent-orange"
                      />
                      <StatItem
                        icon={Shield}
                        label="DEATHS"
                        value={profile.death_count.toString()}
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
                        value={profile.bounty_count.toString()}
                        color="text-accent-cyan"
                      />
                      <StatItem
                        icon={MapPin}
                        label="SYSTEMS"
                        value={profile.systems_visited.toString()}
                        color="text-accent-purple"
                      />
                      <StatItem
                        icon={Clock}
                        label="LAST KILL"
                        value={lastKillAgo()}
                        color="text-text-muted"
                      />
                    </div>
                    {profile.last_seen_system && (
                      <div class="mt-2 text-xs text-text-muted">
                        Last seen:{" "}
                        <span class="text-text-secondary">
                          {profile.last_seen_system_name || profile.last_seen_system}
                        </span>
                      </div>
                    )}
                    {profile.titles && profile.titles.length > 0 && (
                      <div class="mt-2 flex flex-wrap gap-1">
                        {profile.titles.map((title) => (
                          <span class="text-[10px] px-1.5 py-0.5 rounded border border-accent-gold/30 text-accent-gold">
                            {title}
                          </span>
                        ))}
                      </div>
                    )}
                    {/* Score breakdown */}
                    {(() => {
                      const b = computeBreakdown(profile);
                      const total =
                        b.kills + b.recency + b.kd + b.bounties + b.movement;
                      const barMax = Math.max(total, 1);
                      const factors = [
                        {
                          label: "Kills",
                          value: b.kills,
                          color: "var(--color-accent-red)",
                        },
                        {
                          label: "Recent 24h",
                          value: b.recency,
                          color: "var(--color-accent-orange)",
                        },
                        {
                          label: "K/D Ratio",
                          value: b.kd,
                          color: "var(--color-accent-gold)",
                        },
                        {
                          label: "Bounties",
                          value: b.bounties,
                          color: "var(--color-accent-cyan)",
                        },
                        {
                          label: "Movement",
                          value: b.movement,
                          color: "var(--color-accent-purple)",
                        },
                      ];
                      return (
                        <div style="margin-top:0.75rem;border-top:1px solid var(--color-border-default);padding-top:0.75rem">
                          <div
                            class="text-xs text-text-muted"
                            style="margin-bottom:0.5rem"
                          >
                            SCORE BREAKDOWN{" "}
                            <span class="text-text-secondary">
                              {(total / 100).toFixed(2)}
                            </span>
                          </div>
                          {/* Stacked bar */}
                          <div style="display:flex;height:6px;border-radius:3px;overflow:hidden;margin-bottom:0.5rem">
                            {factors
                              .filter((f) => f.value > 0)
                              .map((f) => (
                                <div
                                  style={`width:${(f.value / barMax) * 100}%;height:6px;background:${f.color}`}
                                />
                              ))}
                          </div>
                          {/* Legend */}
                          <div class="flex flex-col gap-1">
                            {factors.map((f) => (
                              <div class="flex items-center gap-2 text-xs">
                                <div
                                  style={`width:8px;height:8px;border-radius:2px;background:${f.color};flex-shrink:0`}
                                />
                                <span class="text-text-muted flex-1">
                                  {f.label}
                                </span>
                                <span class="text-text-secondary">
                                  {(f.value / 100).toFixed(1)}
                                </span>
                              </div>
                            ))}
                          </div>
                        </div>
                      );
                    })()}
                  </div>
                </Show>
              </button>
            );
          }}
        </For>
      </div>
    </div>
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
