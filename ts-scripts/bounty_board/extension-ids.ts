import type { SuiJsonRpcClient } from "@mysten/sui/jsonRpc";
import { requireEnv } from "../utils/helper";
import { MODULE } from "./modules";

export type BountyBoardExtensionIds = {
  builderPackageId: string;
  adminCapId: string;
  extensionConfigId: string;
  bountyBoardId: string;
};

export function requireBuilderPackageId(): string {
  return requireEnv("BUILDER_PACKAGE_ID");
}

/** Resolve IDs from env only (no AdminCap lookup). For player-facing scripts. */
export function resolveBountyBoardIdsFromEnv(): {
  builderPackageId: string;
  extensionConfigId: string;
  bountyBoardId: string;
} {
  return {
    builderPackageId: requireBuilderPackageId(),
    extensionConfigId: requireEnv("EXTENSION_CONFIG_ID"),
    bountyBoardId: requireEnv("BOUNTY_BOARD_ID"),
  };
}

/** Resolve all bounty board IDs including AdminCap (for admin scripts). */
export async function resolveBountyBoardExtensionIds(
  client: SuiJsonRpcClient,
  ownerAddress: string,
): Promise<BountyBoardExtensionIds> {
  const { builderPackageId, extensionConfigId, bountyBoardId } =
    resolveBountyBoardIdsFromEnv();
  const adminCapType = `${builderPackageId}::${MODULE.CONFIG}::AdminCap`;
  const result = await client.getOwnedObjects({
    owner: ownerAddress,
    filter: { StructType: adminCapType },
    limit: 1,
  });

  const adminCapId = result.data[0]?.data?.objectId;
  if (!adminCapId) {
    throw new Error(
      `AdminCap not found for ${ownerAddress}. ` +
        `Make sure this address published the bounty_board package.`,
    );
  }

  return { builderPackageId, adminCapId, extensionConfigId, bountyBoardId };
}
