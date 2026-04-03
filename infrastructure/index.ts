import * as pulumi from "@pulumi/pulumi";
import * as aws from "@pulumi/aws";
import * as awsx from "@pulumi/awsx";
import * as cloudflare from "@pulumi/cloudflare";
import * as neon from "@pulumiverse/neon";

// ---------------------------------------------------------------------------
// Config
// ---------------------------------------------------------------------------
const stack = pulumi.getStack(); // "production" or "dev"
const config = new pulumi.Config();
const isProduction = stack === "production";

const imageTag = config.require("imageTag");
const neonOrgId = config.require("neonOrgId");

// Backend runtime config (non-secret, non-chain values)
const BACKEND_CONFIG = {
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
    "0x2df819a1e5a5b21044931b2619cdb8e67d7ff0d22a138fabd94b18d73a795358",
  extensionConfigId:
    "0xadcccc30fa3f13b084020ee72977268950d24047b653772335991cbace1f4194",
  bountyBoardId:
    "0x6fafed6fd8a529c404029addb34f5688f1cf8131aad5d92a3ab5de4036566288",
};

function domainForStack(s: string): string {
  if (s === "production") return "sentinel.zireael.dev";
  return `sentinel-${s}.zireael.dev`;
}

const domain = domainForStack(stack);
const apiDomain = `api.${domain}`;

// SSM parameter ARNs for secrets (created manually, never in code)
// Setup: aws ssm put-parameter --name /sentinel/<stack>/sui-publisher-key --type SecureString --value <key>
const callerIdentity = aws.getCallerIdentity();
const region = aws.getRegion();

function ssmArn(name: string): pulumi.Output<string> {
  return pulumi.output(callerIdentity).apply(
    (id) => `arn:aws:ssm:us-east-1:${id.accountId}:parameter/sentinel/${stack}/${name}`,
  );
}

// ---------------------------------------------------------------------------
// Neon Postgres
// ---------------------------------------------------------------------------
// Production creates the Neon project; dev reads it via StackReference.
// Deploy production first, then dev. Dev branches scale to zero when idle.

let databaseUrl: pulumi.Output<string>;
let neonProjectId: pulumi.Output<string>;

if (isProduction) {
  const db = new neon.Project("sentinel-db", {
    name: "sentinel",
    orgId: neonOrgId,
    historyRetentionSeconds: 86400,
  });

  neonProjectId = db.id;

  const dbRole = new neon.Role("sentinel-db-role", {
    projectId: db.id,
    branchId: db.defaultBranchId,
    name: "sentinel",
  });

  const dbName = new neon.Database("sentinel-database", {
    projectId: db.id,
    branchId: db.defaultBranchId,
    name: "sentinel",
    ownerName: dbRole.name,
  });

  databaseUrl = pulumi.interpolate`postgresql://${dbRole.name}:${dbRole.password}@${db.databaseHost}/${dbName.name}?sslmode=require`;
} else {
  // Read the production stack's Neon project ID
  const prodRef = new pulumi.StackReference(`sentinel/production`);
  neonProjectId = prodRef.requireOutput("neonProjectId") as pulumi.Output<string>;

  const branch = new neon.Branch("sentinel-db-branch", {
    projectId: neonProjectId,
    name: stack,
  });

  const endpoint = new neon.Endpoint("sentinel-db-endpoint", {
    projectId: neonProjectId,
    branchId: branch.id,
    type: "read_write",
  });

  const dbRole = new neon.Role("sentinel-db-role", {
    projectId: neonProjectId,
    branchId: branch.id,
    name: "sentinel",
  });

  const dbName = new neon.Database("sentinel-database", {
    projectId: neonProjectId,
    branchId: branch.id,
    name: "sentinel",
    ownerName: dbRole.name,
  });

  databaseUrl = pulumi.interpolate`postgresql://${dbRole.name}:${dbRole.password}@${endpoint.host}/${dbName.name}?sslmode=require`;
}

// ---------------------------------------------------------------------------
// VPC
// ---------------------------------------------------------------------------
const vpc = new awsx.ec2.Vpc("sentinel-vpc", {
  numberOfAvailabilityZones: 2,
  natGateways: { strategy: isProduction ? awsx.ec2.NatGatewayStrategy.OnePerAz : awsx.ec2.NatGatewayStrategy.Single },
});

