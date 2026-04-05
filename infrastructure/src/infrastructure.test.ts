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

  describe("API domain", () => {
    it("ACM cert targets api.sentinel-dev.zireael.dev", async () => {
      const domain = await outputValue(frontend.apiCert.domainName);
      expect(domain).toBe("api.sentinel-dev.zireael.dev");
    });

    it("ACM cert uses DNS validation", async () => {
      const method = await outputValue(frontend.apiCert.validationMethod);
      expect(method).toBe("DNS");
    });

    it("API Gateway custom domain matches api subdomain", async () => {
      const domain = await outputValue(frontend.apiDomainName.domainName);
      expect(domain).toBe("api.sentinel-dev.zireael.dev");
    });

    it("API Gateway custom domain uses TLS 1.2", async () => {
      const config = await outputValue(
        frontend.apiDomainName.domainNameConfiguration,
      );
      expect(config.securityPolicy).toBe("TLS_1_2");
    });

    it("API Gateway custom domain is REGIONAL", async () => {
      const config = await outputValue(
        frontend.apiDomainName.domainNameConfiguration,
      );
      expect(config.endpointType).toBe("REGIONAL");
    });
  });
});
