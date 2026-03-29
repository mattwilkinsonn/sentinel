import { createEffect, createSignal, onCleanup, Show } from "solid-js";
import { LoadingState } from "./LoadingState";
import { SentinelFeed } from "./SentinelFeed";
import { StatsBar } from "./StatsBar";
import { ThreatLeaderboard } from "./ThreatLeaderboard";
import type { AggregateStats, RawEvent, ThreatProfile } from "./types";
import { FeedView } from "./views/FeedView";
import { KillsView } from "./views/KillsView";
import { PilotsView } from "./views/PilotsView";
import { SystemsView } from "./views/SystemsView";
import { TrackedView } from "./views/TrackedView";

/** Root for all API calls. Defaults to same-origin when `VITE_API_URL` is not set. */
const API_BASE = import.meta.env.VITE_API_URL || "";

/**
 * Named sub-views within the Sentinel dashboard. The active view determines
 * which content panel renders in the left column and which StatsBar tile is
 * highlighted.
 */
export type SubView =
  | "leaderboard"
  | "tracked"
  | "kills"
  | "systems"
  | "feed"
  | "pilots";

/** The data payload for a single mode (demo or live) within the combined API response. */
type ModeData = {
  threats: ThreatProfile[];
  events: RawEvent[];
  /** `character_registered` events used to detect newly-seen pilots. */
  new_pilots: RawEvent[];
  stats: AggregateStats;
};

/**
 * Top-level shape of the `/api/data` response. Both demo and live datasets are
 * bundled together so the user can switch modes without a new network request.
 * `names` and `systems` are shared lookup maps used by all event renderers.
 */
type CombinedData = {
  demo: ModeData;
  live: ModeData;
  /** character_item_id (as string key) → resolved pilot name */
  names?: Record<string, string>;
  /** system_id (as string key) → human-readable system name */
  systems?: Record<string, string>;
};

const emptyStats: AggregateStats = {
  total_tracked: 0,
  avg_score: 0,
  kills_24h: 0,
  top_system: "",
  total_events: 0,
};

/**
 * Top-level dashboard component for the Sentinel threat-intelligence view.
 *
 * Fetches `/api/data` on mount, then re-fetches every 10 seconds and on each
 * SSE push from `/api/events/stream`. The `mode` prop selects between the
 * `demo` and `live` slices of the combined response without re-fetching.
 *
 * Layout: StatsBar across the top, a switchable main content area on the left,
 * and a fixed-height live-intel sidebar on the right (hidden when the feed view
 * is active to avoid duplication).
 */
