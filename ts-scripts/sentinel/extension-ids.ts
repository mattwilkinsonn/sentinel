import { SuiJsonRpcClient } from "@mysten/sui/jsonRpc";
import { requireEnv } from "../utils/helper";
import { MODULE } from "./modules";

export function requireSentinelPackageId(): string {
    return requireEnv("SENTINEL_PACKAGE_ID");
}

/** Resolve sentinel IDs from env. */
export function resolveSentinelIdsFromEnv(): {
    sentinelPackageId: string;
    extensionConfigId: string;
    threatRegistryId: string;
} {
    return {
        sentinelPackageId: requireSentinelPackageId(),
        extensionConfigId: requireEnv("SENTINEL_EXTENSION_CONFIG_ID"),
        threatRegistryId: requireEnv("THREAT_REGISTRY_ID"),
    };
}

/** Resolve all sentinel IDs including AdminCap. */
export async function resolveSentinelExtensionIds(
    client: SuiJsonRpcClient,
    ownerAddress: string
): Promise<{
    sentinelPackageId: string;
    adminCapId: string;
    extensionConfigId: string;
    threatRegistryId: string;
}> {
    const sentinelPackageId = requireSentinelPackageId();
    const adminCapType = `${sentinelPackageId}::${MODULE.CONFIG}::AdminCap`;

    const result = await client.getOwnedObjects({
        owner: ownerAddress,
        filter: { StructType: adminCapType },
        limit: 1,
    });

    const adminCapId = result.data[0]?.data?.objectId;
    if (!adminCapId) {
        throw new Error(
            `Sentinel AdminCap not found for ${ownerAddress}. ` +
                `Make sure this address published the sentinel package.`
        );
    }

    return {
        sentinelPackageId,
        adminCapId,
        extensionConfigId: requireEnv("SENTINEL_EXTENSION_CONFIG_ID"),
        threatRegistryId: requireEnv("THREAT_REGISTRY_ID"),
    };
}
