import { SuiJsonRpcClient } from "@mysten/sui/jsonRpc";
import {
  Clock,
  Crosshair,
  RefreshCw,
  Target,
  Trophy,
  User,
  Users,
} from "lucide-solid";
import { createEffect, createSignal, For, onCleanup, Show } from "solid-js";

const BOUNTY_BOARD_ID = import.meta.env.VITE_BOUNTY_BOARD_ID || "";
const BUILDER_PACKAGE_ID = import.meta.env.VITE_BUILDER_PACKAGE_ID || "";
const SUI_RPC_URL =
  import.meta.env.VITE_SUI_RPC_URL || "https://fullnode.testnet.sui.io:443";

/** A single contributor who stacked additional reward onto a bounty. */
type Contribution = {
  /** Sui wallet address of the contributor. */
  contributor: string;
  /** Number of reward tokens this contributor added. */
  amount: number;
};

/**
 * A bounty record read from the on-chain `BountyBoard` Sui Move object.
 * Fields are deserialized from dynamic-field Move structs, so all IDs arrive as
 * strings even when they represent numeric values.
 */
type Bounty = {
  /** Sequential on-chain bounty ID. */
  id: number;
  /** In-game item ID of the target character, stored as a string. */
  target_item_id: string;
  /** In-game tenant/faction of the target. */
  target_tenant: string;
  /** Move type ID of the reward token. */
  reward_type_id: string;
  /** Total reward quantity across all contributors. */
  reward_quantity: number;
  /** Sui address of the original poster. */
  poster: string;
  /**
   * Expiry timestamp as a numeric string (milliseconds since epoch), or `"0"`
   * for no expiry. Converted to a number at display time.
   */
  expires_at: string;
  claimed: boolean;
  /** Sui address of the hunter who claimed the bounty, or null if unclaimed. */
  claimed_by: string | null;
  contributors: Contribution[];
};

/** A normalised on-chain bounty lifecycle event shown in the activity sidebar. */
type ActivityEvent = {
  type: "posted" | "claimed" | "cancelled" | "stacked";
  /** Wall-clock time of the chain event in milliseconds since epoch. */
  timestamp: number;
  bountyId: number;
  /** Sui address of the poster, hunter, or contributor depending on `type`. */
  actor: string;
  /** Reward token quantity involved, if available from the event payload. */
  rewardQuantity?: number;
};

/** Shortens a Sui address to `0x1234...abcd` form for display. Strings ≤ 10 chars are returned as-is. */
function abbreviate(addr: string): string {
  if (addr.length <= 10) return addr;
  return `${addr.slice(0, 6)}...${addr.slice(-4)}`;
}

/**
 * Returns a reactive accessor that yields a human-readable countdown string
 * ("2d 4h", "30m", "EXPIRED", or "N/A" for `"0"`).
 * The interval only runs while the expiry is in the future, and is cleaned up
 * automatically via `onCleanup`.
 */
function useCountdown(expiresAtMs: string) {
  const [now, setNow] = createSignal(Date.now());
  const expires = Number(expiresAtMs);

  createEffect(() => {
    if (expires === 0 || expires < Date.now()) return;
    const timer = setInterval(() => setNow(Date.now()), 10_000);
    onCleanup(() => clearInterval(timer));
  });

  const display = () => {
    if (expires === 0) return "N/A";
    const diff = expires - now();
    if (diff <= 0) return "EXPIRED";
    const days = Math.floor(diff / 86_400_000);
    const hours = Math.floor((diff % 86_400_000) / 3_600_000);
    const mins = Math.floor((diff % 3_600_000) / 60_000);
    if (days > 0) return `${days}d ${hours}h`;
    if (hours > 0) return `${hours}h ${mins}m`;
    return `${mins}m`;
  };

  return display;
}

/**
 * Classifies a reward quantity into a display tier.
 * Thresholds: BRONZE < 10, SILVER ≥ 10, GOLD ≥ 50, DIAMOND ≥ 100.
 */
function getRewardTier(quantity: number) {
  if (quantity >= 100) return { label: "DIAMOND", class: "text-tier-diamond" };
  if (quantity >= 50) return { label: "GOLD", class: "text-tier-gold" };
  if (quantity >= 10) return { label: "SILVER", class: "text-tier-silver" };
  return { label: "BRONZE", class: "text-tier-bronze" };
}

/**
 * Reads the live bounty state directly from the Sui blockchain via
 * `SuiJsonRpcClient`. Bounties are loaded as dynamic fields on the
 * `BOUNTY_BOARD_ID` object, so one RPC call is made per bounty entry.
 *
 * Activity events (posted / claimed / cancelled / stacked) are queried
 * separately and refreshed every 30 seconds.
 *
 * If `VITE_BOUNTY_BOARD_ID` is not configured, an instructional placeholder is
 * shown instead of making any RPC calls.
 */
