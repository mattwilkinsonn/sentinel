import { useState, useEffect } from "react";
import { abbreviateAddress } from "@evefrontier/dapp-kit";
import { Target, Clock, User, Users, ChevronDown, ChevronUp } from "lucide-react";
import type { BountyData } from "./BountyBoard";

function useCountdown(expiresAtMs: string) {
  const [now, setNow] = useState(Date.now());
  const expires = Number(expiresAtMs);

  useEffect(() => {
    if (expires === 0 || expires < Date.now()) return;
    const urgentThreshold = 5 * 60 * 1000; // 5 minutes
    const diff = expires - Date.now();
    const interval = diff < urgentThreshold ? 1000 : 60000;
    const timer = setInterval(() => setNow(Date.now()), interval);
    return () => clearInterval(timer);
  }, [expires]);

  if (expires === 0) return "N/A";
  const diff = expires - now;
  if (diff <= 0) return "EXPIRED";

  const days = Math.floor(diff / (1000 * 60 * 60 * 24));
  const hours = Math.floor((diff % (1000 * 60 * 60 * 24)) / (1000 * 60 * 60));
  const mins = Math.floor((diff % (1000 * 60 * 60)) / (1000 * 60));
  const secs = Math.floor((diff % (1000 * 60)) / 1000);

  if (days > 0) return `${days}d ${hours}h`;
  if (hours > 0) return `${hours}h ${mins}m`;
  if (mins > 0) return `${mins}m ${secs}s`;
  return `${secs}s`;
}

function getRewardTier(quantity: number): { label: string; className: string } {
  if (quantity >= 100) return { label: "DIAMOND", className: "text-tier-diamond" };
  if (quantity >= 50) return { label: "GOLD", className: "text-tier-gold" };
  if (quantity >= 10) return { label: "SILVER", className: "text-tier-silver" };
  return { label: "BRONZE", className: "text-tier-bronze" };
}

type BountyCardProps = {
  bounty: BountyData;
  walletAddress: string;
  builderPackageId: string;
  bountyBoardId: string;
  onAction: () => void;
};

export function BountyCard({ bounty, walletAddress }: BountyCardProps) {
  const [showContributors, setShowContributors] = useState(false);
  const countdown = useCountdown(bounty.expires_at);
  const isExpired = countdown === "EXPIRED" && !bounty.claimed;
  const isPoster = bounty.poster.toLowerCase() === walletAddress.toLowerCase();
  const tier = getRewardTier(bounty.reward_quantity);

  // Urgency: less than 2 hours
  const expiresIn = Number(bounty.expires_at) - Date.now();
  const isUrgent = !bounty.claimed && !isExpired && expiresIn < 2 * 60 * 60 * 1000 && expiresIn > 0;
  const isHighValue = bounty.reward_quantity >= 50;
  const hasMultipleContributors = bounty.contributors.length > 1;

  let cardClass = "glass-card p-4 transition-all";
  if (bounty.claimed) cardClass += " opacity-50";
  else if (isExpired) cardClass += " opacity-40";
  if (isUrgent) cardClass += " neon-glow-urgent";
  else if (isHighValue && !bounty.claimed) cardClass += " neon-glow-gold";

  return (
    <div className={cardClass}>
      <div className="flex justify-between items-start gap-4">
        {/* Left side */}
        <div className="flex-1 min-w-0">
          {/* Title + badges */}
          <div className="flex items-center gap-2 mb-2 flex-wrap">
            <span className="font-bold text-text-primary tracking-wide">
              BOUNTY #{bounty.id}
            </span>
            {bounty.claimed && (
              <span className="badge bg-accent-green/15 text-accent-green">CLAIMED</span>
            )}
            {isExpired && (
              <span className="badge bg-accent-red/15 text-accent-red">EXPIRED</span>
            )}
            {isPoster && (
              <span className="badge bg-accent-purple/15 text-accent-purple">YOUR BOUNTY</span>
            )}
            {hasMultipleContributors && (
              <span className="badge bg-accent-cyan/15 text-accent-cyan">
                <Users className="w-3 h-3 inline mr-1" />
                {bounty.contributors.length} STACKED
              </span>
            )}
          </div>

          {/* Details */}
          <div className="flex flex-col gap-1">
            <div className="flex items-center gap-1.5 text-sm text-text-secondary">
              <Target className="w-3.5 h-3.5 text-accent-red" />
              Target: Character #{bounty.target_item_id}
              <span className="text-text-muted">({bounty.target_tenant})</span>
            </div>
            <div className="flex items-center gap-1.5 text-sm text-text-secondary">
              <User className="w-3.5 h-3.5" />
              Posted by: {abbreviateAddress(bounty.poster)}
            </div>
            {bounty.claimed && bounty.claimed_by && (
              <div className="text-sm text-accent-green">
                Claimed by: {abbreviateAddress(bounty.claimed_by)}
              </div>
            )}
          </div>

          {/* Contributors expandable */}
          {hasMultipleContributors && (
            <div className="mt-2">
              <button
                onClick={() => setShowContributors(!showContributors)}
                className="flex items-center gap-1 text-xs text-text-muted hover:text-text-secondary bg-transparent border-none p-0 cursor-pointer"
              >
                {showContributors ? <ChevronUp className="w-3 h-3" /> : <ChevronDown className="w-3 h-3" />}
                {showContributors ? "Hide" : "Show"} contributors
              </button>
              {showContributors && (
                <div className="mt-1.5 pl-2 border-l border-border-default">
                  {bounty.contributors.map((c, i) => (
                    <div key={i} className="text-xs text-text-muted py-0.5">
                      {abbreviateAddress(c.contributor)}: {c.amount}x
                    </div>
                  ))}
                </div>
              )}
            </div>
          )}
        </div>

        {/* Right side: reward + timer */}
        <div className="text-right shrink-0">
          <div className={`text-2xl font-bold ${tier.className}`}>
            {bounty.reward_quantity}x
          </div>
          <div className="text-xs text-text-muted">
            Type #{bounty.reward_type_id}
          </div>
          <div className={`text-xs mt-0.5 ${tier.className}`}>
            {tier.label}
          </div>
          {!bounty.claimed && (
            <div className={`flex items-center justify-end gap-1 mt-2 text-xs ${
              isExpired ? "text-accent-red" : isUrgent ? "text-accent-red" : "text-text-muted"
            }`}>
              <Clock className="w-3 h-3" />
              {countdown}
            </div>
          )}
        </div>
      </div>
    </div>
  );
}
