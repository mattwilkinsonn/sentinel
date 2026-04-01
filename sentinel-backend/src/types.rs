use serde::Serialize;
use std::collections::{HashMap, VecDeque};

/// In-memory threat profile for a character.
#[derive(Clone, Debug, Default, Serialize)]
pub struct ThreatProfile {
    pub character_item_id: u64,
    pub name: Option<String>,
    pub threat_score: u64,
    pub kill_count: u64,
    pub death_count: u64,
    pub bounty_count: u64,
    pub last_kill_timestamp: u64,
    pub last_seen_system: String,
    pub last_seen_system_name: String,
    pub tribe_id: String,
    pub tribe_name: String,
    /// Kills in the last 7 days (for recency scoring)
    pub recent_kills_24h: u64,
    /// Deaths in the last 7 days (for recent K/D scoring)
    pub recent_deaths_24h: u64,
    /// Number of unique systems visited
    pub systems_visited: u64,
    /// Whether this profile has been modified since last DB sync
    #[serde(skip)]
    pub dirty: bool,
}

/// A raw event captured from the checkpoint stream.
#[derive(Clone, Debug, Serialize)]
pub struct RawEvent {
    pub event_type: String,
    pub timestamp_ms: u64,
    pub data: serde_json::Value,
}

/// Aggregate stats for the dashboard.
#[derive(Clone, Debug, Default, Serialize)]
pub struct AggregateStats {
    pub total_tracked: u64,
    pub avg_score: u64,
    pub kills_24h: u64,
    pub top_system: String,
    pub total_events: u64,
    /// True when the event ring-buffer is full — the real 24h count may be higher.
    pub events_at_cap: bool,
}

/// A single data store (profiles + events + names).
#[derive(Debug, Default)]
pub struct DataStore {
    pub profiles: HashMap<u64, ThreatProfile>,
    pub recent_events: VecDeque<RawEvent>,
    pub new_pilot_events: VecDeque<RawEvent>,
    pub name_cache: HashMap<u64, String>,
    /// Solar system ID → name cache (populated by metadata resolver)
    pub system_name_cache: HashMap<String, String>,
    /// character_item_id → Sui object ID cache (populated during checkpoint replay)
    pub object_id_cache: HashMap<u64, String>,
    /// Gate object ID → display name cache (populated via gRPC lookups)
    pub gate_name_cache: HashMap<String, String>,
    /// structure item_id → type_id (persisted to DB, populated lazily on structure kills)
    pub structure_type_cache: HashMap<u64, u64>,
    /// type_id → human name (in-memory only, fetched from World API, e.g. 92279 → "Mini Turret")
    pub type_name_cache: HashMap<u64, String>,
}

impl DataStore {
    /// Resolve a structure's display name from its item_id.
    /// Returns e.g. "Mini Turret", "Mini Gate", or "Structure" as fallback.
    pub fn resolve_structure_name(&self, item_id: u64) -> String {
        self.structure_type_cache
            .get(&item_id)
            .and_then(|type_id| self.type_name_cache.get(type_id))
            .cloned()
            .unwrap_or_else(|| "Structure".to_string())
    }
}

impl DataStore {
    /// Prepend an event to the front of the ring-buffer and broadcast it over SSE.
    /// `new_character` events go into `new_pilot_events`; all others into `recent_events`.
    /// `recent_events` is capped at `cap`; `new_pilot_events` is capped at 500.
    pub fn push_event(
        &mut self,
        event: RawEvent,
        sse_tx: &Option<tokio::sync::broadcast::Sender<String>>,
        cap: usize,
    ) {
        if let Some(tx) = sse_tx {
            if let Ok(json) = serde_json::to_string(&event) {
                let _ = tx.send(json);
            }
        }
        if event.event_type == "new_character" {
            self.new_pilot_events.push_front(event);
            if self.new_pilot_events.len() > 500 {
                self.new_pilot_events.pop_back();
            }
        } else {
            self.recent_events.push_front(event);
            if self.recent_events.len() > cap {
                self.recent_events.pop_back();
            }
        }
    }

    /// Compute aggregate dashboard stats from the current profile + event set.
    /// `total_events` counts only events from the last 24 hours.
    /// `events_at_cap` is true when the ring-buffer is full (real count may be higher).
    /// `top_system` is the solar system ID or name where the most pilots were last seen.
    pub fn compute_stats(&self, cap: usize) -> AggregateStats {
        let total = self.profiles.len() as u64;
        let sum_score: u64 = self.profiles.values().map(|p| p.threat_score).sum();
        let avg = if total > 0 { sum_score / total } else { 0 };
        let kills_24h: u64 = self.profiles.values().map(|p| p.recent_kills_24h).sum();

        let mut system_counts: HashMap<&str, u64> = HashMap::new();
        for p in self.profiles.values() {
            if !p.last_seen_system.is_empty() {
                *system_counts.entry(&p.last_seen_system).or_default() += 1;
            }
        }
        let top_system_id = system_counts
            .into_iter()
            .max_by_key(|(_, c)| *c)
            .map(|(s, _)| s.to_string())
            .unwrap_or_default();
        // Resolve name: try cache, then check profiles for a resolved name
        let top_system = if top_system_id.is_empty() {
            String::new()
        } else {
            self.system_name_cache
                .get(&top_system_id)
                .filter(|n| !n.is_empty())
                .cloned()
                .or_else(|| {
                    self.profiles
                        .values()
                        .find(|p| p.last_seen_system == top_system_id)
                        .map(|p| &p.last_seen_system_name)
                        .filter(|n| !n.is_empty())
                        .cloned()
                })
                .unwrap_or(top_system_id)
        };

        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;
        let day_ago = now.saturating_sub(86_400_000);
        let events_24h = self
            .recent_events
            .iter()
            .filter(|e| e.timestamp_ms >= day_ago)
            .count() as u64;

        AggregateStats {
            total_tracked: total,
            avg_score: avg,
            kills_24h,
            top_system,
            total_events: events_24h,
            events_at_cap: self.recent_events.len() >= cap,
        }
    }
}

