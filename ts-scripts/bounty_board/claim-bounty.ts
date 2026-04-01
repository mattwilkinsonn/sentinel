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
 * SUI reward is transferred directly to the hunter's wallet — no SSU required.
 *
 * Required env vars:
 *   PLAYER_A_PRIVATE_KEY   - hunter's private key
 *   HUNTER_CHARACTER_ID    - hunter's Character object ID
 *   KILLMAIL_ID            - Killmail object ID proving the kill
 *   BOUNTY_ID              - bounty ID to claim
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
    const killmailId = requireEnv("KILLMAIL_ID");
    const bountyId = BigInt(requireEnv("BOUNTY_ID"));

    const tx = new Transaction();
    tx.moveCall({
      target: `${builderPackageId}::${MODULE.BOUNTY_BOARD}::claim_bounty`,
      arguments: [
        tx.object(bountyBoardId),
        tx.object(extensionConfigId),
        tx.object(hunterCharacterId),
        tx.object(killmailId),
        tx.pure.u64(bountyId),
        tx.object(CLOCK_OBJECT_ID),
      ],
    });

    const result = await client.signAndExecuteTransaction({
      transaction: tx,
      signer: keypair,
      options: { showEffects: true, showEvents: true },
    });

    console.log("Bounty claimed!");
    console.log("Transaction digest:", result.digest);

    const claimEvent = (result.events || []).find((e: any) =>
      e.type.includes("BountyClaimedEvent"),
    );
    if (claimEvent?.parsedJson) {
      const p = claimEvent.parsedJson as any;
      console.log("\nBounty ID: ", p.bounty_id);
      console.log("Hunter:    ", p.hunter);
      console.log("Killmail:  ", p.killmail_id);
      console.log("Reward:    ", Number(p.reward_mist) / 1e9, "SUI");
    }
  } catch (error) {
    handleError(error);
  }
}

main();
