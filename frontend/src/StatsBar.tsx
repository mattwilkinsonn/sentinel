import { Shield, Skull, Activity, MapPin, Radio } from "lucide-solid";
import type { AggregateStats } from "./types";
import type { SubView } from "./SentinelDashboard";
import { Tooltip } from "./Tooltip";

type StatsBarProps = {
  stats: AggregateStats;
  activeView: SubView;
  onStatClick: (view: SubView) => void;
};

export function StatsBar(props: StatsBarProps) {
  const statItems = () => [
    {
      label: "EVENTS/MIN",
      value: props.stats.events_per_min.toString(),
      icon: Radio,
      color: "text-accent-green",
      view: "feed" as SubView,
      tooltip:
        "Events processed in the last 60 seconds. Click to view the live intel feed.",
    },
    {
      label: "AVG THREAT",
      value: (props.stats.avg_score / 100).toFixed(2),
      icon: Activity,
      color: "text-accent-gold",
      view: "leaderboard" as SubView,
      tooltip:
        "Average threat score across all tracked pilots (0-100). Click to view the threat leaderboard.",
    },
    {
      label: "TRACKED",
      value: props.stats.total_tracked.toString(),
      icon: Shield,
      color: "text-accent-cyan",
      view: "tracked" as SubView,
      tooltip: "Total number of pilots being monitored by the threat network.",
    },
    {
      label: "KILLS 24H",
      value: props.stats.kills_24h.toString(),
      icon: Skull,
      color: "text-accent-red",
      view: "kills" as SubView,
      tooltip: "Total kills recorded across all pilots in the last 24 hours.",
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
  ];

  return (
    <div class="grid grid-cols-3 lg:grid-cols-5 gap-3">
      {statItems().map((item) => (
        <Tooltip text={item.tooltip}>
          <button
            onClick={() => props.onStatClick(item.view)}
            class={`glass-card p-4 text-left bg-transparent transition-all w-full ${
              props.activeView === item.view
                ? "border-accent-cyan"
                : "border-border-default"
            }`}
          >
            <div class="flex items-center gap-2 mb-1">
              <item.icon size={14} class={item.color} />
              <span class="text-xs text-text-muted tracking-wider">
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
