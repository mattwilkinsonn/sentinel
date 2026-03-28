import { MapPin, Skull, Users } from "lucide-solid";
import { createSignal, For, Show } from "solid-js";
import { LoadingState } from "../LoadingState";
import type { ThreatProfile } from "../types";
import { getThreatColor, getThreatColorClass, getThreatTier } from "../types";

type SystemsViewProps = {
  profiles: ThreatProfile[];
  loading?: boolean;
};

type SystemInfo = {
  id: string;
  displayName: string;
  characterCount: number;
  totalThreat: number;
  avgThreat: number;
  totalKills: number;
  topKiller: ThreatProfile | null;
  profiles: ThreatProfile[];
};

export function SystemsView(props: SystemsViewProps) {
  const [expanded, setExpanded] = createSignal<string | null>(null);

  const systems = () => {
    const map = new Map<string, ThreatProfile[]>();

    for (const p of props.profiles) {
      if (!p.last_seen_system) continue;
      const existing = map.get(p.last_seen_system);
      if (existing) {
        existing.push(p);
      } else {
        map.set(p.last_seen_system, [p]);
      }
    }

    const result: SystemInfo[] = [];
    for (const [id, profiles] of map) {
      const totalThreat = profiles.reduce((s, p) => s + p.threat_score, 0);
      const totalKills = profiles.reduce((s, p) => s + p.kill_count, 0);
      const sorted = [...profiles].sort((a, b) => b.kill_count - a.kill_count);
      const displayName =
        profiles.find((p) => p.last_seen_system_name)?.last_seen_system_name ||
        id;
      result.push({
        id,
        displayName,
        characterCount: profiles.length,
        totalThreat,
        avgThreat: Math.round(totalThreat / profiles.length),
        totalKills,
        topKiller: sorted[0] ?? null,
        profiles: sorted,
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
            const tier = () => getThreatTier(system.totalThreat);
            const color = () => getThreatColor(tier());
            const colorClass = () => getThreatColorClass(tier());
            const isExpanded = () => expanded() === system.id;

            return (
              <button
                type="button"
                class="glass-card p-4 w-full text-left bg-transparent cursor-pointer transition-all"
                style={{ "border-left": `3px solid ${color()}` }}
                onClick={() => setExpanded(isExpanded() ? null : system.id)}
              >
                <div class="flex items-center justify-between mb-2">
                  <div class="flex items-center gap-2">
                    <MapPin size={16} class={colorClass()} />
                    <h4 class="text-base tracking-wider">
                      {system.displayName}
                    </h4>
                  </div>
                  <span class={`text-lg font-bold ${colorClass()}`}>
                    {(system.totalThreat / 100).toFixed(0)}
                  </span>
                </div>

                <div class="flex gap-4 text-xs text-text-muted">
                  <span class="flex items-center gap-1">
                    <Users size={12} />
                    {system.characterCount} pilots
                  </span>
                  <span class="flex items-center gap-1">
                    <Skull size={12} />
                    {system.totalKills} kills
                  </span>
                  {system.topKiller && (
                    <span>
                      Top threat:{" "}
                      {system.topKiller.name ||
                        `Pilot #${system.topKiller.character_item_id}`}
                    </span>
                  )}
                </div>

                <Show when={isExpanded()}>
                  <div class="mt-3 pt-3 border-t border-border-default">
                    <div class="flex flex-col gap-2">
                      <For each={system.profiles.slice(0, 10)}>
                        {(p) => {
                          const pTier = () => getThreatTier(p.threat_score);
                          const pColor = () => getThreatColorClass(pTier());
                          return (
                            <div class="flex items-center gap-3 text-sm">
                              <div
                                class="w-2 h-2 rounded-full shrink-0"
                                style={{
                                  background: getThreatColor(pTier()),
                                }}
                              />
                              <span class="flex-1">
                                {p.name || `Pilot #${p.character_item_id}`}
                              </span>
                              <span class={`font-bold ${pColor()}`}>
                                {(p.threat_score / 100).toFixed(2)}
                              </span>
                              <span class="text-text-muted text-xs">
                                {p.kill_count} K / {p.death_count} D
                              </span>
                            </div>
                          );
                        }}
                      </For>
                    </div>
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
