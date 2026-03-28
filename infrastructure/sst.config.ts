/// <reference path="./.sst/platform/config.d.ts" />

interface EnvConfig {
  neonOrgId: string;
  sentinelPackageId: string;
  threatRegistryId: string;
  sentinelAdminCapId: string;
  adminPrivateKey: string;
  worldPackageId: string;
  builderPackageId: string;
  bountyBoardId: string | null;
}

function loadEnv(): EnvConfig {
  function require(name: string): string {
    const val = process.env[name];
    if (!val) throw new Error(`Missing required env var: ${name}`);
    return val;
  }

  return {
    neonOrgId: require("NEON_ORG_ID"),
    sentinelPackageId: require("SENTINEL_PACKAGE_ID"),
    threatRegistryId: require("THREAT_REGISTRY_ID"),
    sentinelAdminCapId: require("SENTINEL_ADMIN_CAP_ID"),
    adminPrivateKey: require("ADMIN_PRIVATE_KEY"),
    worldPackageId: require("WORLD_PACKAGE_ID"),
    builderPackageId: require("BUILDER_PACKAGE_ID"),
    bountyBoardId: process.env.BOUNTY_BOARD_ID ?? null,
  };
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
      },
    };
  },
  async run() {
    const env = loadEnv();

    // Postgres database (Neon serverless)
    const db = new neon.Project("SentinelDb", {
      name: `sentinel-${$app.stage}`,
      orgId: env.neonOrgId,
      historyRetentionSeconds: 21600,
    });

    // Use the default branch created automatically with the project
    const dbBranchId = db.defaultBranchId;

    const dbEndpoint = new neon.Endpoint("SentinelDbEndpoint", {
      projectId: db.id,
      branchId: dbBranchId,
    });

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

    const databaseUrl = $interpolate`postgresql://${dbRole.name}:${dbRole.password}@${dbEndpoint.host}/${dbName.name}?sslmode=require`;

    // VPC + ECS Cluster
    const vpc = new sst.aws.Vpc("SentinelVpc");
    const cluster = new sst.aws.Cluster("SentinelCluster", { vpc });

    // Backend service (ECS Fargate)
    const backend = new sst.aws.Service("SentinelBackend", {
      cluster,
      image: {
        dockerfile: "../sentinel-backend/Dockerfile",
        context: "../sentinel-backend",
      },
      health: {
        command: [
          "CMD-SHELL",
          "curl -f http://localhost:3001/api/health || exit 1",
        ],
        interval: "30 seconds",
      },
      loadBalancer: {
        rules: [{ listen: "80/http", forward: "3001/http" }],
      },
      environment: {
        SENTINEL_API_PORT: "3001",
        SENTINEL_PACKAGE_ID: env.sentinelPackageId,
        THREAT_REGISTRY_ID: env.threatRegistryId,
        SENTINEL_ADMIN_CAP_ID: env.sentinelAdminCapId,
        ADMIN_PRIVATE_KEY: env.adminPrivateKey,
        WORLD_PACKAGE_ID: env.worldPackageId,
        BUILDER_PACKAGE_ID: env.builderPackageId,
        SUI_GRPC_URL: "https://fullnode.testnet.sui.io:443",
        DATABASE_URL: databaseUrl,
      },
    });

    // Frontend (static site on CloudFront)
    const frontend = new sst.aws.StaticSite("SentinelFrontend", {
      path: "../frontend",
      build: {
        command: "bun run build",
        output: "dist",
      },
      environment: {
        VITE_API_URL: backend.url,
        VITE_BOUNTY_BOARD_ID: env.bountyBoardId ?? "",
        VITE_BUILDER_PACKAGE_ID: env.builderPackageId,
        VITE_SUI_RPC_URL: "https://fullnode.testnet.sui.io:443",
      },
    });

    return {
      backendUrl: backend.url,
      frontendUrl: frontend.url,
      databaseHost: dbEndpoint.host,
    };
  },
});
