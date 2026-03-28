import { useState } from "react";
import { useDAppKit } from "@mysten/dapp-kit-react";
import { Transaction } from "@mysten/sui/transactions";
import { Send } from "lucide-react";

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
      if (!targetItemId || !rewardTypeId || !storageUnitId || !extensionConfigId || !characterId || !worldPackageId) {
        throw new Error("All fields are required");
      }

      const durationMs = BigInt(Number(durationHours) * 60 * 60 * 1000);
      const characterOwnerCapId = import.meta.env.VITE_CHARACTER_OWNER_CAP_ID || "";
      if (!characterOwnerCapId) {
        throw new Error("VITE_CHARACTER_OWNER_CAP_ID not set");
      }

      const tx = new Transaction();

      const [ownerCap, returnReceipt] = tx.moveCall({
        target: `${worldPackageId}::character::borrow_owner_cap`,
        typeArguments: [`${worldPackageId}::character::Character`],
        arguments: [tx.object(characterId), tx.object(characterOwnerCapId)],
      });

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

      tx.moveCall({
        target: `${worldPackageId}::character::return_owner_cap`,
        typeArguments: [`${worldPackageId}::character::Character`],
        arguments: [tx.object(characterId), ownerCap, returnReceipt],
      });

      await signAndExecuteTransaction({ transaction: tx });
      onSuccess();
    } catch (err) {
      setError(err instanceof Error ? err.message : "Transaction failed");
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <div className="glass-card p-5">
      <h4 className="text-sm tracking-wider mb-4">POST NEW BOUNTY</h4>

      <form onSubmit={handleSubmit}>
        <div className="flex flex-col gap-3">
          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="block text-xs text-text-muted mb-1">Target Character ID</label>
              <input
                value={targetItemId}
                onChange={(e) => setTargetItemId(e.target.value)}
                placeholder="e.g. 12345"
                required
                className="w-full"
              />
            </div>
            <div>
              <label className="block text-xs text-text-muted mb-1">Target Tenant</label>
              <input
                value={targetTenant}
                onChange={(e) => setTargetTenant(e.target.value)}
                placeholder="e.g. dev"
                required
                className="w-full"
              />
            </div>
          </div>

          <div className="grid grid-cols-3 gap-3">
            <div>
              <label className="block text-xs text-text-muted mb-1">Reward Type ID</label>
              <input
                value={rewardTypeId}
                onChange={(e) => setRewardTypeId(e.target.value)}
                placeholder="e.g. 88069"
                required
                className="w-full"
              />
            </div>
            <div>
              <label className="block text-xs text-text-muted mb-1">Reward Quantity</label>
              <input
                type="number"
                min="1"
                value={rewardQuantity}
                onChange={(e) => setRewardQuantity(e.target.value)}
                required
                className="w-full"
              />
            </div>
            <div>
              <label className="block text-xs text-text-muted mb-1">Duration (hours)</label>
              <input
                type="number"
                min="1"
                max="168"
                value={durationHours}
                onChange={(e) => setDurationHours(e.target.value)}
                required
                className="w-full"
              />
            </div>
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="block text-xs text-text-muted mb-1">Storage Unit ID</label>
              <input
                value={storageUnitId}
                onChange={(e) => setStorageUnitId(e.target.value)}
                placeholder="0x..."
                required
                className="w-full"
              />
            </div>
            <div>
              <label className="block text-xs text-text-muted mb-1">Character ID</label>
              <input
                value={characterId}
                onChange={(e) => setCharacterId(e.target.value)}
                placeholder="0x..."
                required
                className="w-full"
              />
            </div>
          </div>

          <div className="grid grid-cols-2 gap-3">
            <div>
              <label className="block text-xs text-text-muted mb-1">Extension Config ID</label>
              <input
                value={extensionConfigId}
                onChange={(e) => setExtensionConfigId(e.target.value)}
                placeholder="0x..."
                required
                className="w-full"
              />
            </div>
            <div>
              <label className="block text-xs text-text-muted mb-1">World Package ID</label>
              <input
                value={worldPackageId}
                onChange={(e) => setWorldPackageId(e.target.value)}
                placeholder="0x..."
                required
                className="w-full"
              />
            </div>
          </div>

          {error && (
            <p className="text-accent-red text-sm">{error}</p>
          )}

          <button
            type="submit"
            disabled={submitting}
            className="flex items-center justify-center gap-2 px-4 py-2.5 bg-accent-cyan/10 border border-accent-cyan text-accent-cyan rounded hover:bg-accent-cyan/20 transition-all disabled:opacity-40 disabled:cursor-not-allowed"
          >
            <Send className="w-3.5 h-3.5" />
            {submitting ? "POSTING..." : "POST BOUNTY"}
          </button>
        </div>
      </form>
    </div>
  );
}
