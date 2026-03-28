/// <reference path="./.sst/platform/config.d.ts" />

export default $config({
  app(input) {
    return {
      name: "sentinel",
      removal: input?.stage === "production" ? "retain" : "remove",
      home: "aws",
      providers: {
        aws: {
          region: "us-east-1",
        },
        neon: "0.13.0",
      },
    };
  },
  async run() {
    // Postgres database (Neon serverless)
    const db = new neon.Project("SentinelDb", {
      name: `sentinel-${$app.stage}`,
    });

    const dbBranch = new neon.Branch("SentinelDbBranch", {
      projectId: db.id,
      name: "main",
    });

    const dbEndpoint = new neon.Endpoint("SentinelDbEndpoint", {
      projectId: db.id,
      branchId: dbBranch.id,
    });

    const dbRole = new neon.Role("SentinelDbRole", {
      projectId: db.id,
      branchId: dbBranch.id,
      name: "sentinel",
    });

    const dbName = new neon.Database("SentinelDatabase", {
      projectId: db.id,
      branchId: dbBranch.id,
      name: "sentinel",
      ownerName: dbRole.name,
    });

    // Backend service (ECS Fargate)
    const backend = new sst.aws.Service("SentinelBackend", {
      path: "../sentinel-backend",
      port: 3001,
      cpu: "0.25 vCPU",
      memory: "0.5 GB",
      environment: {
        SENTINEL_API_PORT: "3001",
        SENTINEL_PACKAGE_ID: process.env.SENTINEL_PACKAGE_ID || "",
        THREAT_REGISTRY_ID: process.env.THREAT_REGISTRY_ID || "",
        SENTINEL_ADMIN_CAP_ID: process.env.SENTINEL_ADMIN_CAP_ID || "",
        ADMIN_PRIVATE_KEY: process.env.ADMIN_PRIVATE_KEY || "",
        WORLD_PACKAGE_ID: process.env.WORLD_PACKAGE_ID || "",
        BUILDER_PACKAGE_ID: process.env.BUILDER_PACKAGE_ID || "",
        SUI_GRPC_URL: "https://fullnode.testnet.sui.io:443",
        SUI_RPC_URL: "https://fullnode.testnet.sui.io:443",
        DATABASE_URL: $interpolate`postgresql://${dbRole.name}:${dbRole.password}@${dbEndpoint.host}/${dbName.name}?sslmode=require`,
      },
      health: {
        path: "/api/health",
        interval: "30 seconds",
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
        VITE_BOUNTY_BOARD_ID: process.env.BOUNTY_BOARD_ID || "",
        VITE_BUILDER_PACKAGE_ID: process.env.BUILDER_PACKAGE_ID || "",
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
