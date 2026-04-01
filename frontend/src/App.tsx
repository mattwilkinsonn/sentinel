import { Crosshair, Shield, Wallet } from "lucide-solid";
import { createSignal, For, Show } from "solid-js";
import { BountyBoard } from "./BountyBoard";
import { SentinelDashboard } from "./SentinelDashboard";
import { Tooltip } from "./Tooltip";
import { useWallet, WalletProvider } from "./WalletContext";

/** Top-level navigation tabs. */
type Tab = "sentinel" | "bounty";
/**
 * Data source mode, toggled via the header button.
 * - `demo`: pre-seeded simulated data served by the backend
 * - `live`: real events streaming from the Sui blockchain
 */
type DataMode = "demo" | "live";

function WalletButton() {
  const { wallets, connectedAddress, connect, disconnect } = useWallet();
  const [open, setOpen] = createSignal(false);

  function abbreviate(addr: string) {
    if (addr.length <= 10) return addr;
    return `${addr.slice(0, 6)}...${addr.slice(-4)}`;
  }

  return (
    <div class="relative">
      <Show
        when={connectedAddress()}
        fallback={
          <Show
            when={wallets().length > 0}
            fallback={
              <Tooltip text="No Sui wallet extension detected. Install Sui Wallet to post bounties.">
                <button
                  type="button"
                  class="flex items-center gap-2 px-3 py-1.5 rounded text-xs bg-transparent border border-text-muted text-text-muted cursor-not-allowed"
                >
                  <Wallet size={14} />
                  NO WALLET
                </button>
              </Tooltip>
            }
          >
            <div class="relative">
              <button
                type="button"
                onClick={() => setOpen(!open())}
                class="flex items-center gap-2 px-3 py-1.5 rounded text-xs transition-all bg-transparent border border-text-secondary text-text-secondary hover:border-text-primary hover:text-text-primary"
              >
                <Wallet size={14} />
                CONNECT WALLET
              </button>
              <Show when={open()}>
                <div class="absolute right-0 top-full mt-1 z-50 glass-card p-2 min-w-48 flex flex-col gap-1">
                  <For each={wallets()}>
                    {(wallet) => (
                      <button
                        type="button"
                        onClick={async () => {
                          await connect(wallet);
                          setOpen(false);
                        }}
                        class="flex items-center gap-2 px-3 py-2 text-sm text-text-secondary hover:text-text-primary hover:bg-bg-secondary rounded transition-all bg-transparent border-none text-left w-full"
                      >
                        <Show when={wallet.icon}>
                          <img
                            src={wallet.icon}
                            alt=""
                            class="w-4 h-4 rounded"
                          />
                        </Show>
                        {wallet.name}
                      </button>
                    )}
                  </For>
                </div>
              </Show>
            </div>
          </Show>
        }
      >
        <div class="relative">
          <button
            type="button"
            onClick={() => setOpen(!open())}
            class="flex items-center gap-2 px-3 py-1.5 rounded text-xs transition-all bg-transparent border border-text-secondary text-text-secondary hover:border-text-primary hover:text-text-primary"
          >
            <div class="live-dot" />
            {abbreviate(connectedAddress() ?? "")}
          </button>
          <Show when={open()}>
            <div class="absolute right-0 top-full mt-1 z-50 glass-card p-2 min-w-40">
              <button
                type="button"
                onClick={async () => {
                  await disconnect();
                  setOpen(false);
                }}
                class="flex items-center gap-2 px-3 py-2 text-sm text-accent-red hover:bg-accent-red/10 rounded transition-all bg-transparent border-none text-left w-full"
              >
                Disconnect
              </button>
            </div>
          </Show>
        </div>
      </Show>
    </div>
  );
}

function AppInner() {
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
            <img
              src="/sentinel-icon.svg"
              alt="Sentinel"
              class="h-12 shrink-0"
            />
            <button
              type="button"
              onClick={() => setTab("sentinel")}
              class={`flex items-center gap-2 px-3 py-1.5 rounded text-sm transition-all bg-transparent border-none ${
                tab() === "sentinel"
                  ? "text-accent-purple border-b-2 border-b-accent-purple"
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

          <div class="flex items-center gap-3">
            <WalletButton />

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

export default function App() {
  return (
    <WalletProvider>
      <AppInner />
    </WalletProvider>
  );
}
