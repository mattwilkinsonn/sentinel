import { useState, useEffect, useCallback } from "react";
import { Box, Flex, Heading, Text } from "@radix-ui/themes";
import {
  getObjectWithJson,
  getObjectWithDynamicFields,
} from "@evefrontier/dapp-kit";
import { PostBountyForm } from "./PostBountyForm";
import { BountyCard } from "./BountyCard";

// Set these via environment variables or hardcode after deployment
const BOUNTY_BOARD_ID = import.meta.env.VITE_BOUNTY_BOARD_ID || "";
const BUILDER_PACKAGE_ID = import.meta.env.VITE_BUILDER_PACKAGE_ID || "";

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
};

export function BountyBoard({ walletAddress }: { walletAddress: string }) {
  const [bounties, setBounties] = useState<BountyData[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState<string | null>(null);
  const [showPostForm, setShowPostForm] = useState(false);

  const fetchBounties = useCallback(async () => {
    if (!BOUNTY_BOARD_ID) {
      setError("VITE_BOUNTY_BOARD_ID not configured");
      setLoading(false);
      return;
    }

    try {
      setLoading(true);
      setError(null);

      // Fetch the board object to get active bounty IDs
      const boardResult = await getObjectWithJson(BOUNTY_BOARD_ID);
      const boardJson = boardResult.data?.object?.asMoveObject?.contents?.json as any;

      if (!boardJson) {
        setError("Could not load bounty board data");
        setLoading(false);
        return;
      }

      const activeBountyIds: number[] = boardJson.active_bounty_ids || [];

      // Fetch dynamic fields (bounties) from the board
      const dfResult = await getObjectWithDynamicFields(BOUNTY_BOARD_ID);
      const dynamicFields =
        dfResult.data?.object?.asMoveObject?.dynamicFields?.nodes || [];

      const loadedBounties: BountyData[] = [];

      for (const df of dynamicFields) {
        const fieldJson = df?.value?.contents?.json as any;
        if (!fieldJson || fieldJson.id === undefined) continue;

        const bountyId = Number(fieldJson.id);
        loadedBounties.push({
          id: bountyId,
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
        });
      }

      // Sort: active first, then by ID descending
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

  useEffect(() => {
    fetchBounties();
  }, [fetchBounties]);

  const activeBounties = bounties.filter((b) => !b.claimed);
  const claimedBounties = bounties.filter((b) => b.claimed);

  return (
    <Box style={{ padding: "20px 0" }}>
      <Flex
        direction="row"
        justify="between"
        align="center"
        style={{ marginBottom: "20px" }}
      >
        <Heading size="4">
          Active Bounties ({activeBounties.length})
        </Heading>
        <Flex gap="2">
          <button onClick={fetchBounties} disabled={loading}>
            {loading ? "Loading..." : "Refresh"}
          </button>
          <button onClick={() => setShowPostForm(!showPostForm)}>
            {showPostForm ? "Cancel" : "+ Post Bounty"}
          </button>
        </Flex>
      </Flex>

      {error && (
        <Box
          style={{
            padding: "12px",
            marginBottom: "16px",
            border: "1px solid #ff4444",
            borderRadius: "4px",
          }}
        >
          <Text color="red">{error}</Text>
        </Box>
      )}

      {showPostForm && (
        <>
          <PostBountyForm
            builderPackageId={BUILDER_PACKAGE_ID}
            bountyBoardId={BOUNTY_BOARD_ID}
            walletAddress={walletAddress}
            onSuccess={() => {
              setShowPostForm(false);
              fetchBounties();
            }}
          />
          <div className="divider" />
        </>
      )}

      {loading && bounties.length === 0 ? (
        <Text>Loading bounties...</Text>
      ) : activeBounties.length === 0 ? (
        <Text style={{ color: "var(--text-secondary)" }}>
          No active bounties. Be the first to post one!
        </Text>
      ) : (
        <Flex direction="column" gap="3">
          {activeBounties.map((bounty) => (
            <BountyCard
              key={bounty.id}
              bounty={bounty}
              walletAddress={walletAddress}
              builderPackageId={BUILDER_PACKAGE_ID}
              bountyBoardId={BOUNTY_BOARD_ID}
              onAction={fetchBounties}
            />
          ))}
        </Flex>
      )}

      {claimedBounties.length > 0 && (
        <>
          <div className="divider" />
          <Heading size="3" style={{ marginBottom: "12px" }}>
            Claimed ({claimedBounties.length})
          </Heading>
          <Flex direction="column" gap="3">
            {claimedBounties.map((bounty) => (
              <BountyCard
                key={bounty.id}
                bounty={bounty}
                walletAddress={walletAddress}
                builderPackageId={BUILDER_PACKAGE_ID}
                bountyBoardId={BOUNTY_BOARD_ID}
                onAction={fetchBounties}
              />
            ))}
          </Flex>
        </>
      )}
    </Box>
  );
}
