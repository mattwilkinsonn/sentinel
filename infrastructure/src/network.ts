import * as aws from "@pulumi/aws";
import * as awsx from "@pulumi/awsx";
import { stack } from "./config";

// ---------------------------------------------------------------------------
// VPC (no NAT — tasks use public subnets with public IPs)
// ---------------------------------------------------------------------------
export const vpc = new awsx.ec2.Vpc("sentinel-vpc", {
  numberOfAvailabilityZones: 2,
  natGateways: { strategy: awsx.ec2.NatGatewayStrategy.None },
});

// ---------------------------------------------------------------------------
// Security Groups
// ---------------------------------------------------------------------------

// VPC Link SG — API Gateway routes traffic through these ENIs
export const vpcLinkSg = new aws.ec2.SecurityGroup("sentinel-vpclink-sg", {
  vpcId: vpc.vpcId,
  egress: [
    { protocol: "-1", fromPort: 0, toPort: 0, cidrBlocks: ["0.0.0.0/0"] },
  ],
});

// Task SG — only accepts traffic from VPC Link
export const taskSg = new aws.ec2.SecurityGroup("sentinel-task-sg", {
  vpcId: vpc.vpcId,
  ingress: [
    {
      protocol: "tcp",
      fromPort: 3001,
      toPort: 3001,
      securityGroups: [vpcLinkSg.id],
    },
  ],
  egress: [
    { protocol: "-1", fromPort: 0, toPort: 0, cidrBlocks: ["0.0.0.0/0"] },
  ],
});

// ---------------------------------------------------------------------------
// Cloud Map (service discovery for API Gateway → Fargate)
// ---------------------------------------------------------------------------
const namespace = new aws.servicediscovery.PrivateDnsNamespace("sentinel-ns", {
  name: `sentinel-${stack}.local`,
  vpc: vpc.vpcId,
});

// Note: no healthCheckCustomConfig — Cloud Map treats all registered instances
// as healthy. ECS manages registration/deregistration on task start/stop.
export const cloudMapService = new aws.servicediscovery.Service(
  "sentinel-api-discovery",
  {
    name: "backend",
    dnsConfig: {
      namespaceId: namespace.id,
      dnsRecords: [
        { ttl: 10, type: "A" },
        { ttl: 10, type: "SRV" },
      ],
      routingPolicy: "MULTIVALUE",
    },
  },
);

// ---------------------------------------------------------------------------
// API Gateway HTTP API (replaces ALB — pay-per-request, ~$0 at low traffic)
// ---------------------------------------------------------------------------
const vpcLink = new aws.apigatewayv2.VpcLink("sentinel-vpc-link", {
  name: `sentinel-${stack}`,
  subnetIds: vpc.privateSubnetIds,
  securityGroupIds: [vpcLinkSg.id],
});

export const apiGateway = new aws.apigatewayv2.Api("sentinel-api", {
  name: `sentinel-${stack}`,
  protocolType: "HTTP",
});

const integration = new aws.apigatewayv2.Integration(
  "sentinel-api-integration",
  {
    apiId: apiGateway.id,
    integrationType: "HTTP_PROXY",
    integrationMethod: "ANY",
    connectionType: "VPC_LINK",
    connectionId: vpcLink.id,
    integrationUri: cloudMapService.arn,
  },
);

new aws.apigatewayv2.Route("sentinel-api-route", {
  apiId: apiGateway.id,
  routeKey: "ANY /api/{proxy+}",
  target: integration.id.apply((id) => `integrations/${id}`),
});

const apiAccessLogGroup = new aws.cloudwatch.LogGroup("sentinel-api-access-logs", {
  name: `/aws/apigateway/sentinel-${stack}`,
  retentionInDays: 7,
});

new aws.apigatewayv2.Stage("sentinel-api-stage", {
  apiId: apiGateway.id,
  name: "$default",
  autoDeploy: true,
  accessLogSettings: {
    destinationArn: apiAccessLogGroup.arn,
    format: JSON.stringify({
      requestId: "$context.requestId",
      ip: "$context.identity.sourceIp",
      requestTime: "$context.requestTime",
      httpMethod: "$context.httpMethod",
      routeKey: "$context.routeKey",
      status: "$context.status",
      protocol: "$context.protocol",
      responseLength: "$context.responseLength",
      integrationLatency: "$context.integrationLatency",
      userAgent: "$context.identity.userAgent",
    }),
  },
});
