import { config } from "dotenv";

config({ path: "../.env" });

import { Transaction } from "@mysten/sui/transactions";
import { MODULES } from "../utils/config";
import { CLOCK_OBJECT_ID, TENANT } from "../utils/constants";
import {
  getEnvConfig,
  handleError,
  initializeContext,
  requireEnv,
} from "../utils/helper";
import { resolveBountyBoardIdsFromEnv } from "./extension-ids";
import { MODULE } from "./modules";

/**
 * Post a bounty on a target character using SUI coin as reward.
 *
 * The poster locks SUI coins into the bounty. No SSU or item inventory required.
 *
 * Required env vars:
 *   PLAYER_A_PRIVATE_KEY        - poster's private key
 *   CHARACTER_ID                - poster's Character object ID
 *   TARGET_CHARACTER_ITEM_ID    - in-game item ID of the target character
 *   REWARD_SUI                  - reward amount in SUI (e.g. "1.5" for 1.5 SUI)
 *   BOUNTY_DURATION_MS          - (optional) duration in ms, 0 = use board default
 *   TARGET_TENANT               - (optional) defaults to env TENANT constant
 */
async function main() {
  console.log("============= Post Bounty ==============\n");

  try {
    const env = getEnvConfig();
    const playerKey = requireEnv("PLAYER_A_PRIVATE_KEY");
    const ctx = initializeContext(env.network, playerKey);
    const { client, keypair, config: worldConfig } = ctx;

    const { builderPackageId, extensionConfigId, bountyBoardId } =
      resolveBountyBoardIdsFromEnv();

    const characterId = requireEnv("CHARACTER_ID");
    const targetCharacterItemId = BigInt(
      requireEnv("TARGET_CHARACTER_ITEM_ID"),
    );
    const targetTenant = process.env.TARGET_TENANT || TENANT;
    const rewardSui = parseFloat(requireEnv("REWARD_SUI"));
    const rewardMist = BigInt(Math.round(rewardSui * 1_000_000_000));
    const durationMs = BigInt(process.env.BOUNTY_DURATION_MS || "0");

    console.log(
      `Posting bounty: ${rewardSui} SUI (${rewardMist} MIST) on target ${targetCharacterItemId}`,
    );

    const tx = new Transaction();

    // Split the reward coin from gas
    const [payment] = tx.splitCoins(tx.gas, [tx.pure.u64(rewardMist)]);

    // Borrow Character OwnerCap to authenticate the poster
    const characterObj = await client.getObject({
      id: characterId,
      options: { showContent: true },
    });
    const characterOwnerCapId = (characterObj.data?.content as any)?.fields
      ?.owner_cap_id;
    if (!characterOwnerCapId)
      throw new Error(
        `Could not find owner_cap_id on Character ${characterId}`,
      );

    const [ownerCap, returnReceipt] = tx.moveCall({
      target: `${worldConfig.packageId}::${MODULES.CHARACTER}::borrow_owner_cap`,
      typeArguments: [
        `${worldConfig.packageId}::${MODULES.CHARACTER}::Character`,
      ],
      arguments: [tx.object(characterId), tx.object(characterOwnerCapId)],
    });

    tx.moveCall({
      target: `${builderPackageId}::${MODULE.BOUNTY_BOARD}::post_bounty`,
      arguments: [
        tx.object(bountyBoardId),
        tx.object(extensionConfigId),
        tx.object(characterId),
        payment,
        tx.pure.u64(targetCharacterItemId),
        tx.pure.string(targetTenant),
        tx.pure.u64(durationMs),
        tx.object(CLOCK_OBJECT_ID),
      ],
    });

    tx.moveCall({
      target: `${worldConfig.packageId}::${MODULES.CHARACTER}::return_owner_cap`,
      typeArguments: [
        `${worldConfig.packageId}::${MODULES.CHARACTER}::Character`,
      ],
      arguments: [tx.object(characterId), ownerCap, returnReceipt],
    });

    const result = await client.signAndExecuteTransaction({
      transaction: tx,
      signer: keypair,
      options: { showEffects: true, showEvents: true },
    });

    console.log("Bounty posted!");
    console.log("Transaction digest:", result.digest);

    const postEvent = (result.events || []).find((e: any) =>
      e.type.includes("BountyPostedEvent"),
    );
    if (postEvent?.parsedJson) {
      const p = postEvent.parsedJson as any;
      console.log("\nBounty ID:  ", p.bounty_id);
      console.log("Target:     ", p.target_item_id);
      console.log("Reward:     ", Number(p.reward_mist) / 1e9, "SUI");
      console.log("Expires at: ", new Date(Number(p.expires_at)).toISOString());
    }
  } catch (error) {
    handleError(error);
  }
}

main();
