import { config } from "dotenv";

config({ path: "../.env" });

import { Transaction } from "@mysten/sui/transactions";
import {
  getEnvConfig,
  handleError,
  initializeContext,
  requireEnv,
} from "../utils/helper";
import { resolveSentinelExtensionIds } from "./extension-ids";
import { MODULE } from "./modules";

async function main() {
  console.log("============= Configure Gate Threshold ==============\n");

  try {
    const env = getEnvConfig();
    const ctx = initializeContext(env.network, env.adminExportedKey);
    const { client, keypair, address } = ctx;

    const { sentinelPackageId, adminCapId, extensionConfigId } =
      await resolveSentinelExtensionIds(client, address);

    const maxThreatScore = requireEnv("GATE_MAX_THREAT_SCORE");

    const tx = new Transaction();

    tx.moveCall({
      target: `${sentinelPackageId}::${MODULE.SMART_GATE}::set_gate_threshold`,
      arguments: [
        tx.object(extensionConfigId),
        tx.object(adminCapId),
        tx.pure.u64(BigInt(maxThreatScore)),
      ],
    });

    const result = await client.signAndExecuteTransaction({
      transaction: tx,
      signer: keypair,
      options: { showEffects: true },
    });

    console.log("Gate threshold set to:", maxThreatScore);
    console.log("Transaction digest:", result.digest);
  } catch (error) {
    handleError(error);
  }
}

main();
