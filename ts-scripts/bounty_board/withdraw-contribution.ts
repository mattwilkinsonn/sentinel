import { config } from "dotenv";

config({ path: "../.env" });

import { Transaction } from "@mysten/sui/transactions";
import {
  getEnvConfig,
  handleError,
  initializeContext,
  requireEnv,
} from "../utils/helper";
import { resolveBountyBoardIdsFromEnv } from "./extension-ids";
import { MODULE } from "./modules";

/**
 * Withdraw your contribution from a bounty.
 * Any contributor can call this to withdraw their own contribution.
 *
 * Required env vars:
 *   PLAYER_A_PRIVATE_KEY - contributor's private key
 *   CHARACTER_ID - contributor's Character object ID
 *   STORAGE_UNIT_ID - the SSU bound to the bounty board
 *   BOUNTY_ID - the bounty ID to withdraw from
 */
async function main() {
  console.log("============= Withdraw Contribution ==============\n");

  try {
    const env = getEnvConfig();
    const playerKey = requireEnv("PLAYER_A_PRIVATE_KEY");
    const ctx = initializeContext(env.network, playerKey);
    const { client, keypair } = ctx;

    const { builderPackageId, bountyBoardId } = resolveBountyBoardIdsFromEnv();

    const characterId = requireEnv("CHARACTER_ID");
    const storageUnitId = requireEnv("STORAGE_UNIT_ID");
    const bountyId = BigInt(requireEnv("BOUNTY_ID"));

    const tx = new Transaction();

    tx.moveCall({
      target: `${builderPackageId}::${MODULE.BOUNTY_BOARD}::withdraw_my_contribution`,
      arguments: [
        tx.object(bountyBoardId),
        tx.object(storageUnitId),
        tx.object(characterId),
        tx.pure.u64(bountyId),
      ],
    });

    const result = await client.signAndExecuteTransaction({
      transaction: tx,
      signer: keypair,
      options: { showEffects: true, showObjectChanges: true, showEvents: true },
    });

    console.log("Contribution withdrawn!");
    console.log("Transaction digest:", result.digest);

    const events = result.events || [];
    const withdrawEvent = events.find((e: any) =>
      e.type.includes("ContributionWithdrawnEvent"),
    );
    if (withdrawEvent?.parsedJson) {
      const parsed = withdrawEvent.parsedJson as any;
      console.log("\nBounty ID:", parsed.bounty_id);
      console.log("Amount withdrawn:", parsed.amount_withdrawn);
      console.log("Remaining total:", parsed.remaining_total);
    }
  } catch (error) {
    handleError(error);
  }
}

main();