export function BountyBoard() {
  const [bounties, setBounties] = createSignal<Bounty[]>([]);
  const [events, setEvents] = createSignal<ActivityEvent[]>([]);
  const [loading, setLoading] = createSignal(true);
  const [error, setError] = createSignal<string | null>(null);

  const client = new SuiJsonRpcClient({ url: SUI_RPC_URL, network: "testnet" });

  /**
   * Fetches all bounty objects from the on-chain board. Each dynamic field
   * requires its own `getDynamicFieldObject` call; malformed entries are
   * silently skipped. Results are sorted: unclaimed first, then by descending ID.
   */
  async function fetchBounties() {
    if (!BOUNTY_BOARD_ID) {
      setLoading(false);
      return;
    }

    try {
      setLoading(true);
      setError(null);

      const result = await client.getDynamicFields({
        parentId: BOUNTY_BOARD_ID,
      });
      const loaded: Bounty[] = [];

      for (const field of result.data) {
        try {
          const obj = await client.getDynamicFieldObject({
            parentId: BOUNTY_BOARD_ID,
            name: field.name,
          });
          const content = obj.data?.content;
          if (content?.dataType !== "moveObject") continue;
          // biome-ignore lint/suspicious/noExplicitAny: Sui Move object fields have dynamic structure
          const fields = (content as any).fields?.value?.fields;
          if (!fields) continue;

          const contributors: Contribution[] = (fields.contributors || []).map(
            // biome-ignore lint/suspicious/noExplicitAny: Sui contributor array has dynamic fields
            (c: any) => ({
              contributor: c.fields?.contributor || "",
              amount: Number(c.fields?.amount || 0),
            }),
          );

          loaded.push({
            id: Number(fields.id),
            target_item_id: String(fields.target_item_id || "?"),
            target_tenant: String(fields.target_tenant || "?"),
            reward_type_id: String(fields.reward_type_id || "?"),
            reward_quantity: Number(fields.reward_quantity || 0),
            poster: String(fields.poster || "?"),
            expires_at: String(fields.expires_at || "0"),
            claimed: Boolean(fields.claimed),
            claimed_by: fields.claimed_by ? String(fields.claimed_by) : null,
            contributors,
          });
        } catch {
          // Skip malformed fields
        }
      }

      loaded.sort((a, b) => {
        if (a.claimed !== b.claimed) return a.claimed ? 1 : -1;
        return b.id - a.id;
      });

      setBounties(loaded);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load bounties");
    } finally {
      setLoading(false);
    }
  }

  /**
   * Queries the four bounty lifecycle Move event types and merges them into a
   * single activity list, capped at 20 entries. Individual event types that
   * haven't been emitted yet (no matching type on-chain) are silently ignored.
   */
  async function fetchEvents() {
    if (!BUILDER_PACKAGE_ID) return;

    try {
      const types = [
        {
          type: `${BUILDER_PACKAGE_ID}::bounty_board::BountyPostedEvent`,
          kind: "posted" as const,
        },
        {
          type: `${BUILDER_PACKAGE_ID}::bounty_board::BountyClaimedEvent`,
          kind: "claimed" as const,
        },
        {
          type: `${BUILDER_PACKAGE_ID}::bounty_board::BountyCancelledEvent`,
          kind: "cancelled" as const,
        },
        {
          type: `${BUILDER_PACKAGE_ID}::bounty_board::BountyStackedEvent`,
          kind: "stacked" as const,
        },
      ];

      const all: ActivityEvent[] = [];
      for (const { type, kind } of types) {
        try {
          const result = await client.queryEvents({
            query: { MoveEventType: type },
            limit: 10,
            order: "descending",
          });
          for (const ev of result.data) {
            // biome-ignore lint/suspicious/noExplicitAny: Sui event parsedJson is untyped
            const parsed = ev.parsedJson as any;
            all.push({
              type: kind,
              timestamp: Number(ev.timestampMs || 0),
              bountyId: Number(parsed?.bounty_id || 0),
              actor: String(
                parsed?.poster || parsed?.hunter || parsed?.contributor || "",
              ),
              rewardQuantity: Number(
                parsed?.reward_quantity || parsed?.reward_quantity_added || 0,
              ),
            });
          }
        } catch {
          /* type might not exist yet */
        }
      }

      all.sort((a, b) => b.timestamp - a.timestamp);
      setEvents(all.slice(0, 20));
    } catch {
      /* non-critical */
    }
  }

  createEffect(() => {
    fetchBounties();
    fetchEvents();
    const interval = setInterval(fetchEvents, 30_000);
    onCleanup(() => clearInterval(interval));
  });

  const active = () => bounties().filter((b) => !b.claimed);
  const claimed = () => bounties().filter((b) => b.claimed);

  return (
    <div>
      <div class="scanline-overlay" style="margin-bottom:2rem">
        <h2 class="text-3xl tracking-wider mb-2">
          BOUNTY <span class="text-accent-cyan">BOARD</span>
        </h2>
        <p class="text-text-secondary text-sm">
          Post bounties on targets. Hunters claim rewards with killmail proof.
        </p>
      </div>

      <Show when={!BOUNTY_BOARD_ID}>
        <div class="glass-card p-8 text-center">
          <h3 class="text-lg tracking-wider mb-3 text-accent-cyan">
            AWAITING DEPLOYMENT
          </h3>
          <p class="text-text-secondary text-sm mb-4">
            The bounty board contract needs to be deployed and configured before
            bounties can be displayed.
          </p>
          <div class="text-xs text-text-muted">
            <p>Run the following to set up:</p>
            <code class="block mt-2 p-3 rounded bg-bg-primary border border-border-default text-left">
              just contracts-deploy
              <br />
              just script bounty_board/create-board
              <br /># Then set VITE_BOUNTY_BOARD_ID in frontend/.env
            </code>
          </div>
        </div>
      </Show>

      <Show when={BOUNTY_BOARD_ID}>
        {/* Action bar */}
        <div
          class="flex items-center justify-between"
          style="margin-bottom:1.5rem"
        >
          <h3 class="text-lg tracking-wider">
            ACTIVE BOUNTIES{" "}
            <span class="text-text-muted text-sm">({active().length})</span>
          </h3>
          <button
            type="button"
            onClick={() => {
              fetchBounties();
              fetchEvents();
            }}
            disabled={loading()}
            class="flex items-center gap-2 px-3 py-2 border border-border-default rounded bg-transparent text-text-secondary hover:text-text-primary hover:border-border-hover transition-all text-sm"
          >
            <RefreshCw size={14} class={loading() ? "animate-spin" : ""} />
            {loading() ? "LOADING" : "REFRESH"}
          </button>
        </div>

        <Show when={error()}>
          <div class="mb-4 p-3 border border-accent-red/50 rounded bg-accent-red/5 text-accent-red text-sm">
            {error()}
          </div>
        </Show>

        <div class="flex gap-6">
          {/* Bounty list */}
          <div class="flex-1 min-w-0">
            <Show
              when={!loading() || bounties().length > 0}
              fallback={<p class="text-text-muted">Loading bounties...</p>}
            >
              <Show
                when={active().length > 0}
                fallback={
                  <div class="glass-card p-8 text-center">
                    <p class="text-text-secondary">No active bounties yet.</p>
                  </div>
                }
              >
                <div class="flex flex-col gap-3">
                  <For each={active()}>
                    {(bounty) => <BountyCard bounty={bounty} />}
                  </For>
                </div>
              </Show>

              <Show when={claimed().length > 0}>
                <div
                  class="border-t border-border-default"
                  style="margin-top:2rem;margin-bottom:1.5rem"
                />
                <h3
                  class="text-lg tracking-wider text-text-muted"
                  style="margin-bottom:1rem"
                >
                  CLAIMED <span class="text-sm">({claimed().length})</span>
                </h3>
                <div class="flex flex-col gap-3">
                  <For each={claimed()}>
                    {(bounty) => <BountyCard bounty={bounty} />}
                  </For>
                </div>
              </Show>
            </Show>
          </div>

          {/* Activity feed */}
          <Show when={events().length > 0}>
            <div class="w-72 shrink-0 hidden lg:block">
              <div class="glass-card p-4">
                <h4
                  class="text-sm tracking-wider flex items-center gap-2 text-text-secondary"
                  style="margin-bottom:1rem"
                >
                  <div class="live-dot" />
                  BOUNTY FEED
                </h4>
                <div class="flex flex-col gap-2">
                  <For each={events()}>
                    {(ev) => {
                      const config: Record<
                        string,
                        { icon: typeof Crosshair; color: string; verb: string }
                      > = {
                        posted: {
                          icon: Crosshair,
                          color: "text-accent-cyan",
                          verb: "posted",
                        },
                        claimed: {
                          icon: Trophy,
                          color: "text-accent-green",
                          verb: "claimed",
                        },
                        cancelled: {
                          icon: Target,
                          color: "text-accent-red",
                          verb: "cancelled",
                        },
                        stacked: {
                          icon: Users,
                          color: "text-accent-blue",
                          verb: "stacked on",
                        },
                      };
                      const c = config[ev.type] || config.posted;
                      const diff = Date.now() - ev.timestamp;
                      const ago =
                        diff < 60_000
                          ? "just now"
                          : diff < 3_600_000
                            ? `${Math.floor(diff / 60_000)}m ago`
                            : diff < 86_400_000
                              ? `${Math.floor(diff / 3_600_000)}h ago`
                              : `${Math.floor(diff / 86_400_000)}d ago`;

                      return (
                        <div
                          class="flex items-start gap-2 text-xs py-1.5"
                          style={`border-left:2px solid var(--color-accent-cyan)`}
                        >
                          <c.icon
                            size={12}
                            class={`shrink-0 mt-0.5 ml-2 ${c.color}`}
                          />
                          <div class="min-w-0">
                            <span class="text-text-secondary">
                              {abbreviate(ev.actor)}{" "}
                              <span class={c.color}>{c.verb}</span> bounty #
                              {ev.bountyId}
                            </span>
                            <div class="text-text-muted mt-0.5">{ago}</div>
                          </div>
                        </div>
                      );
                    }}
                  </For>
                </div>
              </div>
            </div>
          </Show>
        </div>
      </Show>
    </div>
  );
}

