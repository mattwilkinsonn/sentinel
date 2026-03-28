import { useState, useEffect, useCallback } from "react";
import {
  getObjectWithJson,
  getObjectWithDynamicFields,
} from "@evefrontier/dapp-kit";
import { useSuiClient } from "@mysten/dapp-kit-react";
import { RefreshCw, Plus, X } from "lucide-react";
import { PostBountyForm } from "./PostBountyForm";
import { BountyCard } from "./BountyCard";
import { MostWanted } from "./MostWanted";
import { ActivityFeed } from "./ActivityFeed";

const BOUNTY_BOARD_ID = import.meta.env.VITE_BOUNTY_BOARD_ID || "";
const BUILDER_PACKAGE_ID = import.meta.env.VITE_BUILDER_PACKAGE_ID || "";

export type ContributionData = {
  contributor: string;
  contributor_character_id: string;
  amount: number;
};

export type BountyData = {
  id: number;
  target_item_id: string;
  target_tenant: string;
  reward_type_id: string;
  reward_quantity: number;
  poster: string;
  poster_character_id: string;
  created_at: string;
  expires_at: string;
  claimed: boolean;
  claimed_by: string | null;
  claimed_killmail_id: string | null;
  contributors: ContributionData[];
};

export type ActivityEvent = {
  type: "posted" | "claimed" | "cancelled" | "stacked";
  timestamp: number;
  bountyId: number;
  actor: string;
  rewardQuantity?: number;
  targetItemId?: string;
};

