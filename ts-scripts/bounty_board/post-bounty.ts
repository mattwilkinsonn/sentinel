import { config } from "dotenv";
config({ path: "../.env" });
import { Transaction } from "@mysten/sui/transactions";
import { MODULES } from "../utils/config";
import { CLOCK_OBJECT_ID, TENANT } from "../utils/constants";
import {
    getEnvConfig,
    handleError,
    initializeContext,
    requireEnv,
} from "../utils/helper";
import { resolveBountyBoardIdsFromEnv } from "./extension-ids";
import { MODULE } from "./modules";

/**
 * Post a bounty on a target character.
 *
 * Flow:
 * 1. Borrow Character OwnerCap
 * 2. Call post_bounty (withdraws reward from poster's inventory, escrows in open inventory)
 * 3. Return OwnerCap
 *
 * Required env vars:
 *   PLAYER_A_PRIVATE_KEY - poster's private key
 *   CHARACTER_ID - poster's Character object ID
 *   STORAGE_UNIT_ID - the SSU bound to the bounty board
 *   TARGET_CHARACTER_ITEM_ID - in-game character ID of the target
 *   REWARD_TYPE_ID - type_id of the reward item
 *   REWARD_QUANTITY - quantity of reward
 *   BOUNTY_DURATION_MS - (optional) duration in ms, 0 = use default
 */
async function main() {
    console.log("============= Post Bounty ==============\n");

    try {
        const env = getEnvConfig();
        const playerKey = requireEnv("PLAYER_A_PRIVATE_KEY");
        const ctx = initializeContext(env.network, playerKey);
        const { client, keypair, config, address } = ctx;

        const { builderPackageId, extensionConfigId, bountyBoardId } =
            resolveBountyBoardIdsFromEnv();

        const characterId = requireEnv("CHARACTER_ID");
        const storageUnitId = requireEnv("STORAGE_UNIT_ID");
        const targetCharacterItemId = BigInt(requireEnv("TARGET_CHARACTER_ITEM_ID"));
        const targetTenant = process.env.TARGET_TENANT || TENANT;
        const rewardTypeId = BigInt(requireEnv("REWARD_TYPE_ID"));
        const rewardQuantity = Number(requireEnv("REWARD_QUANTITY"));
        const durationMs = BigInt(process.env.BOUNTY_DURATION_MS || "0");

        // Find the Character's OwnerCap for the Character object
        const characterObj = await client.getObject({
            id: characterId,
            options: { showContent: true },
        });
        const characterOwnerCapId = (characterObj.data?.content as any)?.fields?.owner_cap_id;
        if (!characterOwnerCapId) {
            throw new Error(`Could not find owner_cap_id on Character ${characterId}`);
        }

        const tx = new Transaction();

        // Borrow Character OwnerCap
        const [ownerCap, returnReceipt] = tx.moveCall({
            target: `${config.packageId}::${MODULES.CHARACTER}::borrow_owner_cap`,
            typeArguments: [`${config.packageId}::${MODULES.CHARACTER}::Character`],
            arguments: [tx.object(characterId), tx.object(characterOwnerCapId)],
        });

        // Post bounty
        tx.moveCall({
            target: `${builderPackageId}::${MODULE.BOUNTY_BOARD}::post_bounty`,
            typeArguments: [`${config.packageId}::${MODULES.CHARACTER}::Character`],
            arguments: [
                tx.object(bountyBoardId),
                tx.object(extensionConfigId),
                tx.object(storageUnitId),
                tx.object(characterId),
                ownerCap,
                tx.pure.u64(targetCharacterItemId),
                tx.pure.string(targetTenant),
                tx.pure.u64(rewardTypeId),
                tx.pure.u32(rewardQuantity),
                tx.pure.u64(durationMs),
                tx.object(CLOCK_OBJECT_ID),
            ],
        });

        // Return OwnerCap
        tx.moveCall({
            target: `${config.packageId}::${MODULES.CHARACTER}::return_owner_cap`,
            typeArguments: [`${config.packageId}::${MODULES.CHARACTER}::Character`],
            arguments: [tx.object(characterId), ownerCap, returnReceipt],
        });

        const result = await client.signAndExecuteTransaction({
            transaction: tx,
            signer: keypair,
            options: { showEffects: true, showObjectChanges: true, showEvents: true },
        });

        console.log("Bounty posted!");
        console.log("Transaction digest:", result.digest);

        // Extract bounty ID from events
        const events = result.events || [];
        const postEvent = events.find((e: any) => e.type.includes("BountyPostedEvent"));
        if (postEvent?.parsedJson) {
            const parsed = postEvent.parsedJson as any;
            console.log("\nBounty ID:", parsed.bounty_id);
            console.log("Target:", parsed.target_item_id);
            console.log("Reward:", parsed.reward_quantity, "x type", parsed.reward_type_id);
            console.log("Expires at:", new Date(Number(parsed.expires_at)).toISOString());
        }
    } catch (error) {
        handleError(error);
    }
}

main();
