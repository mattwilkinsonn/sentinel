export type ThreatProfile = {
  character_item_id: number;
  name: string;
  threat_score: number;
  kill_count: number;
  death_count: number;
  bounty_count: number;
  last_kill_timestamp: number;
  last_seen_system: string;
  recent_kills_24h: number;
  systems_visited: number;
};

export type RawEvent = {
  event_type: string;
  timestamp_ms: number;
  data: Record<string, unknown>;
};

export type AggregateStats = {
  total_tracked: number;
  avg_score: number;
  kills_24h: number;
  top_system: string;
  events_per_min: number;
};

export type ScoreBreakdown = {
  kills: number;
  recency: number;
  kd: number;
  bounties: number;
  movement: number;
};

export function computeBreakdown(p: ThreatProfile): ScoreBreakdown {
  const kills = Math.min(2000, Math.floor(Math.log2(p.kill_count + 1) * 600));
  const recency = Math.min(3500, p.recent_kills_24h * 600);
  const kd_ratio =
    p.death_count > 0 ? p.kill_count / p.death_count : p.kill_count;
  const kd = Math.min(1500, Math.floor(kd_ratio * 400));
  const bounties = Math.min(1500, p.bounty_count * 500);
  const movement = Math.min(500, p.systems_visited * 100);
  return { kills, recency, kd, bounties, movement };
}

export type ThreatTier = "LOW" | "MODERATE" | "HIGH" | "CRITICAL";

export function getThreatTier(score: number): ThreatTier {
  if (score <= 2500) return "LOW";
  if (score <= 5000) return "MODERATE";
  if (score <= 7500) return "HIGH";
  return "CRITICAL";
}

export function getThreatColor(tier: ThreatTier): string {
  switch (tier) {
    case "LOW":
      return "var(--color-threat-low)";
    case "MODERATE":
      return "var(--color-threat-moderate)";
    case "HIGH":
      return "var(--color-threat-high)";
    case "CRITICAL":
      return "var(--color-threat-critical)";
  }
}

export function getThreatColorClass(tier: ThreatTier): string {
  switch (tier) {
    case "LOW":
      return "text-threat-low";
    case "MODERATE":
      return "text-threat-moderate";
    case "HIGH":
      return "text-threat-high";
    case "CRITICAL":
      return "text-threat-critical";
  }
}
