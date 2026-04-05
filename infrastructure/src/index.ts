import * as pulumi from "@pulumi/pulumi";
import { apiDomain, domain } from "./config";
import { neonProjectId } from "./database";
import "./network";
import "./backend";
import "./frontend";
import "./observability";

// ---------------------------------------------------------------------------
// Exports
// ---------------------------------------------------------------------------
export const domainName = domain;
export const backendUrl = pulumi.interpolate`https://${apiDomain}/api`;
export const frontendUrl = pulumi.interpolate`https://${domain}`;
// Dev stacks read this via StackReference to create Neon branches
export { neonProjectId };