// ---------------------------------------------------------------------------
// ECS Cluster + Backend Service
// ---------------------------------------------------------------------------
const cluster = new aws.ecs.Cluster("sentinel-cluster", {
  name: `sentinel-${stack}`,
  settings: [{ name: "containerInsights", value: "enabled" }],
});

// Log group (pinned name for metric filters)
const logGroupName = `/sentinel/${stack}/backend`;
const logGroup = new aws.cloudwatch.LogGroup("sentinel-backend-logs", {
  name: logGroupName,
  retentionInDays: 30,
});

// Security group for ALB
const albSg = new aws.ec2.SecurityGroup("sentinel-alb-sg", {
  vpcId: vpc.vpcId,
  ingress: [
    { protocol: "tcp", fromPort: 80, toPort: 80, cidrBlocks: ["0.0.0.0/0"] },
    { protocol: "tcp", fromPort: 443, toPort: 443, cidrBlocks: ["0.0.0.0/0"] },
  ],
  egress: [
    { protocol: "-1", fromPort: 0, toPort: 0, cidrBlocks: ["0.0.0.0/0"] },
  ],
});

// Security group for ECS tasks
const taskSg = new aws.ec2.SecurityGroup("sentinel-task-sg", {
  vpcId: vpc.vpcId,
  ingress: [
    { protocol: "tcp", fromPort: 3001, toPort: 3001, securityGroups: [albSg.id] },
  ],
  egress: [
    { protocol: "-1", fromPort: 0, toPort: 0, cidrBlocks: ["0.0.0.0/0"] },
  ],
});

// ALB
const alb = new aws.lb.LoadBalancer("sentinel-alb", {
  internal: false,
  loadBalancerType: "application",
  securityGroups: [albSg.id],
  subnets: vpc.publicSubnetIds,
});

const targetGroup = new aws.lb.TargetGroup("sentinel-tg", {
  port: 3001,
  protocol: "HTTP",
  targetType: "ip",
  vpcId: vpc.vpcId,
  healthCheck: {
    path: "/api/health",
    interval: 30,
    timeout: 10,
    healthyThreshold: 2,
    unhealthyThreshold: 3,
  },
});

// ACM certificate for API domain
const cert = new aws.acm.Certificate("sentinel-api-cert", {
  domainName: apiDomain,
  validationMethod: "DNS",
});

// Cloudflare DNS validation record for ACM
const certValidation = cert.domainValidationOptions.apply((opts) => opts[0]);
const certValidationRecord = new cloudflare.Record("sentinel-api-cert-validation", {
  zoneId: cloudflare.getZone({ name: "zireael.dev" }).then((z) => z.id),
  name: certValidation.apply((v) => v.resourceRecordName),
  type: certValidation.apply((v) => v.resourceRecordType),
  content: certValidation.apply((v) => v.resourceRecordValue),
  ttl: 60,
});

const certValidated = new aws.acm.CertificateValidation("sentinel-api-cert-validated", {
  certificateArn: cert.arn,
  validationRecordFqdns: [certValidationRecord.hostname],
});

// HTTPS listener
const httpsListener = new aws.lb.Listener("sentinel-https", {
  loadBalancerArn: alb.arn,
  port: 443,
  protocol: "HTTPS",
  certificateArn: certValidated.certificateArn,
  defaultActions: [{ type: "forward", targetGroupArn: targetGroup.arn }],
});

// HTTP → HTTPS redirect
new aws.lb.Listener("sentinel-http-redirect", {
  loadBalancerArn: alb.arn,
  port: 80,
  protocol: "HTTP",
  defaultActions: [{
    type: "redirect",
    redirect: { port: "443", protocol: "HTTPS", statusCode: "HTTP_301" },
  }],
});

// Cloudflare DNS for API
new cloudflare.Record("sentinel-api-dns", {
  zoneId: cloudflare.getZone({ name: "zireael.dev" }).then((z) => z.id),
  name: apiDomain,
  type: "CNAME",
  content: alb.dnsName,
  proxied: true,
});

// IAM role for ECS task execution (pull images, read SSM secrets)
const executionRole = new aws.iam.Role("sentinel-execution-role", {
  assumeRolePolicy: aws.iam.assumeRolePolicyForPrincipal({ Service: "ecs-tasks.amazonaws.com" }),
});

new aws.iam.RolePolicyAttachment("sentinel-execution-policy", {
  role: executionRole.name,
  policyArn: "arn:aws:iam::aws:policy/service-role/AmazonECSTaskExecutionRolePolicy",
});