/// Shared application state with both demo and live data.
#[derive(Debug, Default)]
pub struct AppState {
    pub demo: DataStore,
    pub live: DataStore,
    /// Last processed checkpoint cursor
    pub last_checkpoint: Option<u64>,
    /// Wall-clock time when the last checkpoint was processed (for stream health checks)
    pub last_checkpoint_at: Option<std::time::Instant>,
    /// SSE broadcast channel sender
    pub sse_tx: Option<tokio::sync::broadcast::Sender<String>>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_event_prepends_to_front() {
        let mut store = DataStore::default();
        let no_sse: Option<tokio::sync::broadcast::Sender<String>> = None;
        store.push_event(
            RawEvent {
                event_type: "kill".into(),
                timestamp_ms: 1000,
                data: serde_json::json!({}),
            },
            &no_sse,
            1000,
        );
        store.push_event(
            RawEvent {
                event_type: "jump".into(),
                timestamp_ms: 2000,
                data: serde_json::json!({}),
            },
            &no_sse,
            1000,
        );

        assert_eq!(store.recent_events.len(), 2);
        assert_eq!(store.recent_events[0].event_type, "jump");
        assert_eq!(store.recent_events[1].event_type, "kill");
    }

    #[test]
    fn push_event_caps_at_configured_limit() {
        let mut store = DataStore::default();
        let no_sse: Option<tokio::sync::broadcast::Sender<String>> = None;
        for i in 0..1200u64 {
            store.push_event(
                RawEvent {
                    event_type: "kill".into(),
                    timestamp_ms: i,
                    data: serde_json::json!({}),
                },
                &no_sse,
                1000,
            );
        }
        assert_eq!(store.recent_events.len(), 1000);
        assert_eq!(store.recent_events[0].timestamp_ms, 1199);
    }

    #[test]
    fn compute_stats_empty() {
        let store = DataStore::default();
        let stats = store.compute_stats(1000);
        assert_eq!(stats.total_tracked, 0);
        assert_eq!(stats.avg_score, 0);
        assert_eq!(stats.kills_24h, 0);
        assert_eq!(stats.top_system, "");
    }

    #[test]
    fn compute_stats_avg_score() {
        let mut store = DataStore::default();
        store.profiles.insert(
            1,
            ThreatProfile {
                threat_score: 6000,
                ..Default::default()
            },
        );
        store.profiles.insert(
            2,
            ThreatProfile {
                threat_score: 4000,
                ..Default::default()
            },
        );
        let stats = store.compute_stats(1000);
        assert_eq!(stats.total_tracked, 2);
        assert_eq!(stats.avg_score, 5000);
    }

    #[test]
    fn compute_stats_kills_24h() {
        let mut store = DataStore::default();
        store.profiles.insert(
            1,
            ThreatProfile {
                recent_kills_24h: 5,
                ..Default::default()
            },
        );
        store.profiles.insert(
            2,
            ThreatProfile {
                recent_kills_24h: 3,
                ..Default::default()
            },
        );
        let stats = store.compute_stats(1000);
        assert_eq!(stats.kills_24h, 8);
    }

    #[test]
    fn compute_stats_top_system() {
        let mut store = DataStore::default();
        store.profiles.insert(
            1,
            ThreatProfile {
                last_seen_system: "J-1042".into(),
                ..Default::default()
            },
        );
        store.profiles.insert(
            2,
            ThreatProfile {
                last_seen_system: "X-4419".into(),
                ..Default::default()
            },
        );
        store.profiles.insert(
            3,
            ThreatProfile {
                last_seen_system: "J-1042".into(),
                ..Default::default()
            },
        );
        let stats = store.compute_stats(1000);
        assert_eq!(stats.top_system, "J-1042");
    }

    #[test]
    fn compute_stats_ignores_empty_systems() {
        let mut store = DataStore::default();
        store.profiles.insert(
            1,
            ThreatProfile {
                last_seen_system: "".into(),
                ..Default::default()
            },
        );
        store.profiles.insert(
            2,
            ThreatProfile {
                last_seen_system: "K-9731".into(),
                ..Default::default()
            },
        );
        let stats = store.compute_stats(1000);
        assert_eq!(stats.top_system, "K-9731");
    }

    #[test]
    fn dirty_flag_not_serialized() {
        let p = ThreatProfile {
            dirty: true,
            threat_score: 5000,
            ..Default::default()
        };
        let json = serde_json::to_string(&p).unwrap();
        assert!(!json.contains("dirty"));
        assert!(json.contains("threat_score"));
    }

    #[test]
    fn push_event_broadcasts_via_sse() {
        let (tx, mut rx) = tokio::sync::broadcast::channel::<String>(16);
        let mut store = DataStore::default();
        let sse: Option<tokio::sync::broadcast::Sender<String>> = Some(tx);

        store.push_event(
            RawEvent {
                event_type: "kill".into(),
                timestamp_ms: 1000,
                data: serde_json::json!({"killer": 42}),
            },
            &sse,
            1000,
        );

        let msg = rx.try_recv().unwrap();
        assert!(msg.contains("kill"));
        assert!(msg.contains("42"));
    }
}
