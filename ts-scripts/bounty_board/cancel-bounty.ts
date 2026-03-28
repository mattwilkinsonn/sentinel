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
 * Cancel a bounty and refund the reward to the poster.
 * Only the original poster can cancel.
 *
 * Required env vars:
 *   PLAYER_A_PRIVATE_KEY - poster's private key
 *   POSTER_CHARACTER_ID - poster's Character object ID
 *   STORAGE_UNIT_ID - the SSU bound to the bounty board
 *   BOUNTY_ID - the bounty ID to cancel
 */
async function main() {
  console.log("============= Cancel Bounty ==============\n");

  try {
    const env = getEnvConfig();
    const playerKey = requireEnv("PLAYER_A_PRIVATE_KEY");
    const ctx = initializeContext(env.network, playerKey);
    const { client, keypair } = ctx;

    const { builderPackageId, bountyBoardId } = resolveBountyBoardIdsFromEnv();

    const posterCharacterId = requireEnv("POSTER_CHARACTER_ID");
    const storageUnitId = requireEnv("STORAGE_UNIT_ID");
    const bountyId = BigInt(requireEnv("BOUNTY_ID"));

    const tx = new Transaction();

    tx.moveCall({
      target: `${builderPackageId}::${MODULE.BOUNTY_BOARD}::cancel_bounty`,
      arguments: [
        tx.object(bountyBoardId),
        tx.object(storageUnitId),
        tx.object(posterCharacterId),
        tx.pure.u64(bountyId),
      ],
    });

    const result = await client.signAndExecuteTransaction({
      transaction: tx,
      signer: keypair,
      options: { showEffects: true, showObjectChanges: true, showEvents: true },
    });

    console.log("Bounty cancelled and reward refunded!");
    console.log("Transaction digest:", result.digest);

    const events = result.events || [];
    const cancelEvent = events.find((e: any) =>
      e.type.includes("BountyCancelledEvent"),
    );
    if (cancelEvent?.parsedJson) {
      const parsed = cancelEvent.parsedJson as any;
      console.log("\nBounty ID:", parsed.bounty_id);
      console.log("Poster:", parsed.poster);
    }
  } catch (error) {
    handleError(error);
  }
}

main();
