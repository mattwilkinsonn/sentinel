import * as aws from "@pulumi/aws";
import * as pulumi from "@pulumi/pulumi";
import {
  BACKEND_CONFIG,
  CHAIN_IDS,
  imageTag,
  logGroupName,
  ssmArn,
  stack,
} from "./config";
import { databaseUrl } from "./database";
import { httpsListener, targetGroup, taskSg, vpc } from "./network";

// ---------------------------------------------------------------------------
// ECS Cluster
// ---------------------------------------------------------------------------
export const cluster = new aws.ecs.Cluster("sentinel-cluster", {
  name: `sentinel-${stack}`,
  settings: [{ name: "containerInsights", value: "enabled" }],
});

// ---------------------------------------------------------------------------
// Log Group
// ---------------------------------------------------------------------------
export const logGroup = new aws.cloudwatch.LogGroup("sentinel-backend-logs", {
  name: logGroupName,
  retentionInDays: 30,
});

// ---------------------------------------------------------------------------
// IAM
// ---------------------------------------------------------------------------
const executionRole = new aws.iam.Role("sentinel-execution-role", {
  assumeRolePolicy: aws.iam.assumeRolePolicyForPrincipal({
    Service: "ecs-tasks.amazonaws.com",
  }),
});

new aws.iam.RolePolicyAttachment("sentinel-execution-policy", {
  role: executionRole.name,
  policyArn:
    "arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy",
});

new aws.iam.RolePolicy("sentinel-ssm-policy", {
  role: executionRole.name,
  policy: pulumi
    .all([ssmArn("sui-publisher-key"), ssmArn("discord-token")])
    .apply(([pubKey, discord]) =>
      JSON.stringify({
        Version: "2012-10-17",
        Statement: [
          {
            Effect: "Allow",
            Action: ["ssm:GetParameters", "ssm:GetParameter"],
            Resource: [pubKey, discord],
          },
        ],
      }),
    ),
});

const taskRole = new aws.iam.Role("sentinel-task-role", {
  assumeRolePolicy: aws.iam.assumeRolePolicyForPrincipal({
    Service: "ecs-tasks.amazonaws.com",
  }),
});

// ---------------------------------------------------------------------------
// Task Definition
// ---------------------------------------------------------------------------
const taskDef = new aws.ecs.TaskDefinition("sentinel-backend-task", {
  family: `sentinel-${stack}-backend`,
  requiresCompatibilities: ["FARGATE"],
  networkMode: "awsvpc",
  cpu: "256",
  memory: "512",
  runtimePlatform: {
    cpuArchitecture: "ARM64",
    operatingSystemFamily: "LINUX",
  },
  executionRoleArn: executionRole.arn,
  taskRoleArn: taskRole.arn,
  containerDefinitions: pulumi
    .all([databaseUrl, ssmArn("sui-publisher-key"), ssmArn("discord-token")])
    .apply(([dbUrl, pubKeyArn, discordArn]) =>
      JSON.stringify([
        {
          name: "backend",
          image: `ghcr.io/mattwilkinsonn/sentinel/backend:${imageTag}`,
          essential: true,
          portMappings: [{ containerPort: 3001, protocol: "tcp" }],
          environment: [
            { name: "SENTINEL_API_PORT", value: BACKEND_CONFIG.apiPort },
            {
              name: "SENTINEL_PUBLISH_INTERVAL_MS",
              value: BACKEND_CONFIG.publishIntervalMs,
            },
            {
              name: "SENTINEL_PUBLISH_THRESHOLD_BP",
              value: BACKEND_CONFIG.publishThresholdBp,
            },
            {
              name: "MAX_RECENT_EVENTS",
              value: BACKEND_CONFIG.maxRecentEvents,
            },
            {
              name: "SENTINEL_LOG_LEVEL",
              value: BACKEND_CONFIG.sentinelLogLevel,
            },
            { name: "CRATES_LOG_LEVEL", value: BACKEND_CONFIG.cratesLogLevel },
            { name: "LOG_FORMAT", value: BACKEND_CONFIG.logFormat },
            { name: "SUI_GRPC_URL", value: BACKEND_CONFIG.suiGrpcUrl },
            { name: "SUI_GRAPHQL_URL", value: BACKEND_CONFIG.suiGraphqlUrl },
            { name: "WORLD_API_URL", value: BACKEND_CONFIG.worldApiUrl },
            {
              name: "SENTINEL_PACKAGE_ID",
              value: CHAIN_IDS.sentinelPackageId,
            },
            { name: "THREAT_REGISTRY_ID", value: CHAIN_IDS.threatRegistryId },
            {
              name: "SENTINEL_ADMIN_CAP_ID",
              value: CHAIN_IDS.sentinelAdminCapId,
            },
            { name: "WORLD_PACKAGE_ID", value: CHAIN_IDS.worldPackageId },
            { name: "BUILDER_PACKAGE_ID", value: CHAIN_IDS.builderPackageId },
            { name: "DATABASE_URL", value: dbUrl },
          ],
          secrets: [
            { name: "SUI_PUBLISHER_KEY", valueFrom: pubKeyArn },
            { name: "DISCORD_TOKEN", valueFrom: discordArn },
          ],
          logConfiguration: {
            logDriver: "awslogs",
            options: {
              "awslogs-group": logGroupName,
              "awslogs-region": "us-east-1",
              "awslogs-stream-prefix": "backend",
            },
          },
          healthCheck: {
            command: [
              "CMD-SHELL",
              "curl -f http://localhost:3001/api/health || exit 1",
            ],
            interval: 30,
            timeout: 10,
            retries: 3,
            startPeriod: 60,
          },
        },
      ]),
    ),
});

// ---------------------------------------------------------------------------
// ECS Service
// ---------------------------------------------------------------------------
export const service = new aws.ecs.Service(
  "sentinel-backend",
  {
    name: `sentinel-${stack}-backend`,
    cluster: cluster.arn,
    taskDefinition: taskDef.arn,
    desiredCount: 1,
    launchType: "FARGATE",
    networkConfiguration: {
      subnets: vpc.privateSubnetIds,
      securityGroups: [taskSg.id],
      assignPublicIp: false,
    },
    loadBalancers: [
      {
        targetGroupArn: targetGroup.arn,
        containerName: "backend",
        containerPort: 3001,
      },
    ],
    forceNewDeployment: true,
  },
  { dependsOn: [httpsListener] },
);
