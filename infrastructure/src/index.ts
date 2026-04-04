import * as pulumi from "@pulumi/pulumi";
import { apiDomain, domain } from "./config";
import { neonProjectId } from "./database";
import "./network";
import "./backend";
import { cdn, siteBucket } from "./frontend";
import "./observability";

// ---------------------------------------------------------------------------
// Exports
// ---------------------------------------------------------------------------
export const domainName = domain;
export const backendUrl = pulumi.interpolate`https://${apiDomain}`;
export const frontendUrl = pulumi.interpolate`https://${domain}`;
export const siteBucketName = siteBucket.bucket;
export const cdnDistributionId = cdn.id;
// Dev stacks read this via StackReference to create Neon branches
export { neonProjectId };
