import { useState } from "react";
import { Box, Flex, Text } from "@radix-ui/themes";
import { useDAppKit } from "@mysten/dapp-kit-react";
import { Transaction } from "@mysten/sui/transactions";

const CLOCK_OBJECT_ID = "0x6";

type PostBountyFormProps = {
  builderPackageId: string;
  bountyBoardId: string;
  walletAddress: string;
  onSuccess: () => void;
};

export function PostBountyForm({
  builderPackageId,
  bountyBoardId,
  walletAddress,
  onSuccess,
}: PostBountyFormProps) {
  const { signAndExecuteTransaction } = useDAppKit();

  const [targetItemId, setTargetItemId] = useState("");
  const [targetTenant, setTargetTenant] = useState("dev");
  const [rewardTypeId, setRewardTypeId] = useState("");
  const [rewardQuantity, setRewardQuantity] = useState("1");
  const [durationHours, setDurationHours] = useState("24");
  const [storageUnitId, setStorageUnitId] = useState(
    import.meta.env.VITE_STORAGE_UNIT_ID || ""
  );
  const [extensionConfigId, setExtensionConfigId] = useState(
    import.meta.env.VITE_EXTENSION_CONFIG_ID || ""
  );
  const [characterId, setCharacterId] = useState(
    import.meta.env.VITE_CHARACTER_ID || ""
  );
  const [worldPackageId, setWorldPackageId] = useState(
    import.meta.env.VITE_WORLD_PACKAGE_ID || ""
  );
  const [submitting, setSubmitting] = useState(false);
  const [error, setError] = useState<string | null>(null);

  const handleSubmit = async (e: React.FormEvent) => {
    e.preventDefault();
    setSubmitting(true);
    setError(null);

    try {
      if (
        !targetItemId ||
        !rewardTypeId ||
        !storageUnitId ||
        !extensionConfigId ||
        !characterId ||
        !worldPackageId
      ) {
        throw new Error("All fields are required");
      }

      const durationMs = BigInt(Number(durationHours) * 60 * 60 * 1000);

      // Look up the Character's OwnerCap ID
      // For now, the user needs to provide it or we fetch from chain
      const characterOwnerCapId =
        import.meta.env.VITE_CHARACTER_OWNER_CAP_ID || "";
      if (!characterOwnerCapId) {
        throw new Error(
          "VITE_CHARACTER_OWNER_CAP_ID not set - needed for borrow_owner_cap"
        );
      }

      const tx = new Transaction();

      // Borrow Character OwnerCap
      const [ownerCap, returnReceipt] = tx.moveCall({
        target: `${worldPackageId}::character::borrow_owner_cap`,
        typeArguments: [`${worldPackageId}::character::Character`],
        arguments: [tx.object(characterId), tx.object(characterOwnerCapId)],
      });

      // Post bounty
      tx.moveCall({
        target: `${builderPackageId}::bounty_board::post_bounty`,
        typeArguments: [`${worldPackageId}::character::Character`],
        arguments: [
          tx.object(bountyBoardId),
          tx.object(extensionConfigId),
          tx.object(storageUnitId),
          tx.object(characterId),
          ownerCap,
          tx.pure.u64(BigInt(targetItemId)),
          tx.pure.string(targetTenant),
          tx.pure.u64(BigInt(rewardTypeId)),
          tx.pure.u32(Number(rewardQuantity)),
          tx.pure.u64(durationMs),
          tx.object(CLOCK_OBJECT_ID),
        ],
      });

      // Return OwnerCap
      tx.moveCall({
        target: `${worldPackageId}::character::return_owner_cap`,
        typeArguments: [`${worldPackageId}::character::Character`],
        arguments: [tx.object(characterId), ownerCap, returnReceipt],
      });

      await signAndExecuteTransaction({
        transaction: tx,
      });

      onSuccess();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Transaction failed");
    } finally {
      setSubmitting(false);
    }
  };

  const inputStyle: React.CSSProperties = {
    background: "rgba(250, 250, 229, 0.05)",
    border: "1px solid var(--border)",
    borderRadius: "4px",
    padding: "8px 12px",
    color: "var(--text)",
    fontFamily: "var(--font-mono)",
    fontSize: "14px",
    width: "100%",
  };

  const labelStyle: React.CSSProperties = {
    fontSize: "12px",
    color: "var(--text-secondary)",
    marginBottom: "4px",
  };

  return (
    <Box
      style={{
        padding: "20px",
        border: "1px solid var(--border)",
        borderRadius: "8px",
        marginBottom: "20px",
      }}
    >
      <Text weight="bold" size="3" style={{ marginBottom: "16px", display: "block" }}>
        Post New Bounty
      </Text>

      <form onSubmit={handleSubmit}>
        <Flex direction="column" gap="3">
          <Flex gap="3">
            <Box style={{ flex: 1 }}>
              <div style={labelStyle}>Target Character ID</div>
              <input
                style={inputStyle}
                value={targetItemId}
                onChange={(e) => setTargetItemId(e.target.value)}
                placeholder="e.g. 12345"
                required
              />
            </Box>
            <Box style={{ flex: 1 }}>
              <div style={labelStyle}>Target Tenant</div>
              <input
                style={inputStyle}
                value={targetTenant}
                onChange={(e) => setTargetTenant(e.target.value)}
                placeholder="e.g. dev"
                required
              />
            </Box>
          </Flex>

          <Flex gap="3">
            <Box style={{ flex: 1 }}>
              <div style={labelStyle}>Reward Type ID</div>
              <input
                style={inputStyle}
                value={rewardTypeId}
                onChange={(e) => setRewardTypeId(e.target.value)}
                placeholder="e.g. 88069"
                required
              />
            </Box>
            <Box style={{ flex: 1 }}>
              <div style={labelStyle}>Reward Quantity</div>
              <input
                style={inputStyle}
                type="number"
                min="1"
                value={rewardQuantity}
                onChange={(e) => setRewardQuantity(e.target.value)}
                required
              />
            </Box>
            <Box style={{ flex: 1 }}>
              <div style={labelStyle}>Duration (hours)</div>
              <input
                style={inputStyle}
                type="number"
                min="1"
                max="168"
                value={durationHours}
                onChange={(e) => setDurationHours(e.target.value)}
                required
              />
            </Box>
          </Flex>

          <Flex gap="3">
            <Box style={{ flex: 1 }}>
              <div style={labelStyle}>Storage Unit ID</div>
              <input
                style={inputStyle}
                value={storageUnitId}
                onChange={(e) => setStorageUnitId(e.target.value)}
                placeholder="0x..."
                required
              />
            </Box>
            <Box style={{ flex: 1 }}>
              <div style={labelStyle}>Character ID</div>
              <input
                style={inputStyle}
                value={characterId}
                onChange={(e) => setCharacterId(e.target.value)}
                placeholder="0x..."
                required
              />
            </Box>
          </Flex>

          <Flex gap="3">
            <Box style={{ flex: 1 }}>
              <div style={labelStyle}>Extension Config ID</div>
              <input
                style={inputStyle}
                value={extensionConfigId}
                onChange={(e) => setExtensionConfigId(e.target.value)}
                placeholder="0x..."
                required
              />
            </Box>
            <Box style={{ flex: 1 }}>
              <div style={labelStyle}>World Package ID</div>
              <input
                style={inputStyle}
                value={worldPackageId}
                onChange={(e) => setWorldPackageId(e.target.value)}
                placeholder="0x..."
                required
              />
            </Box>
          </Flex>

          {error && (
            <Text color="red" size="2">
              {error}
            </Text>
          )}

          <button type="submit" disabled={submitting}>
            {submitting ? "Posting..." : "Post Bounty"}
          </button>
        </Flex>
      </form>
    </Box>
  );
}
