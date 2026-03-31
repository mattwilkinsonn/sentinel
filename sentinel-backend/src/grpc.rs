//! gRPC checkpoint streaming — ingests events from Sui fullnode.

use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::transport::Channel;

use crate::config::AppConfig;
use crate::threat_engine;
use crate::types::{AppState, RawEvent, ThreatProfile};

// Generated gRPC client code
pub mod sui_rpc {
    tonic::include_proto!("sui.rpc.v2");
}

use sui_rpc::SubscribeCheckpointsRequest;
use sui_rpc::subscription_service_client::SubscriptionServiceClient;

/// Connect to Sui fullnode and stream checkpoints forever (with reconnect).
pub async fn stream_checkpoints(config: AppConfig, state: Arc<RwLock<AppState>>) {
    loop {
        tracing::info!(url = %config.sui_grpc_url, "Connecting to gRPC stream at {}", config.sui_grpc_url);
        match run_stream(&config, &state).await {
            Ok(()) => tracing::warn!("gRPC stream ended cleanly, reconnecting..."),
            Err(e) => tracing::error!(error = %e, "gRPC stream error: {e}, reconnecting in 2s..."),
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

/// Open a single gRPC checkpoint subscription and process events until the stream ends.
/// Returns `Ok(())` on clean stream end (caller reconnects), or `Err` on transport failure.
async fn run_stream(
    config: &AppConfig,
    state: &Arc<RwLock<AppState>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let channel = Channel::from_shared(config.sui_grpc_url.clone())?
        .tls_config(tonic::transport::ClientTlsConfig::new().with_webpki_roots())?
        .connect()
        .await?;

    let mut client = SubscriptionServiceClient::new(channel);

    // Request checkpoint stream with events included
    let request = SubscribeCheckpointsRequest {
        read_mask: Some(prost_types::FieldMask {
            paths: vec!["checkpoint.transactions".into()],
        }),
    };

    let mut stream = client.subscribe_checkpoints(request).await?.into_inner();

    tracing::info!("gRPC checkpoint stream connected");
    tracing::info!(
        world_pkg = %config.world_package_id,
        bounty_pkg = %config.bounty_board_package_id,
        sentinel_pkg = %config.sentinel_package_id,
        "Filtering for packages: world={}, bounty={}, sentinel={}",
        config.world_package_id,
        config.bounty_board_package_id,
        config.sentinel_package_id
    );

    let mut checkpoint_count: u64 = 0;
    while let Some(response) = stream.message().await? {
        let cursor = response.cursor.unwrap_or(0);
        checkpoint_count += 1;

        if let Some(checkpoint) = response.checkpoint {
            process_checkpoint(config, state, &checkpoint, cursor).await;
        }

        // Update cursor and heartbeat timestamp
        {
            let mut s = state.write().await;
            s.last_checkpoint = Some(cursor);
            s.last_checkpoint_at = Some(std::time::Instant::now());
        }

        // Heartbeat every 100 checkpoints
        if checkpoint_count % 100 == 0 {
            tracing::debug!(
                checkpoint_count,
                cursor,
                "gRPC stream alive — processed {checkpoint_count} checkpoints, cursor={cursor}"
            );
        }
    }

    Ok(())
}

/// Process all events in a single live checkpoint.
/// Gate names are pre-resolved via gRPC before acquiring the write lock, to keep lock
/// hold time short. The first 20 events are sampled to debug package ID filtering.
async fn process_checkpoint(
    config: &AppConfig,
    state: &Arc<RwLock<AppState>>,
    checkpoint: &sui_rpc::Checkpoint,
    cursor: u64,
) {
    use std::sync::atomic::{AtomicU64, Ordering};
    static SAMPLE_COUNT: AtomicU64 = AtomicU64::new(0);
    let timestamp_ms = checkpoint_timestamp_ms(checkpoint);

    // Pre-scan: collect gate IDs from jump events that aren't cached yet
    let uncached_gate_ids = {
        let s = state.read().await;
        collect_uncached_gate_ids(config, &s.live.gate_name_cache, checkpoint)
    };

    // Resolve uncached gate names via gRPC (outside the state lock)
    if !uncached_gate_ids.is_empty() {
        let channel = crate::sui_client::connect(&config.sui_grpc_url).await.ok();
        if let Some(ch) = channel {
            let mut s = state.write().await;
            for gate_id in &uncached_gate_ids {
                let name = crate::historical::resolve_gate_name(
                    ch.clone(),
                    gate_id,
                    &mut s.live.gate_name_cache,
                )
                .await;
                tracing::debug!(gate_id, gate_name = %name, "Resolved gate {gate_id} → {name}");
            }
        }
    }

    for tx in &checkpoint.transactions {
        let events = tx
            .events
            .as_ref()
            .map(|e| &e.events[..])
            .unwrap_or_default();

        for event in events {
            let event_type = event.event_type.as_deref().unwrap_or("");
            let package_id = event.package_id.as_deref().unwrap_or("");

            // Sample first 20 events for debugging package IDs
            let sample = SAMPLE_COUNT.load(Ordering::Relaxed);
            if sample < 20 {
                SAMPLE_COUNT.fetch_add(1, Ordering::Relaxed);
                tracing::info!(
                    sample,
                    event_type,
                    package_id,
                    "Event sample #{sample}: type={event_type} pkg={package_id}"
                );
            }

            // Filter for events from world or bounty_board packages
            if package_id != config.world_package_id
                && package_id != config.bounty_board_package_id
                && package_id != config.sentinel_package_id
            {
                if event_type.contains("Kill") || event_type.contains("kill") {
                    tracing::info!(
                        event_type,
                        package_id,
                        want = %config.world_package_id,
                        "Skipped kill-like event: type={event_type} pkg={package_id} (want={})",
                        config.world_package_id
                    );
                }
                continue;
            }

            // Parse JSON payload
            let json_value = event
                .json
                .as_ref()
                .map(|v| proto_value_to_json(v))
                .unwrap_or(serde_json::Value::Null);

            // Route to handler
            let mut s = state.write().await;
            let sse_tx = s.sse_tx.clone();
            let store = &mut s.live;

            if event_type.contains("KillMailCreatedEvent")
                || event_type.contains("KillmailCreatedEvent")
            {
                handle_killmail(
                    store,
                    &sse_tx,
                    &json_value,
                    timestamp_ms,
                    config.max_recent_events,
                );
            } else if event_type.contains("BountyPostedEvent") {
                handle_bounty_posted(
                    store,
                    &sse_tx,
                    &json_value,
                    timestamp_ms,
                    config.max_recent_events,
                );
            } else if event_type.contains("BountyCancelledEvent")
                || event_type.contains("ContributionWithdrawnEvent")
            {
                handle_bounty_removed(
                    store,
                    &sse_tx,
                    &json_value,
                    timestamp_ms,
                    config.max_recent_events,
                );
            } else if event_type.contains("JumpEvent") {
                handle_jump(
                    store,
                    &sse_tx,
                    &json_value,
                    timestamp_ms,
                    config.max_recent_events,
                );
            }

            tracing::info!(
                cursor,
                event_type,
                package_id,
                "checkpoint={cursor} event={event_type} pkg={package_id}"
            );
        }
    }
}

/// Scan a checkpoint for jump events and return gate IDs not yet in the cache.
fn collect_uncached_gate_ids(
    config: &AppConfig,
    cache: &std::collections::HashMap<String, String>,
    checkpoint: &sui_rpc::Checkpoint,
) -> Vec<String> {
    let mut ids = Vec::new();
    for tx in &checkpoint.transactions {
        let events = tx
            .events
            .as_ref()
            .map(|e| &e.events[..])
            .unwrap_or_default();
        for event in events {
            let event_type = event.event_type.as_deref().unwrap_or("");
            let package_id = event.package_id.as_deref().unwrap_or("");
            if !event_type.contains("JumpEvent") {
                continue;
            }
            if package_id != config.world_package_id {
                continue;
            }
            let json = event
                .json
                .as_ref()
                .map(|v| proto_value_to_json(v))
                .unwrap_or(serde_json::Value::Null);
            for field in ["source_gate_id", "destination_gate_id"] {
                if let Some(gate_id) = json[field].as_str() {
                    if !gate_id.is_empty() && !cache.contains_key(gate_id) {
                        ids.push(gate_id.to_string());
                    }
                }
            }
        }
    }
    ids.sort();
    ids.dedup();
    ids
}

/// Extract timestamp from a checkpoint.
pub fn checkpoint_timestamp_ms(checkpoint: &sui_rpc::Checkpoint) -> u64 {
    checkpoint
        .summary
        .as_ref()
        .and_then(|s| s.timestamp.as_ref())
        .map(|ts| (ts.seconds as u64) * 1000 + (ts.nanos as u64) / 1_000_000)
        .unwrap_or(0)
}

/// Process events from a checkpoint into a DataStore.
/// Used by both live streaming and historical replay.
pub fn process_checkpoint_events(
    config: &AppConfig,
    store: &mut crate::types::DataStore,
    sse_tx: &Option<tokio::sync::broadcast::Sender<String>>,
    checkpoint: &sui_rpc::Checkpoint,
    timestamp_ms: u64,
) {
    for tx in &checkpoint.transactions {
        let events = tx
            .events
            .as_ref()
            .map(|e| &e.events[..])
            .unwrap_or_default();

        for event in events {
            let event_type = event.event_type.as_deref().unwrap_or("");
            let package_id = event.package_id.as_deref().unwrap_or("");

            if package_id != config.world_package_id
                && package_id != config.bounty_board_package_id
                && package_id != config.sentinel_package_id
            {
                continue;
            }

            tracing::debug!(
                event_type,
                package_id,
                timestamp_ms,
                "event dispatched: {event_type} from {package_id}"
            );

            let json_value = event
                .json
                .as_ref()
                .map(|v| proto_value_to_json(v))
                .unwrap_or(serde_json::Value::Null);

            if event_type.contains("KillMailCreatedEvent")
                || event_type.contains("KillmailCreatedEvent")
            {
                handle_killmail(
                    store,
                    sse_tx,
                    &json_value,
                    timestamp_ms,
                    config.max_recent_events,
                );
            } else if event_type.contains("BountyPostedEvent") {
                handle_bounty_posted(
                    store,
                    sse_tx,
                    &json_value,
                    timestamp_ms,
                    config.max_recent_events,
                );
            } else if event_type.contains("BountyCancelledEvent")
                || event_type.contains("ContributionWithdrawnEvent")
            {
                handle_bounty_removed(
                    store,
                    sse_tx,
                    &json_value,
                    timestamp_ms,
                    config.max_recent_events,
                );
            } else if event_type.contains("JumpEvent") {
                handle_jump(
                    store,
                    sse_tx,
                    &json_value,
                    timestamp_ms,
                    config.max_recent_events,
                );
            } else if event_type.contains("CharacterCreatedEvent") {
                handle_character_created(
                    store,
                    sse_tx,
                    &json_value,
                    timestamp_ms,
                    config.max_recent_events,
                );
            }
        }
    }
}

/// Handle a KillMailCreatedEvent: credit the killer, record the victim's death, push a kill event.
/// Structure kills (item_id >= STRUCTURE_ITEM_ID_MIN) are credited to the structure owner
/// (reported_by_character_id) rather than the structure itself.
fn handle_killmail(
    state: &mut crate::types::DataStore,
    sse_tx: &Option<tokio::sync::broadcast::Sender<String>>,
    json: &serde_json::Value,
    timestamp_ms: u64,
    cap: usize,
) {
    // Extract killer and victim character IDs (nested: {"item_id": "123", "tenant": "..."})
    let killer_id = json_item_id(json, "killer_id")
        .or_else(|| json_item_id(json, "killer_character_id"))
        .or_else(|| json_u64(json, "killerId"));
    let victim_id = json_item_id(json, "victim_id")
        .or_else(|| json_item_id(json, "victim_character_id"))
        .or_else(|| json_u64(json, "victimId"));
    let reported_by_id = json_item_id(json, "reported_by_character_id");
    let loss_is_structure = json["loss_type"]["@variant"]
        .as_str()
        .map(|v| v == "STRUCTURE")
        .unwrap_or(false);
    let system = json_item_id_str(json, "solar_system_id")
        .or_else(|| json_str(json, "solarSystemId"))
        .unwrap_or_default();
    let system_name = resolve_system_name(state, &system);

    // A structure killed the player when the killer item_id is in the structure
    // range (>= 1T). Credit the structure owner (reported_by_character_id) instead.
    let killed_by_structure = !loss_is_structure
        && killer_id
            .map(|k| k >= crate::historical::STRUCTURE_ITEM_ID_MIN)
            .unwrap_or(false);

    let (credited_killer, structure_name) = if killed_by_structure {
        let structure_item_id = killer_id.unwrap();
        let name = state.resolve_structure_name(structure_item_id);
        (reported_by_id.or(killer_id), Some(name))
    } else {
        (killer_id, None)
    };

    if let Some(killer) = credited_killer {
        let name = resolve_name(state, killer);
        let profile = state
            .profiles
            .entry(killer)
            .or_insert_with(|| ThreatProfile {
                character_item_id: killer,
                name,
                ..Default::default()
            });
        profile.kill_count += 1;
        profile.recent_kills_24h += 1;
        profile.last_kill_timestamp = timestamp_ms;
        profile.last_seen_system = system.clone();
        profile.last_seen_system_name = system_name.clone();
        profile.dirty = true;
        profile.threat_score = threat_engine::compute_score(profile);
    }

    if let Some(victim) = victim_id {
        if !loss_is_structure {
            let name = resolve_name(state, victim);
            let profile = state
                .profiles
                .entry(victim)
                .or_insert_with(|| ThreatProfile {
                    character_item_id: victim,
                    name,
                    ..Default::default()
                });
            profile.death_count += 1;
            profile.last_seen_system = system.clone();
            profile.last_seen_system_name = system_name.clone();
            profile.dirty = true;
            profile.threat_score = threat_engine::compute_score(profile);
        }
    }

    let event_type = if loss_is_structure {
        "structure_destroyed"
    } else {
        "kill"
    };

    let mut data = serde_json::json!({
        "killer_character_id": credited_killer,
        "target_item_id": victim_id,
        "solar_system_id": system,
    });
    if let Some(name) = structure_name {
        data["killed_by_structure"] = serde_json::json!(true);
        data["structure_name"] = serde_json::json!(name);
    }

    state.push_event(
        RawEvent {
            event_type: event_type.into(),
            timestamp_ms,
            data,
        },
        sse_tx,
        cap,
    );
}

/// Handle a BountyPostedEvent: increment the target's bounty count and push the event.
fn handle_bounty_posted(
    state: &mut crate::types::DataStore,
    sse_tx: &Option<tokio::sync::broadcast::Sender<String>>,
    json: &serde_json::Value,
    timestamp_ms: u64,
    cap: usize,
) {
    let target_id = json_item_id(json, "target_item_id").or_else(|| json_u64(json, "targetItemId"));

    if let Some(target) = target_id {
        let name = resolve_name(state, target);
        let profile = state
            .profiles
            .entry(target)
            .or_insert_with(|| ThreatProfile {
                character_item_id: target,
                name,
                ..Default::default()
            });
        profile.bounty_count += 1;
        profile.dirty = true;
        profile.threat_score = threat_engine::compute_score(profile);
    }

    state.push_event(
        RawEvent {
            event_type: "bounty_posted".into(),
            timestamp_ms,
            data: serde_json::json!({
                "target_item_id": target_id,
                "poster_id": json_item_id(json, "poster_id"),
            }),
        },
        sse_tx,
        cap,
    );
}

/// Handle a BountyCancelledEvent or ContributionWithdrawnEvent: decrement the bounty count
/// (saturating at 0). Only modifies profiles that already exist; unknown targets are ignored.
fn handle_bounty_removed(
    state: &mut crate::types::DataStore,
    sse_tx: &Option<tokio::sync::broadcast::Sender<String>>,
    json: &serde_json::Value,
    timestamp_ms: u64,
    cap: usize,
) {
    let target_id = json_item_id(json, "target_item_id").or_else(|| json_u64(json, "targetItemId"));

    if let Some(target) = target_id {
        if let Some(profile) = state.profiles.get_mut(&target) {
            profile.bounty_count = profile.bounty_count.saturating_sub(1);
            profile.dirty = true;
            profile.threat_score = threat_engine::compute_score(profile);
        }
    }

    state.push_event(
        RawEvent {
            event_type: "bounty_removed".into(),
            timestamp_ms,
            data: serde_json::json!({
                "target_item_id": target_id,
            }),
        },
        sse_tx,
        cap,
    );
}

/// Handle a JumpEvent: update the character's last-seen system and increment systems_visited
/// if this is a new system. Gate names are resolved from cache (pre-fetched before this call).
fn handle_jump(
    state: &mut crate::types::DataStore,
    sse_tx: &Option<tokio::sync::broadcast::Sender<String>>,
    json: &serde_json::Value,
    timestamp_ms: u64,
    cap: usize,
) {
    let character_id = json_item_id(json, "character_id")
        .or_else(|| json_item_id(json, "character_key"))
        .or_else(|| json_u64(json, "characterId"));
    let system = json_item_id_str(json, "solar_system_id")
        .or_else(|| json_str(json, "solarSystemId"))
        .unwrap_or_default();
    let system_for_event = system.clone();
    let system_name = resolve_system_name(state, &system);

    let source_gate_id = json["source_gate_id"]
        .as_str()
        .unwrap_or_default()
        .to_string();
    let dest_gate_id = json["destination_gate_id"]
        .as_str()
        .unwrap_or_default()
        .to_string();

    // Resolve gate names from cache if available
    let source_gate = state
        .gate_name_cache
        .get(&source_gate_id)
        .cloned()
        .unwrap_or_else(|| source_gate_id.clone());
    let dest_gate = state
        .gate_name_cache
        .get(&dest_gate_id)
        .cloned()
        .unwrap_or_else(|| dest_gate_id.clone());

    if let Some(char_id) = character_id {
        let name = resolve_name(state, char_id);
        let profile = state
            .profiles
            .entry(char_id)
            .or_insert_with(|| ThreatProfile {
                character_item_id: char_id,
                name,
                ..Default::default()
            });
        if profile.last_seen_system != system {
            profile.systems_visited += 1;
            profile.last_seen_system_name = system_name;
            profile.last_seen_system = system;
        }
        profile.dirty = true;
        profile.threat_score = threat_engine::compute_score(profile);
    }

    state.push_event(
        RawEvent {
            event_type: "jump".into(),
            timestamp_ms,
            data: serde_json::json!({
                "character_id": character_id,
                "solar_system_id": system_for_event,
                "source_gate": source_gate,
                "dest_gate": dest_gate,
                "source_gate_id": source_gate_id,
                "dest_gate_id": dest_gate_id,
            }),
        },
        sse_tx,
        cap,
    );
}

/// Handle a CharacterCreatedEvent: insert a blank profile for the new character if one
/// doesn't already exist, and push a `new_character` event.
fn handle_character_created(
    state: &mut crate::types::DataStore,
    sse_tx: &Option<tokio::sync::broadcast::Sender<String>>,
    json: &serde_json::Value,
    timestamp_ms: u64,
    cap: usize,
) {
    let char_id = json_item_id(json, "key")
        .or_else(|| json_item_id(json, "character_id"))
        .or_else(|| json_u64(json, "characterId"));

    if let Some(id) = char_id {
        state.profiles.entry(id).or_insert_with(|| ThreatProfile {
            character_item_id: id,
            name: None,
            ..Default::default()
        });

        state.push_event(
            RawEvent {
                event_type: "new_character".into(),
                timestamp_ms,
                data: serde_json::json!({ "character_id": id }),
            },
            sse_tx,
            cap,
        );
    }
}

/// Look up a cached name, or return None if not yet resolved.
fn resolve_name(state: &crate::types::DataStore, character_item_id: u64) -> Option<String> {
    state.name_cache.get(&character_item_id).cloned()
}

/// Look up a cached system name from the inline DataStore cache.
fn resolve_system_name(state: &crate::types::DataStore, system_id: &str) -> String {
    state
        .system_name_cache
        .get(system_id)
        .cloned()
        .unwrap_or_default()
}

// === JSON helpers ===

/// Extract a u64 from a JSON field, accepting both number and numeric-string values.
fn json_u64(v: &serde_json::Value, key: &str) -> Option<u64> {
    v.get(key).and_then(|v| {
        v.as_u64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    })
}

/// Extract a u64 from a nested EVE Frontier key object: {"item_id": "123", "tenant": "..."}
fn json_item_id(v: &serde_json::Value, key: &str) -> Option<u64> {
    v.get(key).and_then(|val| {
        // Nested: {"item_id": "123"}
        val.get("item_id")
            .and_then(|id| {
                id.as_str()
                    .and_then(|s| s.parse().ok())
                    .or_else(|| id.as_u64())
            })
            // Fallback: plain value
            .or_else(|| val.as_u64())
            .or_else(|| val.as_str().and_then(|s| s.parse().ok()))
    })
}

/// Extract a string item_id from a nested key object.
fn json_item_id_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|val| {
        // Nested: {"item_id": "30016543"}
        val.get("item_id")
            .and_then(|id| id.as_str().map(|s| s.to_string()))
            // Fallback: plain string
            .or_else(|| val.as_str().map(|s| s.to_string()))
    })
}

/// Extract a string value from a JSON field.
fn json_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// Convert protobuf `Value` to serde_json `Value`.
pub fn proto_value_to_json(v: &prost_types::Value) -> serde_json::Value {
    use prost_types::value::Kind;
    match &v.kind {
        Some(Kind::NullValue(_)) => serde_json::Value::Null,
        Some(Kind::NumberValue(n)) => serde_json::json!(*n),
        Some(Kind::StringValue(s)) => serde_json::json!(s),
        Some(Kind::BoolValue(b)) => serde_json::json!(*b),
        Some(Kind::StructValue(s)) => {
            let mut map = serde_json::Map::new();
            for (k, v) in &s.fields {
                map.insert(k.clone(), proto_value_to_json(v));
            }
            serde_json::Value::Object(map)
        }
        Some(Kind::ListValue(l)) => {
            serde_json::Value::Array(l.values.iter().map(proto_value_to_json).collect())
        }
        None => serde_json::Value::Null,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::DataStore;

    fn empty_store() -> DataStore {
        DataStore::default()
    }

    fn no_sse() -> Option<tokio::sync::broadcast::Sender<String>> {
        None
    }

    // === Killmail handler ===

    #[test]
    fn killmail_creates_killer_and_victim_profiles() {
        let mut store = empty_store();
        let json = serde_json::json!({
            "killer_character_id": 100,
            "victim_character_id": 200,
            "solar_system_id": "J-1042"
        });

        handle_killmail(&mut store, &no_sse(), &json, 1000, 1000);

        assert_eq!(store.profiles.len(), 2);

        let killer = store.profiles.get(&100).unwrap();
        assert_eq!(killer.kill_count, 1);
        assert_eq!(killer.recent_kills_24h, 1);
        assert_eq!(killer.last_kill_timestamp, 1000);
        assert_eq!(killer.last_seen_system, "J-1042");
        assert!(killer.dirty);
        assert!(killer.threat_score > 0);

        let victim = store.profiles.get(&200).unwrap();
        assert_eq!(victim.death_count, 1);
        assert_eq!(victim.kill_count, 0);
        assert_eq!(victim.last_seen_system, "J-1042");
        assert!(victim.dirty);
    }

    #[test]
    fn killmail_increments_existing_profile() {
        let mut store = empty_store();
        store.profiles.insert(
            100,
            ThreatProfile {
                character_item_id: 100,
                kill_count: 5,
                recent_kills_24h: 2,
                ..Default::default()
            },
        );

        let json = serde_json::json!({
            "killer_character_id": 100,
            "victim_character_id": 200,
            "solar_system_id": "X-4419"
        });
        handle_killmail(&mut store, &no_sse(), &json, 2000, 1000);

        let killer = store.profiles.get(&100).unwrap();
        assert_eq!(killer.kill_count, 6);
        assert_eq!(killer.recent_kills_24h, 3);
    }

    #[test]
    fn killmail_pushes_event() {
        let mut store = empty_store();
        let json = serde_json::json!({
            "killer_character_id": 100,
            "victim_character_id": 200,
        });
        handle_killmail(&mut store, &no_sse(), &json, 1000, 1000);

        assert_eq!(store.recent_events.len(), 1);
        assert_eq!(store.recent_events[0].event_type, "kill");
    }

    #[test]
    fn killmail_handles_alternate_field_names() {
        let mut store = empty_store();
        let json = serde_json::json!({
            "killerId": 300,
            "victimId": 400,
            "solarSystemId": "Z-0091"
        });
        handle_killmail(&mut store, &no_sse(), &json, 1000, 1000);

        assert!(store.profiles.contains_key(&300));
        assert!(store.profiles.contains_key(&400));
        assert_eq!(store.profiles.get(&300).unwrap().last_seen_system, "Z-0091");
    }

    // === Bounty posted handler ===

    #[test]
    fn bounty_posted_increments_bounty_count() {
        let mut store = empty_store();
        let json = serde_json::json!({ "target_item_id": 100 });

        handle_bounty_posted(&mut store, &no_sse(), &json, 1000, 1000);

        let profile = store.profiles.get(&100).unwrap();
        assert_eq!(profile.bounty_count, 1);
        assert!(profile.dirty);
        assert_eq!(store.recent_events[0].event_type, "bounty_posted");
    }

    #[test]
    fn bounty_posted_stacks() {
        let mut store = empty_store();
        let json = serde_json::json!({ "target_item_id": 100 });

        handle_bounty_posted(&mut store, &no_sse(), &json, 1000, 1000);
        handle_bounty_posted(&mut store, &no_sse(), &json, 2000, 1000);

        assert_eq!(store.profiles.get(&100).unwrap().bounty_count, 2);
    }

    // === Bounty removed handler ===

    #[test]
    fn bounty_removed_decrements_count() {
        let mut store = empty_store();
        store.profiles.insert(
            100,
            ThreatProfile {
                character_item_id: 100,
                bounty_count: 3,
                ..Default::default()
            },
        );

        let json = serde_json::json!({ "target_item_id": 100 });
        handle_bounty_removed(&mut store, &no_sse(), &json, 1000, 1000);

        assert_eq!(store.profiles.get(&100).unwrap().bounty_count, 2);
        assert_eq!(store.recent_events[0].event_type, "bounty_removed");
    }

    #[test]
    fn bounty_removed_saturates_at_zero() {
        let mut store = empty_store();
        store.profiles.insert(
            100,
            ThreatProfile {
                character_item_id: 100,
                bounty_count: 0,
                ..Default::default()
            },
        );

        let json = serde_json::json!({ "target_item_id": 100 });
        handle_bounty_removed(&mut store, &no_sse(), &json, 1000, 1000);

        assert_eq!(store.profiles.get(&100).unwrap().bounty_count, 0);
    }

    #[test]
    fn bounty_removed_ignores_unknown_character() {
        let mut store = empty_store();
        let json = serde_json::json!({ "target_item_id": 999 });
        handle_bounty_removed(&mut store, &no_sse(), &json, 1000, 1000);

        // No profile created — only existing profiles are modified
        assert!(!store.profiles.contains_key(&999));
        // Event is still pushed
        assert_eq!(store.recent_events.len(), 1);
    }

    // === Jump handler ===

    #[test]
    fn jump_creates_profile_and_tracks_system() {
        let mut store = empty_store();
        let json = serde_json::json!({
            "character_id": 100,
            "solar_system_id": "K-9731"
        });

        handle_jump(&mut store, &no_sse(), &json, 1000, 1000);

        let profile = store.profiles.get(&100).unwrap();
        assert_eq!(profile.last_seen_system, "K-9731");
        assert_eq!(profile.systems_visited, 1);
        assert!(profile.dirty);
        assert_eq!(store.recent_events[0].event_type, "jump");
    }

    #[test]
    fn jump_same_system_does_not_increment_visited() {
        let mut store = empty_store();
        store.profiles.insert(
            100,
            ThreatProfile {
                character_item_id: 100,
                last_seen_system: "K-9731".into(),
                systems_visited: 3,
                ..Default::default()
            },
        );

        let json = serde_json::json!({
            "character_id": 100,
            "solar_system_id": "K-9731"
        });
        handle_jump(&mut store, &no_sse(), &json, 1000, 1000);

        assert_eq!(store.profiles.get(&100).unwrap().systems_visited, 3);
    }

    #[test]
    fn jump_new_system_increments_visited() {
        let mut store = empty_store();
        store.profiles.insert(
            100,
            ThreatProfile {
                character_item_id: 100,
                last_seen_system: "K-9731".into(),
                systems_visited: 3,
                ..Default::default()
            },
        );

        let json = serde_json::json!({
            "character_id": 100,
            "solar_system_id": "X-4419"
        });
        handle_jump(&mut store, &no_sse(), &json, 1000, 1000);

        let profile = store.profiles.get(&100).unwrap();
        assert_eq!(profile.systems_visited, 4);
        assert_eq!(profile.last_seen_system, "X-4419");
    }

    // === JSON helpers ===

    #[test]
    fn json_u64_parses_number() {
        let v = serde_json::json!({"id": 42});
        assert_eq!(json_u64(&v, "id"), Some(42));
    }

    #[test]
    fn json_u64_parses_string_number() {
        let v = serde_json::json!({"id": "42"});
        assert_eq!(json_u64(&v, "id"), Some(42));
    }

    #[test]
    fn json_u64_returns_none_for_missing() {
        let v = serde_json::json!({"other": 1});
        assert_eq!(json_u64(&v, "id"), None);
    }

    // === Name resolution ===

    #[test]
    fn resolve_name_uses_cache() {
        let mut store = empty_store();
        store.name_cache.insert(42, "Vex Nightburn".into());
        assert_eq!(resolve_name(&store, 42), Some("Vex Nightburn".to_string()));
    }

    #[test]
    fn resolve_name_falls_back() {
        let store = empty_store();
        assert_eq!(resolve_name(&store, 42), None);
    }
}
