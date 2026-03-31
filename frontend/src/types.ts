/**
 * Full threat profile for a tracked EVE Frontier pilot, as returned by the
 * backend `/api/data` endpoint. Scores are stored as integers scaled by ×100
 * (e.g. a display score of 75.00 is stored as 7500).
 */
export type ThreatProfile = {
  /** In-game item ID for the character — used as the stable unique key. */
  character_item_id: number;
  /** Resolved pilot name, or empty string when the name lookup hasn't completed yet. */
  name: string;
  /** Composite threat score (×100 integer). Max theoretical value is 10 000 (display: 100.00). */
  threat_score: number;
  kill_count: number;
  death_count: number;
  /** Number of active bounties currently placed on this pilot. */
  bounty_count: number;
  /** Unix timestamp in milliseconds of the most recent kill, or 0 if never killed. */
  last_kill_timestamp: number;
  /** Raw system ID string of the pilot's most recently observed location. */
  last_seen_system: string;
  /** Human-readable name for `last_seen_system`, resolved server-side. May be empty. */
  last_seen_system_name: string;
  /** EVE Frontier corporation/tribe ID. Empty string if the pilot has no tribe. */
  tribe_id: string;
  /** Human-readable tribe name resolved server-side. Empty if `tribe_id` is empty. */
  tribe_name: string;
  /** In-game earned titles (e.g. "Bounty Hunter"). At most a few entries; can be empty. */
  titles: string[];
  /** Pre-computed tier bucket; mirrors the result of `getThreatTier(threat_score)`. */
  threat_tier: ThreatTier;
  /** Kills recorded in the rolling 24-hour window. Drives the recency component of the score. */
  recent_kills_24h: number;
  /** Distinct solar systems the pilot has been observed in. Drives the movement component. */
  systems_visited: number;
};

/**
 * A raw on-chain event as delivered by the backend. The shape of `data` varies
 * per `event_type` — see `eventConfig` in SentinelFeed/FeedView for how each
 * type is rendered.
 */
export type RawEvent = {
  /** Discriminator string, e.g. `"kill"`, `"jump"`, `"bounty_posted"`. */
  event_type: string;
  /** Wall-clock time the event was observed by the indexer, in milliseconds since epoch. */
  timestamp_ms: number;
  /** Type-specific payload. Character IDs inside are numeric strings or numbers. */
  data: Record<string, unknown>;
};

/** Dashboard-level summary stats returned alongside threat profiles in `/api/data`. */
export type AggregateStats = {
  /** Total number of distinct pilots being tracked. */
  total_tracked: number;
  /** Mean threat score (×100 integer) across all tracked pilots. */
  avg_score: number;
  /** Kills recorded across all pilots in the rolling 24-hour window. */
  kills_24h: number;
  /** Display name of the solar system with the most currently tracked pilots. */
  top_system: string;
  /** Total events ingested in the last 24 hours (all types combined). */
  total_events: number;
  /** True when the event buffer is full — the real count may be higher. */
  events_at_cap: boolean;
};

/**
 * The five weighted components that sum to a pilot's composite threat score.
 * All values are ×100 integers — divide by 100 for the human-readable display
 * value. The maximum for each component is defined by `computeBreakdown`.
 */
export type ScoreBreakdown = {
  /** Logarithmic all-time kill contribution. Capped at 2000 (display: 20). */
  kills: number;
  /** Recent activity bonus based on kills in the last 24h. Capped at 3500. */
  recency: number;
  /** Kill/death ratio contribution. Capped at 1500. */
  kd: number;
  /** Contribution from active bounties placed on this pilot. Capped at 1500. */
  bounties: number;
  /** Movement score based on number of distinct systems visited. Capped at 500. */
  movement: number;
};

/**
 * Re-computes the score breakdown client-side from a raw profile so the UI can
 * render a stacked bar without an extra API call. The formula mirrors the
 * backend scoring logic; keep both in sync when tuning weights.
 */
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

/**
 * Named threat bands used for colour-coding and UI labelling.
 * Thresholds (×100 score): LOW ≤ 2500, MODERATE ≤ 5000, HIGH ≤ 7500, CRITICAL > 7500.
 */
export type ThreatTier = "LOW" | "MODERATE" | "HIGH" | "CRITICAL";

/** Maps a raw ×100 threat score to its named tier bucket. */
export function getThreatTier(score: number): ThreatTier {
  if (score <= 2500) return "LOW";
  if (score <= 5000) return "MODERATE";
  if (score <= 7500) return "HIGH";
  return "CRITICAL";
}

/** Returns the CSS custom property value (e.g. `"var(--color-threat-high)"`) for inline styles. */
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

/** Returns the Tailwind text-colour utility class (e.g. `"text-threat-high"`) for class-based styling. */
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