function BountyCard(props: { bounty: Bounty }) {
  const b = props.bounty;
  const countdown = useCountdown(b.expires_at);
  const isExpired = () => countdown() === "EXPIRED" && !b.claimed;
  const tier = getRewardTier(b.reward_quantity);
  const hasMultiple = b.contributors.length > 1;
  const [showContrib, setShowContrib] = createSignal(false);

  const expiresIn = Number(b.expires_at) - Date.now();
  const isUrgent =
    !b.claimed && !isExpired() && expiresIn < 7_200_000 && expiresIn > 0;
  const isHighValue = b.reward_quantity >= 50;

  const cardClass = () => {
    let c = "glass-card p-4 transition-all";
    if (b.claimed) c += " opacity-50";
    else if (isExpired()) c += " opacity-40";
    if (isUrgent) c += " neon-glow-urgent";
    else if (isHighValue && !b.claimed) c += " neon-glow-gold";
    return c;
  };

  return (
    <div class={cardClass()}>
      <div class="flex justify-between items-start gap-4">
        <div class="flex-1 min-w-0">
          {/* Title + badges */}
          <div class="flex items-center gap-2 mb-2 flex-wrap">
            <span class="font-bold text-text-primary tracking-wide">
              BOUNTY #{b.id}
            </span>
            {b.claimed && (
              <span class="badge bg-accent-green/15 text-accent-green">
                CLAIMED
              </span>
            )}
            {isExpired() && (
              <span class="badge bg-accent-red/15 text-accent-red">
                EXPIRED
              </span>
            )}
            {hasMultiple && (
              <span class="badge bg-accent-cyan/15 text-accent-cyan">
                {b.contributors.length} STACKED
              </span>
            )}
          </div>

          {/* Details */}
          <div class="flex flex-col gap-1">
            <div class="flex items-center gap-1.5 text-sm text-text-secondary">
              <Target size={14} class="text-accent-red" />
              Target: Character #{b.target_item_id}
              <span class="text-text-muted">({b.target_tenant})</span>
            </div>
            <div class="flex items-center gap-1.5 text-sm text-text-secondary">
              <User size={14} />
              Posted by: {abbreviate(b.poster)}
            </div>
            {b.claimed && b.claimed_by && (
              <div class="text-sm text-accent-green">
                Claimed by: {abbreviate(b.claimed_by)}
              </div>
            )}
          </div>

          {/* Contributors */}
          <Show when={hasMultiple}>
            <button
              type="button"
              onClick={() => setShowContrib(!showContrib())}
              class="flex items-center gap-1 text-xs text-text-muted hover:text-text-secondary bg-transparent border-none p-0 mt-2"
            >
              {showContrib() ? "Hide" : "Show"} contributors
            </button>
            <Show when={showContrib()}>
              <div class="mt-1.5 pl-2 border-l border-border-default">
                <For each={b.contributors}>
                  {(c) => (
                    <div class="text-xs text-text-muted py-0.5">
                      {abbreviate(c.contributor)}: {c.amount}x
                    </div>
                  )}
                </For>
              </div>
            </Show>
          </Show>
        </div>

        {/* Right: reward + timer */}
        <div class="text-right shrink-0">
          <div class={`text-2xl font-bold ${tier.class}`}>
            {b.reward_quantity}x
          </div>
          <div class="text-xs text-text-muted">Type #{b.reward_type_id}</div>
          <div class={`text-xs mt-0.5 ${tier.class}`}>{tier.label}</div>
          <Show when={!b.claimed}>
            <div
              class={`flex items-center justify-end gap-1 mt-2 text-xs ${
                isExpired() || isUrgent ? "text-accent-red" : "text-text-muted"
              }`}
            >
              <Clock size={12} />
              {countdown()}
            </div>
          </Show>
        </div>
      </div>
    </div>
  );
}