export function BountyBoard({ walletAddress }: { walletAddress: string }) {
  const [bounties, setBounties] = useState<BountyData[]>([]);
  const [events, setEvents] = useState<ActivityEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showPostForm, setShowPostForm] = useState(false);
  const suiClient = useSuiClient();

  const fetchBounties = useCallback(async () => {
    if (!BOUNTY_BOARD_ID) {
      setError("VITE_BOUNTY_BOARD_ID not configured");
      setLoading(false);
      return;
    }

    try {
      setLoading(true);
      setError(null);

      const boardResult = await getObjectWithJson(BOUNTY_BOARD_ID);
      const boardJson = boardResult.data?.object?.asMoveObject?.contents?.json as any;

      if (!boardJson) {
        setError("Could not load bounty board data");
        setLoading(false);
        return;
      }

      const dfResult = await getObjectWithDynamicFields(BOUNTY_BOARD_ID);
      const dynamicFields = dfResult.data?.object?.asMoveObject?.dynamicFields?.nodes || [];

      const loadedBounties: BountyData[] = [];

      for (const df of dynamicFields) {
        const fieldJson = df?.value?.contents?.json as any;
        if (!fieldJson || fieldJson.id === undefined) continue;

        const contributors: ContributionData[] = (fieldJson.contributors || []).map((c: any) => ({
          contributor: c.contributor || "",
          contributor_character_id: c.contributor_character_id || "",
          amount: Number(c.amount || 0),
        }));

        loadedBounties.push({
          id: Number(fieldJson.id),
          target_item_id: fieldJson.target_item_id || "?",
          target_tenant: fieldJson.target_tenant || "?",
          reward_type_id: fieldJson.reward_type_id || "?",
          reward_quantity: Number(fieldJson.reward_quantity || 0),
          poster: fieldJson.poster || "?",
          poster_character_id: fieldJson.poster_character_id || "?",
          created_at: fieldJson.created_at || "0",
          expires_at: fieldJson.expires_at || "0",
          claimed: fieldJson.claimed || false,
          claimed_by: fieldJson.claimed_by || null,
          claimed_killmail_id: fieldJson.claimed_killmail_id || null,
          contributors,
        });
      }

      loadedBounties.sort((a, b) => {
        if (a.claimed !== b.claimed) return a.claimed ? 1 : -1;
        return b.id - a.id;
      });

      setBounties(loadedBounties);
    } catch (err) {
      setError(err instanceof Error ? err.message : "Failed to load bounties");
    } finally {
      setLoading(false);
    }
  }, []);

  const fetchEvents = useCallback(async () => {
    if (!BUILDER_PACKAGE_ID) return;

    try {
      const eventTypes = [
        { type: `${BUILDER_PACKAGE_ID}::bounty_board::BountyPostedEvent`, kind: "posted" as const },
        { type: `${BUILDER_PACKAGE_ID}::bounty_board::BountyClaimedEvent`, kind: "claimed" as const },
        { type: `${BUILDER_PACKAGE_ID}::bounty_board::BountyCancelledEvent`, kind: "cancelled" as const },
        { type: `${BUILDER_PACKAGE_ID}::bounty_board::BountyStackedEvent`, kind: "stacked" as const },
      ];

      const allEvents: ActivityEvent[] = [];

      for (const { type, kind } of eventTypes) {
        try {
          const result = await suiClient.queryEvents({
            query: { MoveEventType: type },
            limit: 10,
            order: "descending",
          });

          for (const ev of result.data) {
            const parsed = ev.parsedJson as any;
            allEvents.push({
              type: kind,
              timestamp: Number(ev.timestampMs || 0),
              bountyId: Number(parsed?.bounty_id || 0),
              actor: parsed?.poster || parsed?.hunter || parsed?.contributor || "",
              rewardQuantity: Number(parsed?.reward_quantity || parsed?.reward_quantity_added || 0),
              targetItemId: parsed?.target_item_id,
            });
          }
        } catch {
          // Event type might not exist yet on chain
        }
      }

      allEvents.sort((a, b) => b.timestamp - a.timestamp);
      setEvents(allEvents.slice(0, 20));
    } catch {
      // Non-critical, silently fail
    }
  }, [suiClient]);

  useEffect(() => {
    fetchBounties();
    fetchEvents();
    const interval = setInterval(fetchEvents, 30000);
    return () => clearInterval(interval);
  }, [fetchBounties, fetchEvents]);

  const activeBounties = bounties.filter((b) => !b.claimed);
  const claimedBounties = bounties.filter((b) => b.claimed);

  return (
    <div>
      {/* Most Wanted */}
      {activeBounties.length > 0 && (
        <MostWanted bounties={activeBounties} />
      )}

      {/* Action Bar */}
      <div className="flex items-center justify-between mb-6">
        <h3 className="text-xl tracking-wider">
          ACTIVE BOUNTIES <span className="text-text-muted text-base">({activeBounties.length})</span>
        </h3>
        <div className="flex gap-2">
          <button
            onClick={fetchBounties}
            disabled={loading}
            className="flex items-center gap-2 px-3 py-2 border border-border-default rounded bg-transparent text-text-secondary hover:text-text-primary hover:border-border-hover transition-all text-sm disabled:opacity-40"
          >
            <RefreshCw className={`w-3.5 h-3.5 ${loading ? "animate-spin" : ""}`} />
            {loading ? "LOADING" : "REFRESH"}
          </button>
          <button
            onClick={() => setShowPostForm(!showPostForm)}
            className={`flex items-center gap-2 px-4 py-2 border rounded text-sm transition-all ${
              showPostForm
                ? "border-accent-red text-accent-red hover:bg-accent-red/10"
                : "border-accent-cyan text-accent-cyan hover:bg-accent-cyan/10"
            }`}
          >
            {showPostForm ? <X className="w-3.5 h-3.5" /> : <Plus className="w-3.5 h-3.5" />}
            {showPostForm ? "CANCEL" : "POST BOUNTY"}
          </button>
        </div>
      </div>

      {/* Error */}
      {error && (
        <div className="mb-4 p-3 border border-accent-red/50 rounded bg-accent-red/5 text-accent-red text-sm">
          {error}
        </div>
      )}

      {/* Post Form */}
      {showPostForm && (
        <div className="mb-6">
          <PostBountyForm
            builderPackageId={BUILDER_PACKAGE_ID}
            bountyBoardId={BOUNTY_BOARD_ID}
            walletAddress={walletAddress}
            onSuccess={() => {
              setShowPostForm(false);
              fetchBounties();
              fetchEvents();
            }}
          />
        </div>
      )}

      {/* Main content: bounties + activity feed */}
      <div className="flex gap-6">
        {/* Bounty list */}
        <div className="flex-1 min-w-0">
          {loading && bounties.length === 0 ? (
            <p className="text-text-muted">Loading bounties...</p>
          ) : activeBounties.length === 0 ? (
            <div className="glass-card p-8 text-center">
              <p className="text-text-secondary">No active bounties. Be the first to post one!</p>
            </div>
          ) : (
            <div className="flex flex-col gap-3">
              {activeBounties.map((bounty) => (
                <BountyCard
                  key={bounty.id}
                  bounty={bounty}
                  walletAddress={walletAddress}
                  builderPackageId={BUILDER_PACKAGE_ID}
                  bountyBoardId={BOUNTY_BOARD_ID}
                  onAction={() => { fetchBounties(); fetchEvents(); }}
                />
              ))}
            </div>
          )}

          {/* Claimed */}
          {claimedBounties.length > 0 && (
            <div className="mt-8">
              <div className="border-t border-border-default mb-6" />
              <h3 className="text-lg tracking-wider mb-4 text-text-muted">
                CLAIMED <span className="text-text-muted text-sm">({claimedBounties.length})</span>
              </h3>
              <div className="flex flex-col gap-3">
                {claimedBounties.map((bounty) => (
                  <BountyCard
                    key={bounty.id}
                    bounty={bounty}
                    walletAddress={walletAddress}
                    builderPackageId={BUILDER_PACKAGE_ID}
                    bountyBoardId={BOUNTY_BOARD_ID}
                    onAction={() => { fetchBounties(); fetchEvents(); }}
                  />
                ))}
              </div>
            </div>
          )}
        </div>

        {/* Activity Feed sidebar */}
        {events.length > 0 && (
          <div className="w-72 shrink-0 hidden lg:block">
            <ActivityFeed events={events} />
          </div>
        )}
      </div>
    </div>
  );
}
