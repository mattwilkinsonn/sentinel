export function domainForStack(s: string): string {
  if (s === "production") return "sentinel.zireael.dev";
  return `sentinel-${s}.zireael.dev`;
}
