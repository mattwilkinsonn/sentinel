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
 * Add more SUI reward to an existing bounty (stack it).
 *
 * Required env vars:
 *   PLAYER_A_PRIVATE_KEY    - contributor's private key
 *   CHARACTER_ID            - contributor's Character object ID
 *   BOUNTY_ID               - the bounty ID to add to
 *   REWARD_SUI              - additional reward in SUI (e.g. "0.5")
 */
async function main() {
  console.log("============= Add To Bounty ==============\n");

  try {
    const env = getEnvConfig();
    const playerKey = requireEnv("PLAYER_A_PRIVATE_KEY");
    const ctx = initializeContext(env.network, playerKey);
    const { client, keypair, config: worldConfig } = ctx;

    const { builderPackageId, bountyBoardId } = resolveBountyBoardIdsFromEnv();

    const characterId = requireEnv("CHARACTER_ID");
    const bountyId = BigInt(requireEnv("BOUNTY_ID"));
    const rewardSui = parseFloat(requireEnv("REWARD_SUI"));
    const rewardMist = BigInt(Math.round(rewardSui * 1_000_000_000));

    console.log(
      `Adding ${rewardSui} SUI (${rewardMist} MIST) to bounty #${bountyId}`,
    );

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

    const tx = new Transaction();

    const [payment] = tx.splitCoins(tx.gas, [tx.pure.u64(rewardMist)]);

    const [ownerCap, returnReceipt] = tx.moveCall({
      target: `${worldConfig.packageId}::${MODULES.CHARACTER}::borrow_owner_cap`,
      typeArguments: [
        `${worldConfig.packageId}::${MODULES.CHARACTER}::Character`,
      ],
      arguments: [tx.object(characterId), tx.object(characterOwnerCapId)],
    });

    tx.moveCall({
      target: `${builderPackageId}::${MODULE.BOUNTY_BOARD}::add_to_bounty`,
      arguments: [
        tx.object(bountyBoardId),
        tx.object(characterId),
        payment,
        tx.pure.u64(bountyId),
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

    console.log("Added to bounty!");
    console.log("Transaction digest:", result.digest);

    const stackedEvent = (result.events || []).find((e: any) =>
      e.type.includes("BountyStackedEvent"),
    );
    if (stackedEvent?.parsedJson) {
      const p = stackedEvent.parsedJson as any;
      console.log("\nBounty ID:    ", p.bounty_id);
      console.log("Added:        ", Number(p.amount_added_mist) / 1e9, "SUI");
      console.log("New total:    ", Number(p.new_total_mist) / 1e9, "SUI");
    }
  } catch (error) {
    handleError(error);
  }
}

main();
