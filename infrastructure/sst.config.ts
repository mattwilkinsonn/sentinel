/// <reference path="./.sst/platform/config.d.ts" />

// Public on-chain addresses (not secrets, change when contracts are redeployed)
const CHAIN_IDS = {
  worldPackageId:
    "0x28b497559d65ab320d9da4613bf2498d5946b2c0ae3597ccfda3072ce127448c",
  sentinelPackageId:
    "0x952418ab0e70edeb8ff2802fb90ec4db36e3ff940d459f32027225a12d5087bd",
  threatRegistryId:
    "0x8a3bc7affaaa0d5a9ee67271a77edabc86e475dc95912de4a3b56aa54a4dcc6a",
  sentinelAdminCapId:
    "0x63b04e2700e25b29519767027598d223e5286c4e36e90b17105a2c8b2724a52b",
  builderPackageId:
    "0x9c9cf5193822b91395f206251f58377d1f4486681e1a30fc5078e220911f6b8e",
  bountyBoardId: "",
};

interface EnvConfig {
  imageTag: string;
  neonOrgId: string;
  adminPrivateKey: string;
}

function loadEnv(): EnvConfig {
  function require(name: string): string {
    const val = process.env[name];
    if (!val) throw new Error(`Missing required env var: ${name}`);
    return val;
  }

  return {
    imageTag: require("IMAGE_TAG"),
    neonOrgId: require("NEON_ORG_ID"),
    adminPrivateKey: require("ADMIN_PRIVATE_KEY"),
  };
}

function domainForStage(stage: string): string {
  if (stage === "production") return "sentinel.zireael.dev";
  return `sentinel-${stage}.zireael.dev`;
}

export default $config({
  app(input) {
    return {
      name: "sentinel",
      removal: input?.stage === "production" ? "retain" : "remove",
      home: "aws",
      providers: {
        aws: "7.22.0",
        neon: "0.13.0",
        cloudflare: "6.13.0",
      },
    };
  },
  async run() {
    const env = loadEnv();
    const domain = domainForStage($app.stage);

    // Postgres database (Neon serverless)
    const db = new neon.Project("SentinelDb", {
      name: `sentinel-${$app.stage}`,
      orgId: env.neonOrgId,
      historyRetentionSeconds: 21600,
    });

    const dbBranchId = db.defaultBranchId;

    const dbRole = new neon.Role("SentinelDbRole", {
      projectId: db.id,
      branchId: dbBranchId,
      name: "sentinel",
    });

    const dbName = new neon.Database("SentinelDatabase", {
      projectId: db.id,
      branchId: dbBranchId,
      name: "sentinel",
      ownerName: dbRole.name,
    });

    const databaseUrl = $interpolate`postgresql://${dbRole.name}:${dbRole.password}@${db.databaseHost}/${dbName.name}?sslmode=require`;

    // VPC + ECS Cluster
    const vpc = new sst.aws.Vpc("SentinelVpc");
    const cluster = new sst.aws.Cluster("SentinelCluster", { vpc });

    // Backend service (ECS Fargate)
    const backend = new sst.aws.Service("SentinelBackend", {
      cluster,
      architecture: "arm64",
      image: `ghcr.io/mattwilkinsonn/sentinel/backend:${env.imageTag}`,
      health: {
        command: [
          "CMD-SHELL",
          "curl -f http://localhost:3001/api/health || exit 1",
        ],
        interval: "30 seconds",
      },
      loadBalancer: {
        domain: {
          name: `api.${domain}`,
          dns: sst.cloudflare.dns(),
        },
        health: {
          "3001/http": {
            path: "/api/health",
            interval: "30 seconds",
            timeout: "10 seconds",
          },
        },
        rules: [
          { listen: "443/https", forward: "3001/http" },
          { listen: "80/http", forward: "3001/http" },
        ],
      },
      environment: {
        SENTINEL_API_PORT: "3001",
        SENTINEL_PACKAGE_ID: CHAIN_IDS.sentinelPackageId,
        THREAT_REGISTRY_ID: CHAIN_IDS.threatRegistryId,
        SENTINEL_ADMIN_CAP_ID: CHAIN_IDS.sentinelAdminCapId,
        ADMIN_PRIVATE_KEY: env.adminPrivateKey,
        WORLD_PACKAGE_ID: CHAIN_IDS.worldPackageId,
        BUILDER_PACKAGE_ID: CHAIN_IDS.builderPackageId,
        SUI_GRPC_URL: "https://fullnode.testnet.sui.io:443",
        DATABASE_URL: databaseUrl,
      },
    });

    // Frontend (static site on CloudFront, DNS via Cloudflare)
    // Note: sst.cloudflare.StaticSite has a "Could not resolve sst" bug in SST 4.5.12
    const frontend = new sst.aws.StaticSite("SentinelFrontend", {
      path: "../frontend",
      domain: {
        name: domain,
        dns: sst.cloudflare.dns(),
      },
      build: {
        command: "bun install && bun run build",
        output: "dist",
      },
      environment: {
        VITE_API_URL: `https://api.${domain}`,
        VITE_BOUNTY_BOARD_ID: CHAIN_IDS.bountyBoardId,
        VITE_BUILDER_PACKAGE_ID: CHAIN_IDS.builderPackageId,
        VITE_SUI_RPC_URL: "https://fullnode.testnet.sui.io:443",
      },
    });

    return {
      domain,
      backendUrl: backend.url,
      frontendUrl: frontend.url,
      databaseHost: db.databaseHost,
    };
  },
});
