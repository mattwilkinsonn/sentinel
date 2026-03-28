import { config } from "dotenv";

config({ path: "../.env" });

import { Transaction } from "@mysten/sui/transactions";
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
 * Claim a bounty by providing a Killmail as proof-of-kill.
 *
 * The hunter doesn't need to borrow an OwnerCap — the contract uses
 * deposit_to_owned which doesn't require the recipient to be tx sender.
 * However, the contract verifies killer_id == hunter_character.key().
 *
 * Required env vars:
 *   PLAYER_A_PRIVATE_KEY - hunter's private key (or any sender)
 *   HUNTER_CHARACTER_ID - hunter's Character object ID
 *   STORAGE_UNIT_ID - the SSU bound to the bounty board
 *   KILLMAIL_ID - the Killmail object ID proving the kill
 *   BOUNTY_ID - the bounty ID to claim
 */
async function main() {
  console.log("============= Claim Bounty ==============\n");

  try {
    const env = getEnvConfig();
    const playerKey = requireEnv("PLAYER_A_PRIVATE_KEY");
    const ctx = initializeContext(env.network, playerKey);
    const { client, keypair } = ctx;

    const { builderPackageId, extensionConfigId, bountyBoardId } =
      resolveBountyBoardIdsFromEnv();

    const hunterCharacterId = requireEnv("HUNTER_CHARACTER_ID");
    const storageUnitId = requireEnv("STORAGE_UNIT_ID");
    const killmailId = requireEnv("KILLMAIL_ID");
    const bountyId = BigInt(requireEnv("BOUNTY_ID"));

    const tx = new Transaction();

    tx.moveCall({
      target: `${builderPackageId}::${MODULE.BOUNTY_BOARD}::claim_bounty`,
      arguments: [
        tx.object(bountyBoardId),
        tx.object(extensionConfigId),
        tx.object(storageUnitId),
        tx.object(hunterCharacterId),
        tx.object(killmailId),
        tx.pure.u64(bountyId),
        tx.object(CLOCK_OBJECT_ID),
      ],
    });

    const result = await client.signAndExecuteTransaction({
      transaction: tx,
      signer: keypair,
      options: { showEffects: true, showObjectChanges: true, showEvents: true },
    });

    console.log("Bounty claimed!");
    console.log("Transaction digest:", result.digest);

    const events = result.events || [];
    const claimEvent = events.find((e: any) =>
      e.type.includes("BountyClaimedEvent"),
    );
    if (claimEvent?.parsedJson) {
      const parsed = claimEvent.parsedJson as any;
      console.log("\nBounty ID:", parsed.bounty_id);
      console.log("Hunter:", parsed.hunter);
      console.log("Killmail:", parsed.killmail_id);
      console.log(
        "Reward:",
        parsed.reward_quantity,
        "x type",
        parsed.reward_type_id,
      );
    }
  } catch (error) {
    handleError(error);
  }
}

main();
