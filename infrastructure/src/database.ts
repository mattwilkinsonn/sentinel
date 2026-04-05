import * as neon from "@pulumi/neon";
import * as pulumi from "@pulumi/pulumi";
import { isProduction, neonOrgId, stack } from "./config";

// ---------------------------------------------------------------------------
// Neon Postgres
// ---------------------------------------------------------------------------
// Single Neon project "Sentinel". Production creates the project and uses the
// default branch. Dev/preview stages branch off it via StackReference for
// scale-to-zero. Deploy production first.
//
// Provider installed via: pulumi package add terraform-provider kislerdm/neon

let databaseUrl: pulumi.Output<string>;
let neonProjectId: pulumi.Output<string> | undefined;

if (isProduction) {
  const db = new neon.Project("sentinel-db", {
    name: "sentinel",
    orgId: neonOrgId,
    regionId: "aws-us-east-1",
    historyRetentionSeconds: 86400,
    defaultEndpointSettings: {
      autoscalingLimitMinCu: 0.25,
      autoscalingLimitMaxCu: 0.25,
      suspendTimeoutSeconds: 60,
    },
  });

  neonProjectId = db.id;

  const dbRole = new neon.Role("sentinel-db-role", {
    projectId: db.id,
    branchId: db.defaultBranchId,
    name: "sentinel",
  });

  const dbName = new neon.Database("sentinel-database", {
    projectId: db.id,
    branchId: db.defaultBranchId,
    name: "sentinel",
    ownerName: dbRole.name,
  });

  databaseUrl = pulumi.interpolate`postgresql://${dbRole.name}:${dbRole.password}@${db.databaseHost}/${dbName.name}?sslmode=require`;
} else {
  const prodRef = new pulumi.StackReference("Zireael/sentinel/production");
  const prodProjectId = prodRef.requireOutput(
    "neonProjectId",
  ) as pulumi.Output<string>;

  const branch = new neon.Branch("sentinel-db-branch", {
    projectId: prodProjectId,
    name: stack,
  });

  const endpoint = new neon.Endpoint("sentinel-db-endpoint", {
    projectId: prodProjectId,
    branchId: branch.id,
    type: "read_write",
  });

  const dbRole = new neon.Role("sentinel-db-role", {
    projectId: prodProjectId,
    branchId: branch.id,
    name: "sentinel",
  });

  const dbName = new neon.Database("sentinel-database", {
    projectId: prodProjectId,
    branchId: branch.id,
    name: "sentinel",
    ownerName: dbRole.name,
  });

  databaseUrl = pulumi.interpolate`postgresql://${dbRole.name}:${dbRole.password}@${endpoint.host}/${dbName.name}?sslmode=require`;
}

export { databaseUrl, neonProjectId };
