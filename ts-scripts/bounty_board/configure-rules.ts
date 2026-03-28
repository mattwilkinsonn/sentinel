import { config } from "dotenv";

config({ path: "../.env" });

import { Transaction } from "@mysten/sui/transactions";
import { getEnvConfig, handleError, initializeContext } from "../utils/helper";
import { resolveBountyBoardExtensionIds } from "./extension-ids";
import { MODULE } from "./modules";

async function main() {
  console.log("============= Configure Bounty Board Rules ==============\n");

  try {
    const env = getEnvConfig();
    const ctx = initializeContext(env.network, env.adminExportedKey);
    const { client, keypair, address } = ctx;

    const { builderPackageId, adminCapId, extensionConfigId } =
      await resolveBountyBoardExtensionIds(client, address);

    const tx = new Transaction();

    // Set board config:
    //   max_bounty_duration_ms: 7 days
    //   default_bounty_duration_ms: 24 hours
    //   min_killmail_recency_ms: 7 days (0 = no check)
    const SEVEN_DAYS_MS = 7 * 24 * 60 * 60 * 1000;
    const ONE_DAY_MS = 24 * 60 * 60 * 1000;

    tx.moveCall({
      target: `${builderPackageId}::${MODULE.BOUNTY_BOARD}::set_board_config`,
      arguments: [
        tx.object(extensionConfigId),
        tx.object(adminCapId),
        tx.pure.u64(SEVEN_DAYS_MS), // max_bounty_duration_ms
        tx.pure.u64(ONE_DAY_MS), // default_bounty_duration_ms
        tx.pure.u64(SEVEN_DAYS_MS), // min_killmail_recency_ms
      ],
    });

    const result = await client.signAndExecuteTransaction({
      transaction: tx,
      signer: keypair,
      options: { showEffects: true, showObjectChanges: true },
    });

    console.log("Bounty board rules configured!");
    console.log("Transaction digest:", result.digest);
  } catch (error) {
    handleError(error);
  }
}

main();
