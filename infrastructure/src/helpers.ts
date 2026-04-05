export function domainForStack(s: string): string {
  if (s === "production") return "sentinel.zireael.dev";
  return `sentinel-${s}.zireael.dev`;
}

export function apiDomainForStack(s: string): string {
  if (s === "production") return "api.sentinel.zireael.dev";
  return `api.sentinel-${s}.zireael.dev`;
}
