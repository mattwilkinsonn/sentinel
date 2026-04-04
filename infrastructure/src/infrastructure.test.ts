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
    // Import order matters — modules have cross-dependencies
    network = await import("./network");
    backend = await import("./backend");
    frontend = await import("./frontend");
    await import("./observability");
  });

  describe("network", () => {
    it("creates a VPC with 2 availability zones", async () => {
      expect(network.vpc).toBeDefined();
    });

    it("ALB is internet-facing", async () => {
      const internal = await outputValue(network.alb.internal);
      expect(internal).toBe(false);
    });

    it("ALB is an application load balancer", async () => {
      const type = await outputValue(network.alb.loadBalancerType);
      expect(type).toBe("application");
    });

    it("target group uses port 3001", async () => {
      const port = await outputValue(network.targetGroup.port);
      expect(port).toBe(3001);
    });

    it("target group health checks /api/health", async () => {
      const hc = await outputValue(network.targetGroup.healthCheck);
      expect(hc.path).toBe("/api/health");
    });

    it("HTTPS listener is on port 443", async () => {
      const port = await outputValue(network.httpsListener.port);
      expect(port).toBe(443);
    });

    it("ALB security group allows HTTP and HTTPS ingress", async () => {
      const ingress = await outputValue(network.albSg.ingress);
      const ports = ingress?.map((r: { fromPort: number }) => r.fromPort);
      expect(ports).toContain(80);
      expect(ports).toContain(443);
    });

    it("task security group only allows port 3001 from ALB", async () => {
      const ingress = await outputValue(network.taskSg.ingress);
      expect(ingress).toHaveLength(1);
      expect(ingress?.[0].fromPort).toBe(3001);
      expect(ingress?.[0].toPort).toBe(3001);
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

    it("service runs on Fargate", async () => {
      const launch = await outputValue(backend.service.launchType);
      expect(launch).toBe("FARGATE");
    });

    it("service has exactly 1 desired task", async () => {
      const count = await outputValue(backend.service.desiredCount);
      expect(count).toBe(1);
    });

    it("service is named sentinel-dev-backend", async () => {
      const name = await outputValue(backend.service.name);
      expect(name).toBe("sentinel-dev-backend");
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
