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

const API_BASE = "";

export type SubView =
  | "leaderboard"
  | "tracked"
  | "kills"
  | "systems"
  | "feed"
  | "pilots";

type ModeData = {
  threats: ThreatProfile[];
  events: RawEvent[];
  new_pilots: RawEvent[];
  stats: AggregateStats;
};

type CombinedData = {
  demo: ModeData;
  live: ModeData;
  names?: Record<string, string>;
  systems?: Record<string, string>;
};

const emptyStats: AggregateStats = {
  total_tracked: 0,
  avg_score: 0,
  kills_24h: 0,
  top_system: "",
  total_events: 0,
};

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
  const events = () => current().events;
  const newPilots = () => current().new_pilots ?? [];
  const newPilots24h = () => {
    const dayAgo = Date.now() - 86_400_000;
    return newPilots().filter((e) => e.timestamp_ms >= dayAgo);
  };
  const nameMap = () => data()?.names ?? {};
  const systemMap = () => data()?.systems ?? {};
  const stats = () => current().stats;

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

  function handleSelectCharacter(id: number) {
    setSelectedCharacter(id === selectedCharacter() ? null : id);
  }

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
