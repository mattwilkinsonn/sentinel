/// <reference path="./.sst/platform/config.d.ts" />

// Backend runtime config — non-secret, non-chain values
const BACKEND_CONFIG = {
  apiPort: "3001",
  publishIntervalMs: "30000",
  publishThresholdBp: "100",
  sentinelLogLevel: "info",
  cratesLogLevel: "warn",
  logFormat: "json",
  suiGrpcUrl: "https://fullnode.testnet.sui.io:443",
  suiGraphqlUrl: "https://sui-testnet.mystenlabs.com/graphql",
  worldApiUrl: "https://world-api-stillness.live.tech.evefrontier.com",
};

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
  };
}

// SSM Parameter Store ARNs for secrets — stored manually, never in code or CI.
// One-time setup:
//   aws ssm put-parameter --name /sentinel/<stage>/sui-publisher-key --type SecureString --value <key>
//   aws ssm put-parameter --name /sentinel/<stage>/discord-token --type SecureString --value <token>
function ssmArn(stage: string, name: string): string {
  return `arn:aws:ssm:us-east-1:${aws.getCallerIdentityOutput().accountId}:parameter/sentinel/${stage}/${name}`;
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
        SENTINEL_API_PORT: BACKEND_CONFIG.apiPort,
        SENTINEL_PUBLISH_INTERVAL_MS: BACKEND_CONFIG.publishIntervalMs,
        SENTINEL_PUBLISH_THRESHOLD_BP: BACKEND_CONFIG.publishThresholdBp,
        SENTINEL_LOG_LEVEL: BACKEND_CONFIG.sentinelLogLevel,
        CRATES_LOG_LEVEL: BACKEND_CONFIG.cratesLogLevel,
        LOG_FORMAT: BACKEND_CONFIG.logFormat,
        SUI_GRPC_URL: BACKEND_CONFIG.suiGrpcUrl,
        SUI_GRAPHQL_URL: BACKEND_CONFIG.suiGraphqlUrl,
        WORLD_API_URL: BACKEND_CONFIG.worldApiUrl,
        SENTINEL_PACKAGE_ID: CHAIN_IDS.sentinelPackageId,
        THREAT_REGISTRY_ID: CHAIN_IDS.threatRegistryId,
        SENTINEL_ADMIN_CAP_ID: CHAIN_IDS.sentinelAdminCapId,
        WORLD_PACKAGE_ID: CHAIN_IDS.worldPackageId,
        BUILDER_PACKAGE_ID: CHAIN_IDS.builderPackageId,
        DATABASE_URL: databaseUrl,
      },
      ssm: {
        SUI_PUBLISHER_KEY: ssmArn($app.stage, "sui-publisher-key"),
        DISCORD_TOKEN: ssmArn($app.stage, "discord-token"),
      },
    });

    // CloudWatch: operational alarms + dashboard
    // (The auto-scaling alarms are noise — these are the real ones.)
    const region = aws.getRegionOutput().name;
    const albArnSuffix = backend.nodes.loadBalancer.arnSuffix;
    const ecsClusterName = cluster.nodes.cluster.name;
    const ecsServiceName = backend.nodes.service.name;

    const snsAlarmTopic = new aws.sns.Topic("SentinelAlarmTopic", {
      name: `sentinel-${$app.stage}-alarms`,
    });

    // 5xx errors (server-side failures)
    new aws.cloudwatch.MetricAlarm("Backend5xxAlarm", {
      name: `sentinel-${$app.stage}-5xx-errors`,
      alarmDescription: "Backend returning 5xx errors",
      namespace: "AWS/ApplicationELB",
      metricName: "HTTPCode_Target_5XX_Count",
      statistic: "Sum",
      period: 300,
      evaluationPeriods: 2,
      threshold: 10,
      comparisonOperator: "GreaterThanOrEqualToThreshold",
      treatMissingData: "notBreaching",
      dimensions: { LoadBalancer: albArnSuffix },
      alarmActions: [snsAlarmTopic.arn],
      okActions: [snsAlarmTopic.arn],
    });

    // 4xx errors (elevated client errors may indicate a problem)
    new aws.cloudwatch.MetricAlarm("Backend4xxAlarm", {
      name: `sentinel-${$app.stage}-4xx-errors`,
      alarmDescription: "Elevated 4xx client errors",
      namespace: "AWS/ApplicationELB",
      metricName: "HTTPCode_Target_4XX_Count",
      statistic: "Sum",
      period: 300,
      evaluationPeriods: 3,
      threshold: 50,
      comparisonOperator: "GreaterThanOrEqualToThreshold",
      treatMissingData: "notBreaching",
      dimensions: { LoadBalancer: albArnSuffix },
      alarmActions: [snsAlarmTopic.arn],
      okActions: [snsAlarmTopic.arn],
    });

    // ALB 5xx (ALB itself returning errors — targets unreachable or crashing)
    new aws.cloudwatch.MetricAlarm("AlbErrorAlarm", {
      name: `sentinel-${$app.stage}-alb-5xx`,
      alarmDescription: "ALB returning 5xx — targets may be down",
      namespace: "AWS/ApplicationELB",
      metricName: "HTTPCode_ELB_5XX_Count",
      statistic: "Sum",
      period: 60,
      evaluationPeriods: 3,
      threshold: 5,
      comparisonOperator: "GreaterThanOrEqualToThreshold",
      treatMissingData: "notBreaching",
      dimensions: { LoadBalancer: albArnSuffix },
      alarmActions: [snsAlarmTopic.arn],
      okActions: [snsAlarmTopic.arn],
    });

    // CloudWatch Dashboard
    const dashboardBody = $resolve([
      albArnSuffix,
      ecsClusterName,
      ecsServiceName,
      region,
    ]).apply(([alb, ecsCluster, ecsSvc, reg]) =>
      JSON.stringify({
        widgets: [
          {
            type: "metric",
            x: 0,
            y: 0,
            width: 12,
            height: 6,
            properties: {
              title: "HTTP 5xx / 4xx Errors",
              region: reg,
              metrics: [
                [
                  "AWS/ApplicationELB",
                  "HTTPCode_Target_5XX_Count",
                  "LoadBalancer",
                  alb,
                  { stat: "Sum", color: "#d62728", label: "5xx" },
                ],
                [
                  "AWS/ApplicationELB",
                  "HTTPCode_ELB_5XX_Count",
                  "LoadBalancer",
                  alb,
                  { stat: "Sum", color: "#9467bd", label: "ALB 5xx" },
                ],
                [
                  "AWS/ApplicationELB",
                  "HTTPCode_Target_4XX_Count",
                  "LoadBalancer",
                  alb,
                  { stat: "Sum", color: "#ff7f0e", label: "4xx" },
                ],
              ],
              period: 300,
              view: "timeSeries",
              stacked: false,
            },
          },
          {
            type: "metric",
            x: 12,
            y: 0,
            width: 12,
            height: 6,
            properties: {
              title: "Request Count",
              region: reg,
              metrics: [
                [
                  "AWS/ApplicationELB",
                  "RequestCount",
                  "LoadBalancer",
                  alb,
                  { stat: "Sum", label: "Requests" },
                ],
              ],
              period: 300,
              view: "timeSeries",
            },
          },
          {
            type: "metric",
            x: 0,
            y: 6,
            width: 12,
            height: 6,
            properties: {
              title: "Response Time (p99 / avg)",
              region: reg,
              metrics: [
                [
                  "AWS/ApplicationELB",
                  "TargetResponseTime",
                  "LoadBalancer",
                  alb,
                  { stat: "p99", label: "p99" },
                ],
                [
                  "AWS/ApplicationELB",
                  "TargetResponseTime",
                  "LoadBalancer",
                  alb,
                  { stat: "Average", label: "avg" },
                ],
              ],
              period: 300,
              view: "timeSeries",
            },
          },
          {
            type: "metric",
            x: 12,
            y: 6,
            width: 12,
            height: 6,
            properties: {
              title: "Healthy / Unhealthy Targets",
              region: reg,
              metrics: [
                [
                  "AWS/ApplicationELB",
                  "HealthyHostCount",
                  "LoadBalancer",
                  alb,
                  { stat: "Average", color: "#2ca02c", label: "Healthy" },
                ],
                [
                  "AWS/ApplicationELB",
                  "UnHealthyHostCount",
                  "LoadBalancer",
                  alb,
                  { stat: "Average", color: "#d62728", label: "Unhealthy" },
                ],
              ],
              period: 60,
              view: "timeSeries",
            },
          },
          {
            type: "metric",
            x: 0,
            y: 12,
            width: 12,
            height: 6,
            properties: {
              title: "ECS CPU Utilization",
              region: reg,
              metrics: [
                [
                  "AWS/ECS",
                  "CPUUtilization",
                  "ClusterName",
                  ecsCluster,
                  "ServiceName",
                  ecsSvc,
                  { stat: "Average", label: "CPU %" },
                ],
              ],
              period: 300,
              view: "timeSeries",
            },
          },
          {
            type: "metric",
            x: 12,
            y: 12,
            width: 12,
            height: 6,
            properties: {
              title: "ECS Memory Utilization",
              region: reg,
              metrics: [
                [
                  "AWS/ECS",
                  "MemoryUtilization",
                  "ClusterName",
                  ecsCluster,
                  "ServiceName",
                  ecsSvc,
                  { stat: "Average", label: "Memory %" },
                ],
              ],
              period: 300,
              view: "timeSeries",
            },
          },
        ],
      }),
    );

    new aws.cloudwatch.Dashboard("SentinelDashboard", {
      dashboardName: `sentinel-${$app.stage}`,
      dashboardBody,
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
