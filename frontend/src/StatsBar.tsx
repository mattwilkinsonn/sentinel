import { Activity, MapPin, Radio, Shield, Skull, UserPlus } from "lucide-solid";
import type { SubView } from "./SentinelDashboard";
import { Tooltip } from "./Tooltip";
import type { AggregateStats, ThreatProfile } from "./types";

type StatsBarProps = {
  stats: AggregateStats;
  /** Full profile list; used to compute the "Active Threats" count (score > 2500). */
  profiles: ThreatProfile[];
  /** Pre-counted number of new pilots in the last 24 hours (passed in from parent). */
  newPilotCount: number;
  /** Highlights the tile whose `view` matches this value. */
  activeView: SubView;
  /** Called with the target sub-view when any tile is clicked. */
  onStatClick: (view: SubView) => void;
};

/**
 * Six-tile navigation bar displaying key aggregate metrics. Each tile is
 * clickable and navigates to its associated sub-view; the currently active
 * tile is highlighted with a cyan border. Tooltips explain each metric on hover.
 */
export function StatsBar(props: StatsBarProps) {
  const statItems = () => [
    {
      label: "EVENTS 24H",
      value: props.stats.total_events.toString(),
      icon: Radio,
      color: "text-accent-green",
      view: "feed" as SubView,
      tooltip:
        "Events tracked in the last 24 hours. Click to view the live intel feed.",
    },
    {
      label: "KILLS 24H",
      value: props.stats.kills_24h.toString(),
      icon: Skull,
      color: "text-accent-red",
      view: "kills" as SubView,
      tooltip:
        "Total kills recorded across all pilots in the last 24 hours. Click to view kill statistics.",
    },
    {
      label: "ACTIVE THREATS",
      value: props.profiles
        .filter((p) => p.threat_score > 2500)
        .length.toString(),
      icon: Activity,
      color: "text-accent-gold",
      view: "leaderboard" as SubView,
      tooltip:
        "Pilots with threat score above MODERATE (25+). Click to view the threat leaderboard.",
    },
    {
      label: "TOP SYSTEM",
      value: props.stats.top_system || "—",
      icon: MapPin,
      color: "text-accent-purple",
      view: "systems" as SubView,
      tooltip:
        "The solar system with the most tracked pilots. Click to view system intelligence.",
    },
    {
      label: "TRACKED",
      value: props.stats.total_tracked.toString(),
      icon: Shield,
      color: "text-accent-cyan",
      view: "tracked" as SubView,
      tooltip:
        "Total number of pilots being monitored by the threat network. Click to view all tracked pilots.",
    },
    {
      label: "NEW PILOTS 24H",
      value: props.newPilotCount.toString(),
      icon: UserPlus,
      color: "text-text-primary",
      view: "pilots" as SubView,
      tooltip:
        "Newly detected pilots on the frontier. Click to view recent arrivals.",
    },
  ];

  return (
    <div class="grid grid-cols-3 lg:grid-cols-6 gap-3">
      {statItems().map((item) => (
        <Tooltip text={item.tooltip}>
          <button
            type="button"
            onClick={() => props.onStatClick(item.view)}
            class={`glass-card p-4 text-left bg-transparent transition-all w-full h-full ${
              props.activeView === item.view
                ? "border-accent-cyan"
                : "border-border-default"
            }`}
          >
            <div class="flex items-center gap-2 mb-1">
              <item.icon size={14} class={item.color} />
              <span class="text-xs text-text-muted tracking-wider whitespace-nowrap">
                {item.label}
              </span>
            </div>
            <div class={`text-xl font-bold ${item.color}`}>{item.value}</div>
          </button>
        </Tooltip>
      ))}
    </div>
  );
}
