import { Skull, Users } from "lucide-react";
import type { BountyData } from "./BountyBoard";

type MostWantedProps = {
  bounties: BountyData[];
};

type WantedTarget = {
  targetItemId: string;
  targetTenant: string;
  totalReward: number;
  bountyCount: number;
  contributorCount: number;
  topPoster: string;
};

export function MostWanted({ bounties }: MostWantedProps) {
  // Aggregate bounties by target
  const targetMap = new Map<string, WantedTarget>();

  for (const b of bounties) {
    const key = `${b.target_item_id}:${b.target_tenant}`;
    const existing = targetMap.get(key);
    if (existing) {
      existing.totalReward += b.reward_quantity;
      existing.bountyCount += 1;
      existing.contributorCount += b.contributors.length;
    } else {
      targetMap.set(key, {
        targetItemId: b.target_item_id,
        targetTenant: b.target_tenant,
        totalReward: b.reward_quantity,
        bountyCount: 1,
        contributorCount: b.contributors.length,
        topPoster: b.poster,
      });
    }
  }

  const sorted = Array.from(targetMap.values())
    .sort((a, b) => b.totalReward - a.totalReward)
    .slice(0, 5);

  if (sorted.length === 0) return null;

  const rankColors = [
    "border-tier-gold text-tier-gold",
    "border-tier-silver text-tier-silver",
    "border-tier-bronze text-tier-bronze",
    "border-border-hover text-text-secondary",
    "border-border-default text-text-muted",
  ];

  return (
    <div className="mb-8">
      <h3 className="text-lg tracking-wider mb-4 flex items-center gap-2">
        <Skull className="w-5 h-5 text-accent-red" />
        MOST WANTED
      </h3>
      <div className="flex gap-3 overflow-x-auto pb-2">
        {sorted.map((target, i) => (
          <div
            key={`${target.targetItemId}:${target.targetTenant}`}
            className={`glass-card p-4 min-w-[180px] flex-shrink-0 border-l-2 ${rankColors[i] || rankColors[4]} ${
              i === 0 ? "neon-glow-gold" : ""
            }`}
          >
            <div className="flex items-center gap-2 mb-2">
              <span className={`text-xs font-bold ${rankColors[i]?.split(" ")[1] || "text-text-muted"}`}>
                #{i + 1}
              </span>
              <span className="text-sm font-bold text-text-primary truncate">
                Character #{target.targetItemId}
              </span>
            </div>
            <div className="text-2xl font-bold text-accent-gold mb-1">
              {target.totalReward}x
            </div>
            <div className="flex items-center gap-2 text-xs text-text-muted">
              {target.contributorCount > 1 && (
                <span className="flex items-center gap-1">
                  <Users className="w-3 h-3" />
                  {target.contributorCount}
                </span>
              )}
              <span>{target.bountyCount} {target.bountyCount === 1 ? "bounty" : "bounties"}</span>
            </div>
          </div>
        ))}
      </div>
    </div>
  );
}
