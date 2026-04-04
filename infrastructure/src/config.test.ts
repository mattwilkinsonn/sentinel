import type * as pulumi from "@pulumi/pulumi";
import { beforeAll, describe, expect, it } from "vitest";
import { setupPulumiMocks } from "./test-setup";

function outputValue<T>(output: pulumi.Output<T>): Promise<T> {
  return new Promise((resolve) => output.apply(resolve));
}

describe("config (dev stack)", () => {
  let config: typeof import("./config");

  beforeAll(async () => {
    await setupPulumiMocks("dev");
    config = await import("./config");
  });

  it("detects non-production stack", () => {
    expect(config.isProduction).toBe(false);
    expect(config.stack).toBe("dev");
  });

  it("derives correct domain for dev", () => {
    expect(config.domain).toBe("sentinel-dev.zireael.dev");
  });

  it("generates correct SSM ARN pattern", async () => {
    const arn = await outputValue(config.ssmArn("sui-publisher-key"));
    expect(arn).toBe(
      "arn:aws:ssm:us-east-1:123456789012:parameter/sentinel/dev/sui-publisher-key",
    );
  });

  it("sets correct log group name", () => {
    expect(config.logGroupName).toBe("/sentinel/dev/backend");
  });

  it("has all required backend config keys", () => {
    const required = [
      "apiPort",
      "publishIntervalMs",
      "publishThresholdBp",
      "maxRecentEvents",
      "sentinelLogLevel",
      "cratesLogLevel",
      "logFormat",
      "suiGrpcUrl",
      "suiGraphqlUrl",
      "worldApiUrl",
    ];
    for (const key of required) {
      expect(config.BACKEND_CONFIG).toHaveProperty(key);
      expect(
        config.BACKEND_CONFIG[key as keyof typeof config.BACKEND_CONFIG],
      ).toBeTruthy();
    }
  });

  it("has all required chain IDs as 0x-prefixed hex strings", () => {
    for (const [key, value] of Object.entries(config.CHAIN_IDS)) {
      expect(value, `${key} should be 0x-prefixed`).toMatch(/^0x[0-9a-f]+$/);
    }
  });

  it("backend API port is 3001", () => {
    expect(config.BACKEND_CONFIG.apiPort).toBe("3001");
  });
});
