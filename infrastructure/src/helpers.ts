// TODO: Remove "pulumi." prefix after migration from SST is complete
export function domainForStack(s: string): string {
  if (s === "production") return "pulumi.sentinel.zireael.dev";
  return `pulumi.sentinel-${s}.zireael.dev`;
}
