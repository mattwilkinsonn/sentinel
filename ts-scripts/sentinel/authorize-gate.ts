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
  console.log("============= Authorize Sentinel on Gate ==============\n");

  try {
    const env = getEnvConfig();
    const ctx = initializeContext(env.network, env.adminExportedKey);
    const { client, keypair, address } = ctx;

    const { sentinelPackageId } = await resolveSentinelExtensionIds(
      client,
      address,
    );

    const gateId = requireEnv("GATE_ID");
    const characterId = requireEnv("CHARACTER_ID");
    const worldPackageId = requireEnv("WORLD_PACKAGE_ID");

    // Get the gate's OwnerCap
    const tx = new Transaction();

    // Borrow OwnerCap from Character
    const characterOwnerCapId = requireEnv("CHARACTER_OWNER_CAP_ID");
    const [ownerCap, returnReceipt] = tx.moveCall({
      target: `${worldPackageId}::character::borrow_owner_cap`,
      typeArguments: [`${worldPackageId}::gate::Gate`],
      arguments: [tx.object(characterId), tx.object(characterOwnerCapId)],
    });

    // Authorize SentinelAuth extension on the gate
    tx.moveCall({
      target: `${worldPackageId}::gate::authorize_extension`,
      typeArguments: [`${sentinelPackageId}::${MODULE.CONFIG}::SentinelAuth`],
      arguments: [tx.object(gateId), ownerCap],
    });

    // Return OwnerCap
    tx.moveCall({
      target: `${worldPackageId}::character::return_owner_cap`,
      typeArguments: [`${worldPackageId}::gate::Gate`],
      arguments: [tx.object(characterId), ownerCap, returnReceipt],
    });

    const result = await client.signAndExecuteTransaction({
      transaction: tx,
      signer: keypair,
      options: { showEffects: true },
    });

    console.log("SentinelAuth authorized on gate:", gateId);
    console.log("Transaction digest:", result.digest);
  } catch (error) {
    handleError(error);
  }
}

main();
