import { createSignal, createEffect, onCleanup, Show } from "solid-js";
import type { ThreatProfile, RawEvent, AggregateStats } from "./types";
import { StatsBar } from "./StatsBar";
import { ThreatLeaderboard } from "./ThreatLeaderboard";
import { SentinelFeed } from "./SentinelFeed";
import { TrackedView } from "./views/TrackedView";
import { KillsView } from "./views/KillsView";
import { SystemsView } from "./views/SystemsView";
import { FeedView } from "./views/FeedView";
import { LoadingState } from "./LoadingState";

const API_BASE = "";

export type SubView = "leaderboard" | "tracked" | "kills" | "systems" | "feed";

type CombinedData = {
  demo: { threats: ThreatProfile[]; events: RawEvent[]; stats: AggregateStats };
  live: { threats: ThreatProfile[]; events: RawEvent[]; stats: AggregateStats };
};

const emptyStats: AggregateStats = {
  total_tracked: 0,
  avg_score: 0,
  kills_24h: 0,
  top_system: "",
  events_per_min: 0,
};

export function SentinelDashboard(props: { mode: "demo" | "live" }) {
  const [data, setData] = createSignal<CombinedData | null>(null);
  const [selectedCharacter, setSelectedCharacter] = createSignal<number | null>(
    null,
  );
  const [subView, setSubView] = createSignal<SubView>("feed");
  const [loading, setLoading] = createSignal(true);

  const current = () =>
    data()?.[props.mode] ?? { threats: [], events: [], stats: emptyStats };
  const profiles = () => current().threats;
  const events = () => current().events;
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
        activeView={subView()}
        onStatClick={handleStatClick}
      />

      {/* Main content */}
      <div class="flex gap-6 mt-6 items-start">
        {/* Left: main view */}
        <div class="flex-1 min-w-0">
          {/* Sub-views */}
          <Show when={subView() === "tracked"}>
            <TrackedView
              profiles={profiles()}
              onSelect={handleSelectCharacter}
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
          <div
            class="w-80 shrink-0 hidden lg:block overflow-hidden cursor-pointer"
            style={
              subView() !== "feed" && profiles().length > 0
                ? "margin-top:2.5rem"
                : ""
            }
            onClick={() => handleStatClick("feed")}
          >
            <SentinelFeed events={events()} profiles={profiles()} />
          </div>
        </Show>
      </div>
    </div>
  );
}
