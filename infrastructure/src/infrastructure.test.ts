import type * as pulumi from "@pulumi/pulumi";
import { beforeAll, describe, expect, it } from "vitest";
import { setupPulumiMocks } from "./test-setup";

function outputValue<T>(output: pulumi.Output<T>): Promise<T> {
  return new Promise((resolve) => output.apply(resolve));
}

describe("infrastructure (dev stack)", () => {
  let network: typeof import("./network");
  let backend: typeof import("./backend");
  let frontend: typeof import("./frontend");

  beforeAll(async () => {
    await setupPulumiMocks("dev");
    network = await import("./network");
    backend = await import("./backend");
    frontend = await import("./frontend");
    await import("./observability");
  });

  describe("network", () => {
    it("creates a VPC", () => {
      expect(network.vpc).toBeDefined();
    });

    it("task security group allows port 3001 from VPC link SG", async () => {
      const ingress = await outputValue(network.taskSg.ingress);
      expect(ingress).toHaveLength(1);
      expect(ingress?.[0].fromPort).toBe(3001);
      expect(ingress?.[0].toPort).toBe(3001);
    });

    it("creates an API Gateway HTTP API", async () => {
      const protocol = await outputValue(network.apiGateway.protocolType);
      expect(protocol).toBe("HTTP");
    });

    it("creates Cloud Map service discovery", () => {
      expect(network.cloudMapService).toBeDefined();
    });
  });

  describe("backend", () => {
    it("ECS cluster is named sentinel-dev", async () => {
      const name = await outputValue(backend.cluster.name);
      expect(name).toBe("sentinel-dev");
    });

    it("cluster has container insights enabled", async () => {
      const settings = await outputValue(backend.cluster.settings);
      const insights = settings?.find(
        (s: { name: string }) => s.name === "containerInsights",
      );
      expect(insights?.value).toBe("enabled");
    });

    it("log group retains logs for 30 days", async () => {
      const retention = await outputValue(backend.logGroup.retentionInDays);
      expect(retention).toBe(30);
    });

    it("log group name follows convention", async () => {
      const name = await outputValue(backend.logGroup.name);
      expect(name).toBe("/sentinel/dev/backend");
    });

    it("service uses Fargate Spot", async () => {
      const strategies = await outputValue(
        backend.service.capacityProviderStrategies,
      );
      expect(strategies).toHaveLength(1);
      expect(strategies?.[0].capacityProvider).toBe("FARGATE_SPOT");
    });

    it("service has exactly 1 desired task", async () => {
      const count = await outputValue(backend.service.desiredCount);
      expect(count).toBe(1);
    });

    it("service is named sentinel-dev-backend", async () => {
      const name = await outputValue(backend.service.name);
      expect(name).toBe("sentinel-dev-backend");
    });

    it("service uses public subnets with public IP", async () => {
      const netConfig = await outputValue(backend.service.networkConfiguration);
      expect(netConfig?.assignPublicIp).toBe(true);
    });

    it("service registers with Cloud Map", async () => {
      const registries = await outputValue(backend.service.serviceRegistries);
      expect(registries).toBeDefined();
    });
  });

  describe("frontend", () => {
    it("creates an S3 bucket", () => {
      expect(frontend.siteBucket).toBeDefined();
    });

    it("CDN serves index.html as default root", async () => {
      const root = await outputValue(frontend.cdn.defaultRootObject);
      expect(root).toBe("index.html");
    });

    it("CDN is enabled", async () => {
      const enabled = await outputValue(frontend.cdn.enabled);
      expect(enabled).toBe(true);
    });

    it("CDN has two origins (s3 + api)", async () => {
      const origins = await outputValue(frontend.cdn.origins);
      expect(origins).toHaveLength(2);
      const ids = origins?.map((o: { originId: string }) => o.originId);
      expect(ids).toContain("s3");
      expect(ids).toContain("api");
    });

    it("CDN routes /api/* to API Gateway origin", async () => {
      const behaviors = await outputValue(frontend.cdn.orderedCacheBehaviors);
      expect(behaviors).toHaveLength(1);
      expect(behaviors?.[0].pathPattern).toBe("/api/*");
      expect(behaviors?.[0].targetOriginId).toBe("api");
    });

    it("CDN uses redirect-to-https viewer policy", async () => {
      const behavior = await outputValue(frontend.cdn.defaultCacheBehavior);
      expect(behavior.viewerProtocolPolicy).toBe("redirect-to-https");
    });

    it("CDN has SPA fallback for 404 and 403", async () => {
      const errors = await outputValue(frontend.cdn.customErrorResponses);
      const codes = errors?.map((e: { errorCode: number }) => e.errorCode);
      expect(codes).toContain(404);
      expect(codes).toContain(403);
      for (const e of errors ?? []) {
        expect(e.responseCode).toBe(200);
        expect(e.responsePagePath).toBe("/index.html");
      }
    });

    it("CDN uses TLS 1.2 minimum", async () => {
      const cert = await outputValue(frontend.cdn.viewerCertificate);
      expect(cert.minimumProtocolVersion).toBe("TLSv1.2_2021");
    });
  });
});