// Allow reading SSM parameters for secrets
const ssmPolicy = new aws.iam.RolePolicy("sentinel-ssm-policy", {
  role: executionRole.name,
  policy: pulumi.all([ssmArn("sui-publisher-key"), ssmArn("discord-token")]).apply(
    ([pubKey, discord]) =>
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

// Task role (for the running container)
const taskRole = new aws.iam.Role("sentinel-task-role", {
  assumeRolePolicy: aws.iam.assumeRolePolicyForPrincipal({ Service: "ecs-tasks.amazonaws.com" }),
});

// Task definition
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
  containerDefinitions: pulumi.all([databaseUrl, ssmArn("sui-publisher-key"), ssmArn("discord-token")]).apply(
    ([dbUrl, pubKeyArn, discordArn]) =>
      JSON.stringify([
        {
          name: "backend",
          image: `ghcr.io/mattwilkinsonn/sentinel/backend:${imageTag}`,
          essential: true,
          portMappings: [{ containerPort: 3001, protocol: "tcp" }],
          environment: [
            { name: "SENTINEL_API_PORT", value: BACKEND_CONFIG.apiPort },
            { name: "SENTINEL_PUBLISH_INTERVAL_MS", value: BACKEND_CONFIG.publishIntervalMs },
            { name: "SENTINEL_PUBLISH_THRESHOLD_BP", value: BACKEND_CONFIG.publishThresholdBp },
            { name: "MAX_RECENT_EVENTS", value: BACKEND_CONFIG.maxRecentEvents },
            { name: "SENTINEL_LOG_LEVEL", value: BACKEND_CONFIG.sentinelLogLevel },
            { name: "CRATES_LOG_LEVEL", value: BACKEND_CONFIG.cratesLogLevel },
            { name: "LOG_FORMAT", value: BACKEND_CONFIG.logFormat },
            { name: "SUI_GRPC_URL", value: BACKEND_CONFIG.suiGrpcUrl },
            { name: "SUI_GRAPHQL_URL", value: BACKEND_CONFIG.suiGraphqlUrl },
            { name: "WORLD_API_URL", value: BACKEND_CONFIG.worldApiUrl },
            { name: "SENTINEL_PACKAGE_ID", value: CHAIN_IDS.sentinelPackageId },
            { name: "THREAT_REGISTRY_ID", value: CHAIN_IDS.threatRegistryId },
            { name: "SENTINEL_ADMIN_CAP_ID", value: CHAIN_IDS.sentinelAdminCapId },
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
            command: ["CMD-SHELL", "curl -f http://localhost:3001/api/health || exit 1"],
            interval: 30,
            timeout: 10,
            retries: 3,
            startPeriod: 60,
          },
        },
      ]),
  ),
});

// ECS service
const service = new aws.ecs.Service("sentinel-backend", {
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
  loadBalancers: [{
    targetGroupArn: targetGroup.arn,
    containerName: "backend",
    containerPort: 3001,
  }],
  forceNewDeployment: true,
}, { dependsOn: [httpsListener] });

// ---------------------------------------------------------------------------
// Frontend (S3 + CloudFront)
// ---------------------------------------------------------------------------
const siteBucket = new aws.s3.BucketV2("sentinel-frontend-bucket", {});

new aws.s3.BucketPublicAccessBlock("sentinel-frontend-block", {
  bucket: siteBucket.id,
  blockPublicAcls: true,
  blockPublicPolicy: true,
  ignorePublicAcls: true,
  restrictPublicBuckets: true,
});

const oac = new aws.cloudfront.OriginAccessControl("sentinel-oac", {
  name: `sentinel-${stack}-oac`,
  originAccessControlOriginType: "s3",
  signingBehavior: "always",
  signingProtocol: "sigv4",
});

// ACM cert for frontend (must be us-east-1 for CloudFront)
const frontendCert = new aws.acm.Certificate("sentinel-frontend-cert", {
  domainName: domain,
  validationMethod: "DNS",
});

const frontendCertVal = frontendCert.domainValidationOptions.apply((opts) => opts[0]);
const frontendCertValRecord = new cloudflare.Record("sentinel-frontend-cert-validation", {
  zoneId: cloudflare.getZone({ name: "zireael.dev" }).then((z) => z.id),
  name: frontendCertVal.apply((v) => v.resourceRecordName),
  type: frontendCertVal.apply((v) => v.resourceRecordType),
  content: frontendCertVal.apply((v) => v.resourceRecordValue),
  ttl: 60,
});

const frontendCertValidated = new aws.acm.CertificateValidation("sentinel-frontend-cert-validated", {
  certificateArn: frontendCert.arn,
  validationRecordFqdns: [frontendCertValRecord.hostname],
});

const cdn = new aws.cloudfront.Distribution("sentinel-cdn", {
  enabled: true,
  defaultRootObject: "index.html",
  aliases: [domain],
  origins: [{
    domainName: siteBucket.bucketRegionalDomainName,
    originId: "s3",
    originAccessControlId: oac.id,
  }],
  defaultCacheBehavior: {
    targetOriginId: "s3",
    viewerProtocolPolicy: "redirect-to-https",
    allowedMethods: ["GET", "HEAD", "OPTIONS"],
    cachedMethods: ["GET", "HEAD"],
    forwardedValues: { queryString: false, cookies: { forward: "none" } },
    compress: true,
  },
  customErrorResponses: [
    { errorCode: 404, responseCode: 200, responsePagePath: "/index.html" },
    { errorCode: 403, responseCode: 200, responsePagePath: "/index.html" },
  ],
  viewerCertificate: {
    acmCertificateArn: frontendCertValidated.certificateArn,
    sslSupportMethod: "sni-only",
    minimumProtocolVersion: "TLSv1.2_2021",
  },
  restrictions: { geoRestriction: { restrictionType: "none" } },
});

// S3 bucket policy: allow CloudFront OAC
new aws.s3.BucketPolicy("sentinel-frontend-policy", {
  bucket: siteBucket.id,
  policy: pulumi.all([siteBucket.arn, cdn.arn]).apply(([bucketArn, cdnArn]) =>
    JSON.stringify({
      Version: "2012-10-17",
      Statement: [{
        Effect: "Allow",
        Principal: { Service: "cloudfront.amazonaws.com" },
        Action: "s3:GetObject",
        Resource: `${bucketArn}/*`,
        Condition: { StringEquals: { "AWS:SourceArn": cdnArn } },
      }],
    }),
  ),
});

// Cloudflare DNS for frontend
new cloudflare.Record("sentinel-frontend-dns", {
  zoneId: cloudflare.getZone({ name: "zireael.dev" }).then((z) => z.id),
  name: domain,
  type: "CNAME",
  content: cdn.domainName,
  proxied: false, // CloudFront handles TLS, Cloudflare proxy would double-terminate
});

// ---------------------------------------------------------------------------
// CloudWatch Observability
// ---------------------------------------------------------------------------
const alarmTopic = new aws.sns.Topic("sentinel-alarm-topic", {
  name: `sentinel-${stack}-alarms`,
});

// Application error metric filter
const errNs = `Sentinel/${stack}`;
const errorFilter = new aws.cloudwatch.LogMetricFilter("app-error-filter", {
  name: `sentinel-${stack}-app-errors`,
  logGroupName: logGroup.name,
  pattern: '{ $.level = "ERROR" }',
  metricTransformation: {
    namespace: errNs,
    name: "AppErrors",
    value: "1",
    defaultValue: "0",
    unit: "Count",
  },
});

new aws.cloudwatch.MetricAlarm("app-error-alarm", {
  name: `sentinel-${stack}-app-errors`,
  alarmDescription: "Backend logged an ERROR — check CloudWatch Logs",
  namespace: errNs,
  metricName: "AppErrors",
  statistic: "Sum",
  period: 60,
  evaluationPeriods: 1,
  threshold: 1,
  comparisonOperator: "GreaterThanOrEqualToThreshold",
  treatMissingData: "notBreaching",
  alarmActions: [alarmTopic.arn],
  okActions: [alarmTopic.arn],
}, { dependsOn: [errorFilter] });

const albArnSuffix = alb.arnSuffix;

new aws.cloudwatch.MetricAlarm("backend-5xx-alarm", {
  name: `sentinel-${stack}-5xx-errors`,
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
  alarmActions: [alarmTopic.arn],
  okActions: [alarmTopic.arn],
});

new aws.cloudwatch.MetricAlarm("backend-4xx-alarm", {
  name: `sentinel-${stack}-4xx-errors`,
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
  alarmActions: [alarmTopic.arn],
  okActions: [alarmTopic.arn],
});

new aws.cloudwatch.MetricAlarm("alb-5xx-alarm", {
  name: `sentinel-${stack}-alb-5xx`,
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
  alarmActions: [alarmTopic.arn],
  okActions: [alarmTopic.arn],
});

// Dashboard
const dashboardBody = pulumi.all([
  albArnSuffix,
  cluster.name,
  service.name,
  pulumi.output(region).apply((r) => r.name),
]).apply(([alb, ecsCluster, ecsSvc, reg]) => {
  return JSON.stringify({
    widgets: [
      {
        type: "metric", x: 0, y: 0, width: 12, height: 6,
        properties: {
          title: "HTTP 5xx / 4xx Errors", region: reg,
          metrics: [
            ["AWS/ApplicationELB", "HTTPCode_Target_5XX_Count", "LoadBalancer", alb, { stat: "Sum", color: "#d62728", label: "5xx" }],
            ["AWS/ApplicationELB", "HTTPCode_ELB_5XX_Count", "LoadBalancer", alb, { stat: "Sum", color: "#9467bd", label: "ALB 5xx" }],
            ["AWS/ApplicationELB", "HTTPCode_Target_4XX_Count", "LoadBalancer", alb, { stat: "Sum", color: "#ff7f0e", label: "4xx" }],
          ],
          period: 300, view: "timeSeries", stacked: false,
        },
      },
      {
        type: "metric", x: 12, y: 0, width: 12, height: 6,
        properties: {
          title: "Request Count", region: reg,
          metrics: [["AWS/ApplicationELB", "RequestCount", "LoadBalancer", alb, { stat: "Sum", label: "Requests" }]],
          period: 300, view: "timeSeries",
        },
      },
      {
        type: "metric", x: 0, y: 6, width: 12, height: 6,
        properties: {
          title: "Response Time (p99 / avg)", region: reg,
          metrics: [
            ["AWS/ApplicationELB", "TargetResponseTime", "LoadBalancer", alb, { stat: "p99", label: "p99" }],
            ["AWS/ApplicationELB", "TargetResponseTime", "LoadBalancer", alb, { stat: "Average", label: "avg" }],
          ],
          period: 300, view: "timeSeries",
        },
      },
      {
        type: "metric", x: 12, y: 6, width: 12, height: 6,
        properties: {
          title: "Healthy / Unhealthy Targets", region: reg,
          metrics: [
            ["AWS/ApplicationELB", "HealthyHostCount", "LoadBalancer", alb, { stat: "Average", color: "#2ca02c", label: "Healthy" }],
            ["AWS/ApplicationELB", "UnHealthyHostCount", "LoadBalancer", alb, { stat: "Average", color: "#d62728", label: "Unhealthy" }],
          ],
          period: 60, view: "timeSeries",
        },
      },
      {
        type: "metric", x: 0, y: 12, width: 12, height: 6,
        properties: {
          title: "ECS CPU Utilization", region: reg,
          metrics: [["AWS/ECS", "CPUUtilization", "ClusterName", ecsCluster, "ServiceName", ecsSvc, { stat: "Average", label: "CPU %" }]],
          period: 300, view: "timeSeries",
        },
      },
      {
        type: "metric", x: 12, y: 12, width: 12, height: 6,
        properties: {
          title: "ECS Memory Utilization", region: reg,
          metrics: [["AWS/ECS", "MemoryUtilization", "ClusterName", ecsCluster, "ServiceName", ecsSvc, { stat: "Average", label: "Memory %" }]],
          period: 300, view: "timeSeries",
        },
      },
      {
        type: "metric", x: 0, y: 18, width: 24, height: 6,
        properties: {
          title: "Application Errors (ERROR log level)", region: reg,
          metrics: [[errNs, "AppErrors", { stat: "Sum", color: "#d62728", label: "Errors" }]],
          period: 60, view: "timeSeries",
        },
      },
    ],
  });
});

new aws.cloudwatch.Dashboard("sentinel-dashboard", {
  dashboardName: `sentinel-${stack}`,
  dashboardBody,
});

// ---------------------------------------------------------------------------
// Exports
// ---------------------------------------------------------------------------
export const domainName = domain;
export const backendUrl = pulumi.interpolate`https://${apiDomain}`;
export const frontendUrl = pulumi.interpolate`https://${domain}`;
export const neonProjectIdOutput = neonProjectId;
export const siteBucketName = siteBucket.bucket;
export const cdnDistributionId = cdn.id;
