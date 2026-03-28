import { config } from "dotenv";
config({ path: "../.env" });
import { Transaction } from "@mysten/sui/transactions";
import {
  getEnvConfig,
  handleError,
  initializeContext,
  requireEnv,
} from "../utils/helper";
import { MODULE } from "./modules";

const CLOCK_OBJECT_ID = "0x6";

async function main() {
  console.log("============= Create Threat Registry ==============\n");

  try {
    const env = getEnvConfig();
    const ctx = initializeContext(env.network, env.adminExportedKey);
    const { client, keypair, address } = ctx;

    const sentinelPackageId = requireEnv("SENTINEL_PACKAGE_ID");

    // Find AdminCap owned by this address
    const adminCapType = `${sentinelPackageId}::${MODULE.CONFIG}::AdminCap`;
    const caps = await client.getOwnedObjects({
      owner: address,
      filter: { StructType: adminCapType },
      limit: 1,
    });
    const adminCapId = caps.data[0]?.data?.objectId;
    if (!adminCapId) {
      throw new Error(`Sentinel AdminCap not found for ${address}`);
    }
    console.log("Using AdminCap:", adminCapId);

    const tx = new Transaction();

    tx.moveCall({
      target: `${sentinelPackageId}::${MODULE.THREAT_REGISTRY}::create_registry`,
      arguments: [tx.object(adminCapId), tx.object(CLOCK_OBJECT_ID)],
    });

    const result = await client.signAndExecuteTransaction({
      transaction: tx,
      signer: keypair,
      options: { showEffects: true, showObjectChanges: true },
    });

    console.log("Threat registry created!");
    console.log("Transaction digest:", result.digest);

    const changes = result.objectChanges || [];
    const registryObj = changes.find(
      (c: any) =>
        c.type === "created" && c.objectType?.includes("ThreatRegistry"),
    );
    if (registryObj && "objectId" in registryObj) {
      console.log("\nThreatRegistry object ID:", registryObj.objectId);
      console.log("Add this to .env as THREAT_REGISTRY_ID");
    }
  } catch (error) {
    handleError(error);
  }
}

main();
