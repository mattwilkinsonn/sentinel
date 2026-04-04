import { describe, expect, it } from "vitest";
import { domainForStack } from "./helpers";

describe("domainForStack", () => {
  it("returns production domain", () => {
    expect(domainForStack("production")).toBe("sentinel.zireael.dev");
  });

  it("returns dev domain with stack suffix", () => {
    expect(domainForStack("dev")).toBe("sentinel-dev.zireael.dev");
  });

  it("returns preview domain with stack suffix", () => {
    expect(domainForStack("preview")).toBe("sentinel-preview.zireael.dev");
  });

  it("always produces a valid subdomain of zireael.dev", () => {
    for (const stack of ["dev", "staging", "production", "pr-42"]) {
      const result = domainForStack(stack);
      expect(result).toMatch(/\.zireael\.dev$/);
      expect(result).not.toContain(" ");
    }
  });

  it("prefixes non-production domains with sentinel-<stack>", () => {
    expect(domainForStack("staging")).toBe("sentinel-staging.zireael.dev");
    expect(domainForStack("pr-42")).toBe("sentinel-pr-42.zireael.dev");
  });
});
