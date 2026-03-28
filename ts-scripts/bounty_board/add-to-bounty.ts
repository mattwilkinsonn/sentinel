import { config } from "dotenv";

config({ path: "../.env" });

import { Transaction } from "@mysten/sui/transactions";
import { MODULES } from "../utils/config";
import { CLOCK_OBJECT_ID } from "../utils/constants";
import {
  getEnvConfig,
  handleError,
  initializeContext,
  requireEnv,
} from "../utils/helper";
import { resolveBountyBoardIdsFromEnv } from "./extension-ids";
import { MODULE } from "./modules";

/**
 * Add additional reward to an existing bounty.
 *
 * Flow:
 * 1. Borrow Character OwnerCap
 * 2. Call add_to_bounty (withdraws additional reward from contributor's inventory)
 * 3. Return OwnerCap
 *
 * Required env vars:
 *   PLAYER_A_PRIVATE_KEY - contributor's private key
 *   CHARACTER_ID - contributor's Character object ID
 *   STORAGE_UNIT_ID - the SSU bound to the bounty board
 *   BOUNTY_ID - the bounty ID to add to
 *   REWARD_QUANTITY - quantity of reward to add
 */
async function main() {
  console.log("============= Add To Bounty ==============\n");

  try {
    const env = getEnvConfig();
    const playerKey = requireEnv("PLAYER_A_PRIVATE_KEY");
    const ctx = initializeContext(env.network, playerKey);
    const { client, keypair, config } = ctx;

    const { builderPackageId, extensionConfigId, bountyBoardId } =
      resolveBountyBoardIdsFromEnv();

    const characterId = requireEnv("CHARACTER_ID");
    const storageUnitId = requireEnv("STORAGE_UNIT_ID");
    const bountyId = BigInt(requireEnv("BOUNTY_ID"));
    const rewardQuantity = Number(requireEnv("REWARD_QUANTITY"));

    // Find the Character's OwnerCap for the Character object
    const characterObj = await client.getObject({
      id: characterId,
      options: { showContent: true },
    });
    const characterOwnerCapId = (characterObj.data?.content as any)?.fields
      ?.owner_cap_id;
    if (!characterOwnerCapId) {
      throw new Error(
        `Could not find owner_cap_id on Character ${characterId}`,
      );
    }

    const tx = new Transaction();

    // Borrow Character OwnerCap
    const [ownerCap, returnReceipt] = tx.moveCall({
      target: `${config.packageId}::${MODULES.CHARACTER}::borrow_owner_cap`,
      typeArguments: [`${config.packageId}::${MODULES.CHARACTER}::Character`],
      arguments: [tx.object(characterId), tx.object(characterOwnerCapId)],
    });

    // Add to bounty
    tx.moveCall({
      target: `${builderPackageId}::${MODULE.BOUNTY_BOARD}::add_to_bounty`,
      typeArguments: [`${config.packageId}::${MODULES.CHARACTER}::Character`],
      arguments: [
        tx.object(bountyBoardId),
        tx.object(extensionConfigId),
        tx.object(storageUnitId),
        tx.object(characterId),
        ownerCap,
        tx.pure.u64(bountyId),
        tx.pure.u32(rewardQuantity),
        tx.object(CLOCK_OBJECT_ID),
      ],
    });

    // Return OwnerCap
    tx.moveCall({
      target: `${config.packageId}::${MODULES.CHARACTER}::return_owner_cap`,
      typeArguments: [`${config.packageId}::${MODULES.CHARACTER}::Character`],
      arguments: [tx.object(characterId), ownerCap, returnReceipt],
    });

    const result = await client.signAndExecuteTransaction({
      transaction: tx,
      signer: keypair,
      options: { showEffects: true, showObjectChanges: true, showEvents: true },
    });

    console.log("Added to bounty!");
    console.log("Transaction digest:", result.digest);

    // Extract event
    const events = result.events || [];
    const stackedEvent = events.find((e: any) =>
      e.type.includes("BountyStackedEvent"),
    );
    if (stackedEvent?.parsedJson) {
      const parsed = stackedEvent.parsedJson as any;
      console.log("\nBounty ID:", parsed.bounty_id);
      console.log("Reward quantity added:", parsed.reward_quantity_added);
      console.log("New total quantity:", parsed.new_total_quantity);
    }
  } catch (error) {
    handleError(error);
  }
}

main();
