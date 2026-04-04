import * as aws from "@pulumi/aws";
import * as awsx from "@pulumi/awsx";
import * as cloudflare from "@pulumi/cloudflare";
import { apiDomain, isProduction, zireaelZoneId } from "./config";

// ---------------------------------------------------------------------------
// VPC
// ---------------------------------------------------------------------------
export const vpc = new awsx.ec2.Vpc("sentinel-vpc", {
  numberOfAvailabilityZones: 2,
  natGateways: {
    strategy: isProduction
      ? awsx.ec2.NatGatewayStrategy.OnePerAz
      : awsx.ec2.NatGatewayStrategy.Single,
  },
});

// ---------------------------------------------------------------------------
// Security Groups
// ---------------------------------------------------------------------------
export const albSg = new aws.ec2.SecurityGroup("sentinel-alb-sg", {
  vpcId: vpc.vpcId,
  ingress: [
    { protocol: "tcp", fromPort: 80, toPort: 80, cidrBlocks: ["0.0.0.0/0"] },
    {
      protocol: "tcp",
      fromPort: 443,
      toPort: 443,
      cidrBlocks: ["0.0.0.0/0"],
    },
  ],
  egress: [
    { protocol: "-1", fromPort: 0, toPort: 0, cidrBlocks: ["0.0.0.0/0"] },
  ],
});

export const taskSg = new aws.ec2.SecurityGroup("sentinel-task-sg", {
  vpcId: vpc.vpcId,
  ingress: [
    {
      protocol: "tcp",
      fromPort: 3001,
      toPort: 3001,
      securityGroups: [albSg.id],
    },
  ],
  egress: [
    { protocol: "-1", fromPort: 0, toPort: 0, cidrBlocks: ["0.0.0.0/0"] },
  ],
});

// ---------------------------------------------------------------------------
// ALB
// ---------------------------------------------------------------------------
export const alb = new aws.lb.LoadBalancer("sentinel-alb", {
  internal: false,
  loadBalancerType: "application",
  securityGroups: [albSg.id],
  subnets: vpc.publicSubnetIds,
});

export const targetGroup = new aws.lb.TargetGroup("sentinel-tg", {
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

// ---------------------------------------------------------------------------
// ACM + HTTPS
// ---------------------------------------------------------------------------
const cert = new aws.acm.Certificate("sentinel-api-cert", {
  domainName: apiDomain,
  validationMethod: "DNS",
});

const certValidation = cert.domainValidationOptions.apply((opts) => opts[0]);
const certValidationRecord = new cloudflare.Record(
  "sentinel-api-cert-validation",
  {
    zoneId: zireaelZoneId,
    name: certValidation.apply((v) => v.resourceRecordName),
    type: certValidation.apply((v) => v.resourceRecordType),
    content: certValidation.apply((v) => v.resourceRecordValue),
    ttl: 60,
  },
);

const certValidated = new aws.acm.CertificateValidation(
  "sentinel-api-cert-validated",
  {
    certificateArn: cert.arn,
    validationRecordFqdns: [certValidationRecord.name],
  },
);

export const httpsListener = new aws.lb.Listener("sentinel-https", {
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
  defaultActions: [
    {
      type: "redirect",
      redirect: { port: "443", protocol: "HTTPS", statusCode: "HTTP_301" },
    },
  ],
});

// ---------------------------------------------------------------------------
// Cloudflare DNS for API
// ---------------------------------------------------------------------------
new cloudflare.Record("sentinel-api-dns", {
  zoneId: zireaelZoneId,
  name: apiDomain,
  type: "CNAME",
  content: alb.dnsName,
  ttl: 1, // automatic (proxied)
  proxied: true,
});
