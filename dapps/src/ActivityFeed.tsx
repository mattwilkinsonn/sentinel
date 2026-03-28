import { abbreviateAddress } from "@evefrontier/dapp-kit";
import { Activity, Crosshair, Trophy, XCircle, Layers } from "lucide-react";
import type { ActivityEvent } from "./BountyBoard";

function timeAgo(timestamp: number): string {
  if (timestamp === 0) return "";
  const diff = Date.now() - timestamp;
  if (diff < 60000) return "just now";
  if (diff < 3600000) return `${Math.floor(diff / 60000)}m ago`;
  if (diff < 86400000) return `${Math.floor(diff / 3600000)}h ago`;
  return `${Math.floor(diff / 86400000)}d ago`;
}

const eventConfig = {
  posted: { icon: Crosshair, color: "text-accent-cyan", verb: "posted" },
  claimed: { icon: Trophy, color: "text-accent-green", verb: "claimed" },
  cancelled: { icon: XCircle, color: "text-accent-red", verb: "cancelled" },
  stacked: { icon: Layers, color: "text-accent-purple", verb: "stacked on" },
};

export function ActivityFeed({ events }: { events: ActivityEvent[] }) {
  return (
    <div className="glass-card p-4 sticky top-24">
      <h4 className="text-sm tracking-wider mb-3 flex items-center gap-2 text-text-secondary">
        <Activity className="w-4 h-4 text-accent-cyan" />
        LIVE FEED
      </h4>
      <div className="flex flex-col gap-2 max-h-[500px] overflow-y-auto">
        {events.map((ev, i) => {
          const config = eventConfig[ev.type];
          const Icon = config.icon;
          return (
            <div
              key={`${ev.type}-${ev.bountyId}-${ev.timestamp}-${i}`}
              className="flex items-start gap-2 text-xs py-1.5 border-b border-border-default/50 last:border-0"
              style={{ animation: `slideIn 0.3s ease ${i * 0.05}s both` }}
            >
              <Icon className={`w-3.5 h-3.5 shrink-0 mt-0.5 ${config.color}`} />
              <div className="min-w-0">
                <span className="text-text-secondary">
                  {abbreviateAddress(ev.actor)}{" "}
                  <span className={config.color}>{config.verb}</span>{" "}
                  bounty #{ev.bountyId}
                </span>
                {ev.rewardQuantity ? (
                  <span className="text-text-muted"> ({ev.rewardQuantity}x)</span>
                ) : null}
                <div className="text-text-muted mt-0.5">{timeAgo(ev.timestamp)}</div>
              </div>
            </div>
          );
        })}
        {events.length === 0 && (
          <p className="text-text-muted text-xs">No activity yet</p>
        )}
      </div>
    </div>
  );
}
