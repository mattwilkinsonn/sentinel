import { Transaction } from "@mysten/sui/transactions";
import { Send } from "lucide-solid";
import { createSignal } from "solid-js";
import { useWallet } from "./WalletContext";

const CLOCK_OBJECT_ID = "0x6";
const BUILDER_PACKAGE_ID = import.meta.env.VITE_BUILDER_PACKAGE_ID || "";
const BOUNTY_BOARD_ID = import.meta.env.VITE_BOUNTY_BOARD_ID || "";
const WORLD_PACKAGE_ID = import.meta.env.VITE_WORLD_PACKAGE_ID || "";
const EXTENSION_CONFIG_ID = import.meta.env.VITE_EXTENSION_CONFIG_ID || "";

type Props = {
  onSuccess: () => void;
};

export function PostBountyForm(props: Props) {
  const { signAndExecuteTransaction, connectedAddress } = useWallet();

  const [targetItemId, setTargetItemId] = createSignal("");
  const [targetTenant, setTargetTenant] = createSignal("stillness");
  const [rewardTypeId, setRewardTypeId] = createSignal("");
  const [rewardQuantity, setRewardQuantity] = createSignal("1");
  const [durationHours, setDurationHours] = createSignal("24");
  const [storageUnitId, setStorageUnitId] = createSignal("");
  const [characterId, setCharacterId] = createSignal("");
  const [characterOwnerCapId, setCharacterOwnerCapId] = createSignal("");
  const [extensionConfigId, setExtensionConfigId] =
    createSignal(EXTENSION_CONFIG_ID);
  const [worldPackageId, setWorldPackageId] = createSignal(WORLD_PACKAGE_ID);
  const [submitting, setSubmitting] = createSignal(false);
  const [error, setError] = createSignal<string | null>(null);

  function friendlyError(err: unknown): string {
    const msg = err instanceof Error ? err.message : String(err);
    const lower = msg.toLowerCase();
    if (
      lower.includes("rejected") ||
      lower.includes("denied") ||
      lower.includes("cancelled")
    ) {
      return "Transaction cancelled.";
    }
    if (
      lower.includes("wrong chain") ||
      lower.includes("wrong network") ||
      lower.includes("chain mismatch")
    ) {
      return "Wrong network — switch Slush to Testnet and try again.";
    }
    if (
      lower.includes("simulation") ||
      lower.includes("dry run") ||
      lower.includes("move abort")
    ) {
      return `Transaction simulation failed — make sure you're on Testnet. Details: ${msg}`;
    }
    if (lower.includes("insufficient") && lower.includes("gas")) {
      return "Insufficient SUI for gas. Top up your testnet wallet and try again.";
    }
    if (lower.includes("object not found") || lower.includes("not found")) {
      return `Object not found — check your IDs are on Testnet. Details: ${msg}`;
    }
    return msg || "Transaction failed.";
  }

  async function handleSubmit(e: Event) {
    e.preventDefault();
    setSubmitting(true);
    setError(null);

    try {
      if (
        !targetItemId() ||
        !rewardTypeId() ||
        !storageUnitId() ||
        !extensionConfigId() ||
        !characterId() ||
        !characterOwnerCapId() ||
        !worldPackageId()
      ) {
        throw new Error("All fields are required");
      }

      const durationMs = BigInt(Number(durationHours()) * 60 * 60 * 1000);
      const tx = new Transaction();

      const [ownerCap, returnReceipt] = tx.moveCall({
        target: `${worldPackageId()}::character::borrow_owner_cap`,
        typeArguments: [`${worldPackageId()}::character::Character`],
        arguments: [tx.object(characterId()), tx.object(characterOwnerCapId())],
      });

      tx.moveCall({
        target: `${BUILDER_PACKAGE_ID}::bounty_board::post_bounty`,
        typeArguments: [`${worldPackageId()}::character::Character`],
        arguments: [
          tx.object(BOUNTY_BOARD_ID),
          tx.object(extensionConfigId()),
          tx.object(storageUnitId()),
          tx.object(characterId()),
          ownerCap,
          tx.pure.u64(BigInt(targetItemId())),
          tx.pure.string(targetTenant()),
          tx.pure.u64(BigInt(rewardTypeId())),
          tx.pure.u32(Number(rewardQuantity())),
          tx.pure.u64(durationMs),
          tx.object(CLOCK_OBJECT_ID),
        ],
      });

      tx.moveCall({
        target: `${worldPackageId()}::character::return_owner_cap`,
        typeArguments: [`${worldPackageId()}::character::Character`],
        arguments: [tx.object(characterId()), ownerCap, returnReceipt],
      });

      await signAndExecuteTransaction(tx);
      props.onSuccess();
    } catch (err) {
      setError(friendlyError(err));
    } finally {
      setSubmitting(false);
    }
  }

  return (
    <div class="glass-card p-5">
      <h4 class="text-sm tracking-wider" style="margin-bottom:1rem">
        POST NEW BOUNTY
      </h4>
      <p class="text-xs text-text-muted" style="margin-bottom:1rem">
        Posting as:{" "}
        <span class="text-text-secondary font-mono">{connectedAddress()}</span>
      </p>

      <form onSubmit={handleSubmit}>
        <div class="flex flex-col gap-3">
          {/* Target */}
          <div class="grid grid-cols-2 gap-3">
            <div>
              <label
                for="target-character-id"
                class="block text-xs text-text-muted"
                style="margin-bottom:0.25rem"
              >
                Target Character ID
              </label>
              <input
                id="target-character-id"
                value={targetItemId()}
                onInput={(e) => setTargetItemId(e.currentTarget.value)}
                placeholder="e.g. 12345"
                required
                class="w-full"
              />
            </div>
            <div>
              <label
                for="target-tenant"
                class="block text-xs text-text-muted"
                style="margin-bottom:0.25rem"
              >
                Target Tenant
              </label>
              <input
                id="target-tenant"
                value={targetTenant()}
                onInput={(e) => setTargetTenant(e.currentTarget.value)}
                placeholder="e.g. stillness"
                required
                class="w-full"
              />
            </div>
          </div>

          {/* Reward */}
          <div class="grid grid-cols-3 gap-3">
            <div>
              <label
                for="reward-type-id"
                class="block text-xs text-text-muted"
                style="margin-bottom:0.25rem"
              >
                Reward Type ID
              </label>
              <input
                id="reward-type-id"
                value={rewardTypeId()}
                onInput={(e) => setRewardTypeId(e.currentTarget.value)}
                placeholder="e.g. 88069"
                required
                class="w-full"
              />
            </div>
            <div>
              <label
                for="reward-quantity"
                class="block text-xs text-text-muted"
                style="margin-bottom:0.25rem"
              >
                Reward Quantity
              </label>
              <input
                id="reward-quantity"
                type="number"
                min="1"
                value={rewardQuantity()}
                onInput={(e) => setRewardQuantity(e.currentTarget.value)}
                required
                class="w-full"
              />
            </div>
            <div>
              <label
                for="duration-hours"
                class="block text-xs text-text-muted"
                style="margin-bottom:0.25rem"
              >
                Duration (hours)
              </label>
              <input
                id="duration-hours"
                type="number"
                min="1"
                max="168"
                value={durationHours()}
                onInput={(e) => setDurationHours(e.currentTarget.value)}
                required
                class="w-full"
              />
            </div>
          </div>

          {/* Character objects */}
          <div class="grid grid-cols-2 gap-3">
            <div>
              <label
                for="character-object-id"
                class="block text-xs text-text-muted"
                style="margin-bottom:0.25rem"
              >
                Character Object ID
              </label>
              <input
                id="character-object-id"
                value={characterId()}
                onInput={(e) => setCharacterId(e.currentTarget.value)}
                placeholder="0x..."
                required
                class="w-full"
              />
            </div>
            <div>
              <label
                for="character-owner-cap-id"
                class="block text-xs text-text-muted"
                style="margin-bottom:0.25rem"
              >
                Character Owner Cap ID
              </label>
              <input
                id="character-owner-cap-id"
                value={characterOwnerCapId()}
                onInput={(e) => setCharacterOwnerCapId(e.currentTarget.value)}
                placeholder="0x..."
                required
                class="w-full"
              />
            </div>
          </div>

          <div class="grid grid-cols-2 gap-3">
            <div>
              <label
                for="storage-unit-id"
                class="block text-xs text-text-muted"
                style="margin-bottom:0.25rem"
              >
                Storage Unit ID
              </label>
              <input
                id="storage-unit-id"
                value={storageUnitId()}
                onInput={(e) => setStorageUnitId(e.currentTarget.value)}
                placeholder="0x..."
                required
                class="w-full"
              />
            </div>
            <div>
              <label
                for="extension-config-id"
                class="block text-xs text-text-muted"
                style="margin-bottom:0.25rem"
              >
                Extension Config ID
              </label>
              <input
                id="extension-config-id"
                value={extensionConfigId()}
                onInput={(e) => setExtensionConfigId(e.currentTarget.value)}
                placeholder="0x..."
                required
                class="w-full"
              />
            </div>
          </div>

          <div>
            <label
              for="world-package-id"
              class="block text-xs text-text-muted"
              style="margin-bottom:0.25rem"
            >
              World Package ID
            </label>
            <input
              id="world-package-id"
              value={worldPackageId()}
              onInput={(e) => setWorldPackageId(e.currentTarget.value)}
              placeholder="0x..."
              required
              class="w-full font-mono text-xs"
            />
          </div>

          {error() && <p class="text-accent-red text-sm">{error()}</p>}

          <button
            type="submit"
            disabled={submitting()}
            class="flex items-center justify-center gap-2 px-4 py-2.5 bg-accent-cyan/10 border border-accent-cyan text-accent-cyan rounded hover:bg-accent-cyan/20 transition-all disabled:opacity-40 disabled:cursor-not-allowed"
          >
            <Send size={14} />
            {submitting() ? "POSTING..." : "POST BOUNTY"}
          </button>
        </div>
      </form>
    </div>
  );
}
