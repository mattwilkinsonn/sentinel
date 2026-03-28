import { bcs } from "@mysten/sui/bcs";
import type { SuiJsonRpcClient } from "@mysten/sui/jsonRpc";
import { type getConfig, MODULES } from "../utils/config";
import { devInspectMoveCallFirstReturnValueBytes } from "../utils/dev-inspect";

export async function getOwnerCap(
  gateId: string,
  client: SuiJsonRpcClient,
  config: ReturnType<typeof getConfig>,
  senderAddress?: string,
): Promise<string | null> {
  try {
    const bytes = await devInspectMoveCallFirstReturnValueBytes(client, {
      target: `${config.packageId}::${MODULES.GATE}::owner_cap_id`,
      senderAddress,
      arguments: (tx) => [tx.object(gateId)],
    });

    if (!bytes) {
      console.warn("Error checking gate ownercap id");
      return null;
    }

    return bcs.Address.parse(bytes);
  } catch (error) {
    console.warn(
      "Failed to get gate ownerCap:",
      error instanceof Error ? error.message : error,
    );
    return null;
  }
}
