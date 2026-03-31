import { UserPlus } from "lucide-solid";
import { For } from "solid-js";
import { LoadingState } from "../LoadingState";
import type { RawEvent, ThreatProfile } from "../types";

type PilotsViewProps = {
  /**
   * `character_registered` events from the backend, unbounded (not filtered to
   * 24h here — the header count comes from the parent which pre-filters).
   */
  events: RawEvent[];
  profiles: ThreatProfile[];
  /** character_item_id → name; used to resolve names before falling back to profiles. */
  names?: Record<string, string>;
  /** Shows a loading spinner while the initial data fetch is in progress. */
  loading?: boolean;
};

function timeAgo(timestampMs: number): string {
  if (!timestampMs) return "";
  const diff = Date.now() - timestampMs;
  if (diff < 60_000) return "just now";
  if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
  if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
  return `${Math.floor(diff / 86_400_000)}d ago`;
}

/**
 * Chronological list of newly detected pilots (character_registered events).
 * Shows up to 200 entries. The header count reflects pilots seen in the last
 * 24 hours, computed inline from the event timestamps.
 */
export function PilotsView(props: PilotsViewProps) {
  const nameOf = (id: unknown): string => {
    if (id == null) return "?";
    const str = String(id);
    if (props.names?.[str]) return props.names[str];
    const numId = Number(id);
    const profile = props.profiles.find((p) => p.character_item_id === numId);
    return profile?.name || `Pilot #${numId}`;
  };

  return (
    <div>
      <h3 class="text-lg tracking-wider" style="margin-bottom:1rem">
        NEW PILOTS
        <span class="text-text-muted text-sm ml-2">
          {
            props.events.filter(
              (e) => e.timestamp_ms >= Date.now() - 86_400_000,
            ).length
          }{" "}
          last 24h
        </span>
      </h3>

      <LoadingState
        loading={props.loading ?? false}
        hasData={props.events.length > 0}
        loadingText="Scanning for new pilots..."
        emptyText="No new pilots detected yet."
      />

      <div class="flex flex-col gap-2">
        <For each={props.events.slice(0, 200)}>
          {(event) => {
            const charId = () => event.data.character_id;
            const name = () => nameOf(charId());

            return (
              <div
                class="glass-card p-3 flex items-center gap-3"
                style="border-left:3px solid var(--color-text-primary)"
              >
                <UserPlus size={16} class="text-text-primary shrink-0" />
                <div class="flex-1 min-w-0">
                  <span class="text-sm text-text-primary font-bold">
                    {name()}
                  </span>
                </div>
                <span class="text-xs text-text-muted shrink-0">
                  {timeAgo(event.timestamp_ms)}
                </span>
              </div>
            );
          }}
        </For>
      </div>
    </div>
  );
}
