import * as pulumi from "@pulumi/pulumi";

// Mock Neon provider resources for testing.
// The real @pulumi/neon package is generated via `pulumi package add terraform-provider kislerdm/neon`.

class MockResource extends pulumi.CustomResource {}

export class Project extends MockResource {
  public declare readonly id: pulumi.Output<string>;
  public readonly defaultBranchId!: pulumi.Output<string>;
  public readonly databaseHost!: pulumi.Output<string>;

  constructor(
    name: string,
    props: Record<string, unknown>,
    opts?: pulumi.CustomResourceOptions,
  ) {
    super("neon:index:Project", name, props, opts);
  }
}

export class Role extends MockResource {
  public readonly name!: pulumi.Output<string>;
  public readonly password!: pulumi.Output<string>;

  constructor(
    name: string,
    props: Record<string, unknown>,
    opts?: pulumi.CustomResourceOptions,
  ) {
    super("neon:index:Role", name, props, opts);
  }
}

export class Database extends MockResource {
  public readonly name!: pulumi.Output<string>;

  constructor(
    name: string,
    props: Record<string, unknown>,
    opts?: pulumi.CustomResourceOptions,
  ) {
    super("neon:index:Database", name, props, opts);
  }
}

export class Branch extends MockResource {
  public declare readonly id: pulumi.Output<string>;

  constructor(
    name: string,
    props: Record<string, unknown>,
    opts?: pulumi.CustomResourceOptions,
  ) {
    super("neon:index:Branch", name, props, opts);
  }
}

export class Endpoint extends MockResource {
  public readonly host!: pulumi.Output<string>;

  constructor(
    name: string,
    props: Record<string, unknown>,
    opts?: pulumi.CustomResourceOptions,
  ) {
    super("neon:index:Endpoint", name, props, opts);
  }
}
