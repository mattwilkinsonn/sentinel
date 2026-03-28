import { For } from "solid-js";
import { MapPin } from "lucide-solid";
import type { ThreatProfile } from "../types";
import { LoadingState } from "../LoadingState";
import { getThreatTier, getThreatColor, getThreatColorClass } from "../types";

type SystemsViewProps = {
  profiles: ThreatProfile[];
  loading?: boolean;
};

type SystemInfo = {
  name: string;
  characterCount: number;
  totalThreat: number;
  avgThreat: number;
  topKiller: ThreatProfile | null;
};

export function SystemsView(props: SystemsViewProps) {
  const systems = () => {
    const map = new Map<string, { profiles: ThreatProfile[] }>();

    for (const p of props.profiles) {
      if (!p.last_seen_system) continue;
      const existing = map.get(p.last_seen_system);
      if (existing) {
        existing.profiles.push(p);
      } else {
        map.set(p.last_seen_system, { profiles: [p] });
      }
    }

    const result: SystemInfo[] = [];
    for (const [name, data] of map) {
      const totalThreat = data.profiles.reduce((s, p) => s + p.threat_score, 0);
      const sorted = [...data.profiles].sort(
        (a, b) => b.kill_count - a.kill_count,
      );
      result.push({
        name,
        characterCount: data.profiles.length,
        totalThreat,
        avgThreat: Math.round(totalThreat / data.profiles.length),
        topKiller: sorted[0] ?? null,
      });
    }

    return result.sort((a, b) => b.totalThreat - a.totalThreat);
  };

  return (
    <div>
      <h3 class="text-lg tracking-wider" style="margin-bottom:1rem">
        SYSTEM INTELLIGENCE
        <span class="text-text-muted text-sm ml-2">
          {systems().length} systems
        </span>
      </h3>

      <LoadingState
        loading={props.loading ?? false}
        hasData={systems().length > 0}
        loadingText="Loading system intelligence..."
        emptyText="No system data yet. Waiting for jump events..."
      />

      <div class="flex flex-col gap-3">
        <For each={systems()}>
          {(system) => {
            const avgTier = () => getThreatTier(system.avgThreat);
            const color = () => getThreatColor(avgTier());
            const colorClass = () => getThreatColorClass(avgTier());

            return (
              <div
                class="glass-card p-4"
                style={{ "border-left": `3px solid ${color()}` }}
              >
                <div class="flex items-center justify-between mb-2">
                  <div class="flex items-center gap-2">
                    <MapPin size={16} class={colorClass()} />
                    <h4 class="text-base tracking-wider">{system.name}</h4>
                    <span
                      class={`badge ${colorClass()}`}
                      style={{ background: `${color()}15` }}
                    >
                      {avgTier()}
                    </span>
                  </div>
                  <span class={`text-lg font-bold ${colorClass()}`}>
                    {(system.avgThreat / 100).toFixed(2)} avg
                  </span>
                </div>

                <div class="flex gap-4 text-xs text-text-muted">
                  <span>{system.characterCount} characters</span>
                  <span>
                    Total threat: {(system.totalThreat / 100).toFixed(0)}
                  </span>
                  {system.topKiller && (
                    <span>
                      Top killer:{" "}
                      {system.topKiller.name ||
                        `Pilot #${system.topKiller.character_item_id}`}{" "}
                      ({system.topKiller.kill_count}K)
                    </span>
                  )}
                </div>
              </div>
            );
          }}
        </For>
      </div>
    </div>
  );
}
