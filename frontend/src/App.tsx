import { Crosshair, Shield } from "lucide-solid";
import { createSignal } from "solid-js";
import { BountyBoard } from "./BountyBoard";
import { SentinelDashboard } from "./SentinelDashboard";
import { Tooltip } from "./Tooltip";

/** Top-level navigation tabs. */
type Tab = "sentinel" | "bounty";
/**
 * Data source mode, toggled via the header button.
 * - `demo`: pre-seeded simulated data served by the backend
 * - `live`: real events streaming from the Sui blockchain
 */
type DataMode = "demo" | "live";

export default function App() {
  const [tab, setTab] = createSignal<Tab>("sentinel");
  const [mode, setMode] = createSignal<DataMode>("demo");

  function toggleMode() {
    setMode(mode() === "demo" ? "live" : "demo");
  }

  return (
    <div class="min-h-screen bg-bg-primary">
      <header class="sticky top-0 z-50 glass-card rounded-none border-x-0 border-t-0">
        <div class="max-w-6xl mx-auto px-6 py-4 flex items-center justify-between">
          <div class="flex items-center gap-6">
            <button
              type="button"
              onClick={() => setTab("sentinel")}
              class={`flex items-center gap-2 px-3 py-1.5 rounded text-sm transition-all bg-transparent border-none ${
                tab() === "sentinel"
                  ? "text-accent-red border-b-2 border-b-accent-red"
                  : "text-text-secondary hover:text-text-primary"
              }`}
            >
              <Shield size={16} />
              SENTINEL
            </button>
            <button
              type="button"
              onClick={() => setTab("bounty")}
              class={`flex items-center gap-2 px-3 py-1.5 rounded text-sm transition-all bg-transparent border-none ${
                tab() === "bounty"
                  ? "text-accent-cyan border-b-2 border-b-accent-cyan"
                  : "text-text-secondary hover:text-text-primary"
              }`}
            >
              <Crosshair size={16} />
              BOUNTY BOARD
            </button>
          </div>

          <Tooltip
            text={
              mode() === "demo"
                ? "Showing simulated data. Click to switch to live blockchain events."
                : "Streaming live events from Sui. Click to switch to demo data."
            }
          >
            <button
              type="button"
              onClick={toggleMode}
              class="flex items-center gap-2 px-3 py-1.5 rounded text-xs transition-all bg-transparent border border-border-default hover:border-border-hover"
            >
              <div
                class={mode() === "live" ? "live-dot" : ""}
                style={
                  mode() === "live"
                    ? ""
                    : "width:8px;height:8px;border-radius:50%;background:var(--color-text-muted)"
                }
              />
              {mode() === "demo" ? "DEMO" : "LIVE"}
            </button>
          </Tooltip>
        </div>
      </header>

      <main class="max-w-6xl mx-auto px-6 py-8">
        {tab() === "sentinel" ? (
          <SentinelDashboard mode={mode()} />
        ) : (
          <BountyBoard />
        )}
      </main>
    </div>
  );
}
