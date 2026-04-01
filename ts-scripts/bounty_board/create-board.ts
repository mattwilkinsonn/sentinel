import { config } from "dotenv";

config({ path: "../.env" });

import { Transaction } from "@mysten/sui/transactions";
import { getEnvConfig, handleError, initializeContext } from "../utils/helper";
import { requireBuilderPackageId } from "./extension-ids";
import { MODULE } from "./modules";

async function main() {
  console.log("============= Create Bounty Board ==============\n");

  try {
    const env = getEnvConfig();
    const ctx = initializeContext(env.network, env.adminExportedKey);
    const { client, keypair, address } = ctx;

    const builderPackageId = requireBuilderPackageId();
    const adminCapType = `${builderPackageId}::${MODULE.CONFIG}::AdminCap`;
    const ownedObjects = await client.getOwnedObjects({
      owner: address,
      filter: { StructType: adminCapType },
      limit: 1,
    });
    const adminCapId = ownedObjects.data[0]?.data?.objectId;
    if (!adminCapId) {
      throw new Error(
        `AdminCap not found for ${address}. Make sure this address published the bounty_board package.`,
      );
    }

    const tx = new Transaction();
    tx.moveCall({
      target: `${builderPackageId}::${MODULE.BOUNTY_BOARD}::create_board`,
      arguments: [tx.object(adminCapId)],
    });

    const result = await client.signAndExecuteTransaction({
      transaction: tx,
      signer: keypair,
      options: { showEffects: true, showObjectChanges: true },
    });

    console.log("Bounty board created!");
    console.log("Transaction digest:", result.digest);

    const changes = result.objectChanges || [];
    const boardObj = changes.find(
      (c: any) => c.type === "created" && c.objectType?.includes("BountyBoard"),
    );
    if (boardObj && "objectId" in boardObj) {
      console.log("\nBountyBoard object ID:", boardObj.objectId);
      console.log("Add to .env:          BOUNTY_BOARD_ID=<id>");
      console.log("Add to frontend/.env: VITE_BOUNTY_BOARD_ID=<id>");
    }
  } catch (error) {
    handleError(error);
  }
}

main();
