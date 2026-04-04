import * as aws from "@pulumi/aws";
import * as cloudflare from "@pulumi/cloudflare";
import * as pulumi from "@pulumi/pulumi";
import { domainForStack } from "./helpers";

export { domainForStack };

export const stack = pulumi.getStack(); // "production" or "dev"
const config = new pulumi.Config();
export const isProduction = stack === "production";

export const imageTag = config.require("imageTag");
export const neonOrgId = config.require("neonOrgId");

// Backend runtime config (non-secret, non-chain values)
export const BACKEND_CONFIG = {
  apiPort: "3001",
  publishIntervalMs: "30000",
  publishThresholdBp: "100",
  maxRecentEvents: "5000",
  sentinelLogLevel: "info",
  cratesLogLevel: "warn",
  logFormat: "json",
  suiGrpcUrl: "https://fullnode.testnet.sui.io:443",
  suiGraphqlUrl: "https://graphql.testnet.sui.io/graphql",
  worldApiUrl: "https://world-api-stillness.live.tech.evefrontier.com",
};

// Public on-chain addresses (not secrets)
export const CHAIN_IDS = {
  worldPackageId:
    "0x28b497559d65ab320d9da4613bf2498d5946b2c0ae3597ccfda3072ce127448c",
  sentinelPackageId:
    "0x952418ab0e70edeb8ff2802fb90ec4db36e3ff940d459f32027225a12d5087bd",
  threatRegistryId:
    "0x8a3bc7affaaa0d5a9ee67271a77edabc86e475dc95912de4a3b56aa54a4dcc6a",
  sentinelAdminCapId:
    "0x63b04e2700e25b29519767027598d223e5286c4e36e90b17105a2c8b2724a52b",
  builderPackageId:
    "0x2df819a1e5a5b21044931b2619cdb8e67d7ff0d22a138fabd94b18d73a795358",
  extensionConfigId:
    "0xadcccc30fa3f13b084020ee72977268950d24047b653772335991cbace1f4194",
  bountyBoardId:
    "0x6fafed6fd8a529c404029addb34f5688f1cf8131aad5d92a3ab5de4036566288",
};

export const domain = domainForStack(stack);

// SSM parameter ARNs for secrets (created manually, never in code)
// Setup: aws ssm put-parameter --name /sentinel/<stack>/sui-publisher-key --type SecureString --value <key>
const callerIdentity = aws.getCallerIdentity();

export function ssmArn(name: string): pulumi.Output<string> {
  return pulumi
    .output(callerIdentity)
    .apply(
      (id) =>
        `arn:aws:ssm:us-east-1:${id.accountId}:parameter/sentinel/${stack}/${name}`,
    );
}

export const logGroupName = `/sentinel/${stack}/backend`;

export const zireaelZoneId = cloudflare
  .getZone({ filter: { name: "zireael.dev" } })
  .then((z) => z.id);