export function SentinelDashboard(props: { mode: "demo" | "live" }) {
  const [data, setData] = createSignal<CombinedData | null>(null);
  const [selectedCharacter, setSelectedCharacter] = createSignal<number | null>(
    null,
  );
  const [subView, setSubView] = createSignal<SubView>("feed");
  const [loading, setLoading] = createSignal(true);
  const [contentHeight, setContentHeight] = createSignal(600);
  let contentRef: HTMLDivElement | undefined;

  const current = () =>
    data()?.[props.mode] ?? {
      threats: [],
      events: [],
      new_pilots: [],
      stats: emptyStats,
    };
  const profiles = () => current().threats;
  // Hide events with unresolved character names — better to show fewer
  // events with real names than pollute the feed with raw IDs
  const charIdKeys = [
    "killer_character_id",
    "target_item_id",
    "character_id",
    "poster_id",
    "contributor_id",
    "hunter_id",
  ];
  const events = () => {
    const n = data()?.names ?? {};
    return current().events.filter((e) => {
      // Structure kills only need the killer resolved, not the victim
      const keysToCheck =
        e.event_type === "structure_destroyed"
          ? ["killer_character_id"]
          : charIdKeys;
      const d = e.data as Record<string, unknown>;
      return keysToCheck.every((key) => {
        const v = d[key];
        return v == null || !!n[String(v)];
      });
    });
  };
  const newPilots = () => current().new_pilots ?? [];
  const newPilots24h = () => {
    const dayAgo = Date.now() - 86_400_000;
    return newPilots().filter((e) => e.timestamp_ms >= dayAgo);
  };
  const nameMap = () => data()?.names ?? {};
  const systemMap = () => data()?.systems ?? {};
  const stats = () => current().stats;

  /** Loads the combined data bundle from the backend and updates all derived signals. */
  async function fetchData() {
    try {
      const res = await fetch(`${API_BASE}/api/data`);
      if (res.ok) setData(await res.json());
    } catch (e) {
      console.error("Failed to fetch sentinel data:", e);
    } finally {
      setLoading(false);
    }
  }

  createEffect(() => {
    fetchData();
    const interval = setInterval(fetchData, 10_000);
    onCleanup(() => clearInterval(interval));
  });

  // Track left content height for the mini feed sidebar
  createEffect(() => {
    if (!contentRef) return;
    const observer = new ResizeObserver((entries) => {
      for (const entry of entries) {
        setContentHeight(entry.contentRect.height);
      }
    });
    observer.observe(contentRef);
    onCleanup(() => observer.disconnect());
  });

  // SSE triggers re-fetch on new events
  createEffect(() => {
    const source = new EventSource(`${API_BASE}/api/events/stream`);
    source.onmessage = () => fetchData();
    onCleanup(() => source.close());
  });

  /** Toggles expanded detail for a character: clicking the same ID deselects. */
  function handleSelectCharacter(id: number) {
    setSelectedCharacter(id === selectedCharacter() ? null : id);
  }

  /** Switches to a sub-view and clears any character selection to prevent stale detail panels. */
  function handleStatClick(view: SubView) {
    setSubView(view);
    setSelectedCharacter(null);
  }

  return (
    <div>
      {/* Hero */}
      <div class="scanline-overlay mb-8">
        <h2 class="text-3xl tracking-wider mb-2">
          SENTINEL <span class="text-accent-red">THREAT INTELLIGENCE</span>
        </h2>
        <p class="text-text-secondary text-sm">
          Real-time threat scoring via gRPC checkpoint streaming from Sui
        </p>
      </div>

      {/* Stats bar */}
      <StatsBar
        stats={stats()}
        profiles={profiles()}
        newPilotCount={newPilots24h().length}
        activeView={subView()}
        onStatClick={handleStatClick}
      />

      {/* Main content */}
      <div class="flex gap-6 mt-6 items-start">
        {/* Left: main view */}
        <div class="flex-1 min-w-0" ref={(el) => (contentRef = el)}>
          {/* Sub-views */}
          <Show when={subView() === "tracked"}>
            <TrackedView
              profiles={profiles()}
              newPilotCount={newPilots24h().length}
              onSelect={handleSelectCharacter}
              onViewPilots={() => setSubView("pilots")}
              loading={loading()}
            />
          </Show>
          <Show when={subView() === "kills"}>
            <KillsView
              profiles={profiles()}
              onSelect={handleSelectCharacter}
              selectedId={selectedCharacter()}
              loading={loading()}
            />
          </Show>
          <Show when={subView() === "systems"}>
            <SystemsView profiles={profiles()} loading={loading()} />
          </Show>
          <Show when={subView() === "feed"}>
            <FeedView
              events={events()}
              profiles={profiles()}
              names={nameMap()}
              systems={systemMap()}
              loading={loading()}
            />
          </Show>
          <Show when={subView() === "pilots"}>
            <PilotsView
              events={newPilots()}
              profiles={profiles()}
              names={nameMap()}
              loading={loading()}
            />
          </Show>

          {/* Default: leaderboard */}
          <Show when={subView() === "leaderboard"}>
            <LoadingState
              loading={loading()}
              hasData={profiles().length > 0}
              loadingText="Loading threat leaderboard..."
              emptyText="No threat data yet. Waiting for on-chain events..."
            />
            <Show when={profiles().length > 0}>
              <ThreatLeaderboard
                profiles={profiles()}
                onSelect={handleSelectCharacter}
                selectedId={selectedCharacter()}
              />
            </Show>
          </Show>
        </div>

        {/* Right: Activity feed sidebar (hidden when feed view is active) */}
        <Show when={subView() !== "feed"}>
          <button
            type="button"
            class="w-80 shrink-0 hidden lg:block cursor-pointer bg-transparent border-none p-0 text-left overflow-hidden sticky top-20"
            style={`height:min(${contentHeight()}px, calc(100vh - 6rem))`}
            onClick={() => handleStatClick("feed")}
          >
            <SentinelFeed
              events={events()}
              profiles={profiles()}
              names={nameMap()}
              systems={systemMap()}
            />
          </button>
        </Show>
      </div>
    </div>
  );
}
