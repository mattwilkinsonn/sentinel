import { config } from "dotenv";
config({ path: "../.env" });
import { Transaction } from "@mysten/sui/transactions";
import { MODULES } from "../utils/config";
import { deriveObjectId } from "../utils/derive-object-id";
import {
    getEnvConfig,
    handleError,
    initializeContext,
    requireEnv,
} from "../utils/helper";
import { requireBuilderPackageId } from "./extension-ids";
import { getOwnerCap as getStorageUnitOwnerCap } from "../helpers/storage-unit-extension";
import { MODULE } from "./modules";

/**
 * Authorize the bounty board extension (XAuth) on a StorageUnit.
 * Must be called by the SSU owner before bounty operations work.
 */
async function main() {
    console.log("============= Authorize Bounty Board Extension ==============\n");

    try {
        const env = getEnvConfig();
        const playerKey = requireEnv("PLAYER_A_PRIVATE_KEY");
        const ctx = initializeContext(env.network, playerKey);
        const { client, keypair, config, address } = ctx;

        const builderPackageId = requireBuilderPackageId();
        const storageUnitId = requireEnv("STORAGE_UNIT_ID");
        const characterId = requireEnv("CHARACTER_ID");

        const storageUnitOwnerCapId = await getStorageUnitOwnerCap(
            storageUnitId,
            client,
            config,
            address
        );
        if (!storageUnitOwnerCapId) {
            throw new Error(`OwnerCap not found for storage unit ${storageUnitId}`);
        }

        const authType = `${builderPackageId}::${MODULE.CONFIG}::XAuth`;

        const tx = new Transaction();

        // Borrow SSU OwnerCap from Character
        const [storageUnitOwnerCap, returnReceipt] = tx.moveCall({
            target: `${config.packageId}::${MODULES.CHARACTER}::borrow_owner_cap`,
            typeArguments: [`${config.packageId}::${MODULES.STORAGE_UNIT}::StorageUnit`],
            arguments: [tx.object(characterId), tx.object(storageUnitOwnerCapId)],
        });

        // Authorize extension
        tx.moveCall({
            target: `${config.packageId}::${MODULES.STORAGE_UNIT}::authorize_extension`,
            typeArguments: [authType],
            arguments: [tx.object(storageUnitId), storageUnitOwnerCap],
        });

        // Return OwnerCap
        tx.moveCall({
            target: `${config.packageId}::${MODULES.CHARACTER}::return_owner_cap`,
            typeArguments: [`${config.packageId}::${MODULES.STORAGE_UNIT}::StorageUnit`],
            arguments: [tx.object(characterId), storageUnitOwnerCap, returnReceipt],
        });

        const result = await client.signAndExecuteTransaction({
            transaction: tx,
            signer: keypair,
            options: { showEffects: true, showObjectChanges: true, showEvents: true },
        });

        console.log("Storage unit extension authorized for bounty board!");
        console.log("Storage unit:", storageUnitId);
        console.log("Auth type:", authType);
        console.log("Transaction digest:", result.digest);
    } catch (error) {
        handleError(error);
    }
}

main();
