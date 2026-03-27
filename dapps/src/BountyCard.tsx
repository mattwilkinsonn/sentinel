import { Box, Flex, Text } from "@radix-ui/themes";
import { abbreviateAddress } from "@evefrontier/dapp-kit";
import type { BountyData } from "./BountyBoard";

function formatExpiry(expiresAtMs: string): string {
  const expires = Number(expiresAtMs);
  if (expires === 0) return "N/A";
  const now = Date.now();
  const diff = expires - now;
  if (diff <= 0) return "Expired";
  const hours = Math.floor(diff / (1000 * 60 * 60));
  const mins = Math.floor((diff % (1000 * 60 * 60)) / (1000 * 60));
  if (hours > 24) return `${Math.floor(hours / 24)}d ${hours % 24}h`;
  return `${hours}h ${mins}m`;
}

type BountyCardProps = {
  bounty: BountyData;
  walletAddress: string;
  builderPackageId: string;
  bountyBoardId: string;
  onAction: () => void;
};

export function BountyCard({
  bounty,
  walletAddress,
}: BountyCardProps) {
  const isExpired = Number(bounty.expires_at) < Date.now() && !bounty.claimed;
  const isPoster = bounty.poster.toLowerCase() === walletAddress.toLowerCase();

  return (
    <Box
      style={{
        padding: "16px",
        border: `1px solid ${
          bounty.claimed
            ? "rgba(100, 200, 100, 0.3)"
            : isExpired
            ? "rgba(200, 100, 100, 0.3)"
            : "var(--border)"
        }`,
        borderRadius: "8px",
        opacity: bounty.claimed || isExpired ? 0.6 : 1,
      }}
    >
      <Flex direction="row" justify="between" align="start">
        <Box>
          <Flex gap="2" align="center" style={{ marginBottom: "8px" }}>
            <Text weight="bold" size="3">
              Bounty #{bounty.id}
            </Text>
            {bounty.claimed && (
              <span
                style={{
                  background: "rgba(100, 200, 100, 0.2)",
                  color: "#8f8",
                  padding: "2px 8px",
                  borderRadius: "4px",
                  fontSize: "12px",
                }}
              >
                CLAIMED
              </span>
            )}
            {isExpired && (
              <span
                style={{
                  background: "rgba(200, 100, 100, 0.2)",
                  color: "#f88",
                  padding: "2px 8px",
                  borderRadius: "4px",
                  fontSize: "12px",
                }}
              >
                EXPIRED
              </span>
            )}
            {isPoster && (
              <span
                style={{
                  background: "rgba(100, 100, 200, 0.2)",
                  color: "#88f",
                  padding: "2px 8px",
                  borderRadius: "4px",
                  fontSize: "12px",
                }}
              >
                YOUR BOUNTY
              </span>
            )}
          </Flex>

          <Flex direction="column" gap="1">
            <Text size="2" style={{ color: "var(--text-secondary)" }}>
              Target: Character #{bounty.target_item_id} ({bounty.target_tenant})
            </Text>
            <Text size="2" style={{ color: "var(--text-secondary)" }}>
              Posted by: {abbreviateAddress(bounty.poster)}
            </Text>
            {bounty.claimed && bounty.claimed_by && (
              <Text size="2" style={{ color: "#8f8" }}>
                Claimed by: {abbreviateAddress(bounty.claimed_by)}
              </Text>
            )}
          </Flex>
        </Box>

        <Box style={{ textAlign: "right" }}>
          <Text weight="bold" size="4" style={{ color: "#ffd700" }}>
            {bounty.reward_quantity}x
          </Text>
          <Text size="2" style={{ color: "var(--text-secondary)", display: "block" }}>
            Type #{bounty.reward_type_id}
          </Text>
          {!bounty.claimed && (
            <Text
              size="1"
              style={{
                color: isExpired ? "#f88" : "var(--text-secondary)",
                display: "block",
                marginTop: "4px",
              }}
            >
              {formatExpiry(bounty.expires_at)}
            </Text>
          )}
        </Box>
      </Flex>
    </Box>
  );
}
