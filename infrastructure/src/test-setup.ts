import * as pulumi from "@pulumi/pulumi";

export async function setupPulumiMocks(stack = "dev") {
  pulumi.runtime.setAllConfig({
    "sentinel:imageTag": "test-sha-abc123",
    "sentinel:neonOrgId": "mock-neon-org-id",
  });

  pulumi.runtime.setMocks(
    {
      call: async (args) => {
        switch (args.token) {
          case "cloudflare:index/getZone:getZone":
            return { id: "mock-zone-id" };
          case "aws:index/getCallerIdentity:getCallerIdentity":
            return { accountId: "123456789012" };
          case "aws:index/getRegion:getRegion":
            return { name: "us-east-1" };
          default:
            return args.inputs;
        }
      },
      newResource: async (args) => {
        const state = { ...args.inputs };

        // ACM certificates need domainValidationOptions for DNS validation
        if (args.type === "aws:acm/certificate:Certificate") {
          state.domainValidationOptions = [
            {
              domainName: args.inputs.domainName,
              resourceRecordName: `_acme.${args.inputs.domainName}`,
              resourceRecordType: "CNAME",
              resourceRecordValue: "_validate.acm.amazonaws.com",
            },
          ];
        }

        // StackReference outputs for cross-stack references
        if (args.type === "pulumi:pulumi:StackReference") {
          state.outputs = { neonProjectId: "mock-neon-project-id" };
        }

        return { id: `mock-${args.name}`, state };
      },
    },
    "sentinel",
    stack,
  );
}
