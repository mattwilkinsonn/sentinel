import { Clock, MapPin, Shield, Skull, Target, X } from "lucide-solid";
import type { ThreatProfile } from "./types";
import { getThreatColor, getThreatColorClass, getThreatTier } from "./types";

type ThreatCardProps = {
  profile: ThreatProfile;
  onClose: () => void;
};

export function ThreatCard(props: ThreatCardProps) {
  const tier = () => getThreatTier(props.profile.threat_score);
  const color = () => getThreatColor(tier());
  const colorClass = () => getThreatColorClass(tier());
  const scoreDisplay = () => (props.profile.threat_score / 100).toFixed(2);
  const kd = () =>
    props.profile.death_count > 0
      ? (props.profile.kill_count / props.profile.death_count).toFixed(2)
      : props.profile.kill_count.toFixed(2);

  const lastKillAgo = () => {
    if (!props.profile.last_kill_timestamp) return "Never";
    const diff = Date.now() - props.profile.last_kill_timestamp;
    if (diff < 60_000) return "Just now";
    if (diff < 3_600_000) return `${Math.floor(diff / 60_000)}m ago`;
    if (diff < 86_400_000) return `${Math.floor(diff / 3_600_000)}h ago`;
    return `${Math.floor(diff / 86_400_000)}d ago`;
  };

  return (
    <div
      class="glass-card p-5"
      style={{ "border-left": `3px solid ${color()}` }}
    >
      <div class="flex items-start justify-between mb-4">
        <div>
          <h4 class="text-lg tracking-wider">
            {props.profile.name || `Pilot #${props.profile.character_item_id}`}
          </h4>
          <span class="text-xs text-text-muted">
            #{props.profile.character_item_id}
          </span>
          <div class="flex items-center gap-2 mt-1">
            <span
              class={`badge ${colorClass()}`}
              style={{ background: `${color()}15` }}
            >
              {tier()} THREAT
            </span>
            <span class={`text-2xl font-bold ${colorClass()}`}>
              {scoreDisplay()}
            </span>
          </div>
        </div>
        <button
          type="button"
          onClick={props.onClose}
          class="text-text-muted hover:text-text-primary bg-transparent border-none p-1"
        >
          <X size={18} />
        </button>
      </div>

      {/* Score bar */}
      <div style="height:10px;border-radius:5px;background:rgba(250,250,229,0.08);overflow:hidden;margin-bottom:1rem">
        <div
          style={`width:${props.profile.threat_score / 100}%;height:10px;border-radius:5px;background:${color()}`}
        />
      </div>

      {/* Stats grid */}
      <div class="grid grid-cols-2 lg:grid-cols-3 gap-3">
        <StatItem
          icon={Skull}
          label="KILLS"
          value={props.profile.kill_count.toString()}
          color="text-accent-red"
        />
        <StatItem
          icon={Shield}
          label="DEATHS"
          value={props.profile.death_count.toString()}
          color="text-text-secondary"
        />
        <StatItem
          icon={Skull}
          label="K/D RATIO"
          value={kd()}
          color="text-accent-gold"
        />
        <StatItem
          icon={Target}
          label="BOUNTIES"
          value={props.profile.bounty_count.toString()}
          color="text-accent-cyan"
        />
        <StatItem
          icon={MapPin}
          label="SYSTEMS"
          value={props.profile.systems_visited.toString()}
          color="text-accent-purple"
        />
        <StatItem
          icon={Clock}
          label="LAST KILL"
          value={lastKillAgo()}
          color="text-text-muted"
        />
      </div>

      {props.profile.last_seen_system && (
        <div class="mt-3 text-xs text-text-muted">
          Last seen in system:{" "}
          <span class="text-text-secondary">
            {props.profile.last_seen_system}
          </span>
        </div>
      )}
    </div>
  );
}

function StatItem(props: {
  icon: typeof Skull;
  label: string;
  value: string;
  color: string;
}) {
  return (
    <div class="flex items-center gap-2">
      <props.icon size={14} class={props.color} />
      <div>
        <div class="text-xs text-text-muted">{props.label}</div>
        <div class={`text-sm font-bold ${props.color}`}>{props.value}</div>
      </div>
    </div>
  );
}
