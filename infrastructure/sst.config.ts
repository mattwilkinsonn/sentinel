/// <reference path="./.sst/platform/config.d.ts" />

export default $config({
  app(input) {
    return {
      name: "sentinel",
      removal: input?.stage === "production" ? "retain" : "remove",
      home: "aws",
      providers: {
        aws: "6.75.0",
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

    const databaseUrl = $interpolate`postgresql://${dbRole.name}:${dbRole.password}@${dbEndpoint.host}/${dbName.name}?sslmode=require`;

    // VPC + ECS Cluster
    const vpc = new sst.aws.Vpc("SentinelVpc");
    const cluster = new sst.aws.Cluster("SentinelCluster", { vpc });

    // Backend service (ECS Fargate)
    const backend = cluster.addService("SentinelBackend", {
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
        SENTINEL_PACKAGE_ID: process.env.SENTINEL_PACKAGE_ID || "",
        THREAT_REGISTRY_ID: process.env.THREAT_REGISTRY_ID || "",
        SENTINEL_ADMIN_CAP_ID: process.env.SENTINEL_ADMIN_CAP_ID || "",
        ADMIN_PRIVATE_KEY: process.env.ADMIN_PRIVATE_KEY || "",
        WORLD_PACKAGE_ID: process.env.WORLD_PACKAGE_ID || "",
        BUILDER_PACKAGE_ID: process.env.BUILDER_PACKAGE_ID || "",
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
