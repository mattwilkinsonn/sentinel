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

use sui_rpc::subscription_service_client::SubscriptionServiceClient;
use sui_rpc::SubscribeCheckpointsRequest;

/// Connect to Sui fullnode and stream checkpoints forever (with reconnect).
pub async fn stream_checkpoints(config: AppConfig, state: Arc<RwLock<AppState>>) {
    loop {
        tracing::info!("Connecting to gRPC stream at {}", config.sui_grpc_url);
        match run_stream(&config, &state).await {
            Ok(()) => tracing::warn!("gRPC stream ended cleanly, reconnecting..."),
            Err(e) => tracing::error!("gRPC stream error: {e}, reconnecting in 2s..."),
        }
        tokio::time::sleep(std::time::Duration::from_secs(2)).await;
    }
}

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
            paths: vec![
                "checkpoint.transactions.events".into(),
                "checkpoint.transactions.effects".into(),
            ],
        }),
    };

    let mut stream = client
        .subscribe_checkpoints(request)
        .await?
        .into_inner();

    tracing::info!("gRPC checkpoint stream connected");

    while let Some(response) = stream.message().await? {
        let cursor = response.cursor.unwrap_or(0);

        if let Some(checkpoint) = response.checkpoint {
            process_checkpoint(config, state, &checkpoint, cursor).await;
        }

        // Update cursor
        state.write().await.last_checkpoint = Some(cursor);
    }

    Ok(())
}

async fn process_checkpoint(
    config: &AppConfig,
    state: &Arc<RwLock<AppState>>,
    checkpoint: &sui_rpc::Checkpoint,
    cursor: u64,
) {
    let timestamp_ms: u64 = checkpoint
        .summary
        .as_ref()
        .and_then(|s| s.timestamp.as_ref())
        .map(|ts| (ts.seconds as u64) * 1000 + (ts.nanos as u64) / 1_000_000)
        .unwrap_or(0);

    for tx in &checkpoint.transactions {
        let events = tx
            .events
            .as_ref()
            .map(|e| &e.events[..])
            .unwrap_or_default();

        for event in events {
            let event_type = event.event_type.as_deref().unwrap_or("");
            let package_id = event.package_id.as_deref().unwrap_or("");

            // Filter for events from world or bounty_board packages
            if package_id != config.world_package_id
                && package_id != config.bounty_board_package_id
                && package_id != config.sentinel_package_id
            {
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

            if event_type.contains("KillMailCreatedEvent") || event_type.contains("KillmailCreatedEvent") {
                handle_killmail(store, &sse_tx, &json_value, timestamp_ms);
            } else if event_type.contains("BountyPostedEvent") {
                handle_bounty_posted(store, &sse_tx, &json_value, timestamp_ms);
            } else if event_type.contains("BountyCancelledEvent")
                || event_type.contains("ContributionWithdrawnEvent")
            {
                handle_bounty_removed(store, &sse_tx, &json_value, timestamp_ms);
            } else if event_type.contains("JumpEvent") {
                handle_jump(store, &sse_tx, &json_value, timestamp_ms);
            }

            // Log
            tracing::debug!(
                "checkpoint={cursor} event={event_type} pkg={package_id}"
            );
        }
    }
}

fn handle_killmail(state: &mut crate::types::DataStore, sse_tx: &Option<tokio::sync::broadcast::Sender<String>>, json: &serde_json::Value, timestamp_ms: u64) {
    // Extract killer and victim character IDs from the killmail event
    let killer_id = json_u64(json, "killer_character_id")
        .or_else(|| json_u64(json, "killerId"));
    let victim_id = json_u64(json, "victim_character_id")
        .or_else(|| json_u64(json, "victimId"));
    let system = json_str(json, "solar_system_id")
        .or_else(|| json_str(json, "solarSystemId"))
        .unwrap_or_default();

    if let Some(killer) = killer_id {
        let name = resolve_name(state, killer);
        let profile = state.profiles.entry(killer).or_insert_with(|| ThreatProfile {
            character_item_id: killer,
            name,
            ..Default::default()
        });
        profile.kill_count += 1;
        profile.recent_kills_24h += 1;
        profile.last_kill_timestamp = timestamp_ms;
        profile.last_seen_system = system.clone();
        profile.dirty = true;
        profile.threat_score = threat_engine::compute_score(profile);
    }

    if let Some(victim) = victim_id {
        let name = resolve_name(state, victim);
        let profile = state.profiles.entry(victim).or_insert_with(|| ThreatProfile {
            character_item_id: victim,
            name,
            ..Default::default()
        });
        profile.death_count += 1;
        profile.last_seen_system = system.clone();
        profile.dirty = true;
        profile.threat_score = threat_engine::compute_score(profile);
    }

    state.push_event(RawEvent {
        event_type: "kill".into(),
        timestamp_ms,
        data: json.clone(),
    }, sse_tx);
}

fn handle_bounty_posted(state: &mut crate::types::DataStore, sse_tx: &Option<tokio::sync::broadcast::Sender<String>>, json: &serde_json::Value, timestamp_ms: u64) {
    let target_id = json_u64(json, "target_item_id")
        .or_else(|| json_u64(json, "targetItemId"));

    if let Some(target) = target_id {
        let name = resolve_name(state, target);
        let profile = state.profiles.entry(target).or_insert_with(|| ThreatProfile {
            character_item_id: target,
            name,
            ..Default::default()
        });
        profile.bounty_count += 1;
        profile.dirty = true;
        profile.threat_score = threat_engine::compute_score(profile);
    }

    state.push_event(RawEvent {
        event_type: "bounty_posted".into(),
        timestamp_ms,
        data: json.clone(),
    }, sse_tx);
}

fn handle_bounty_removed(state: &mut crate::types::DataStore, sse_tx: &Option<tokio::sync::broadcast::Sender<String>>, json: &serde_json::Value, timestamp_ms: u64) {
    let target_id = json_u64(json, "target_item_id")
        .or_else(|| json_u64(json, "targetItemId"));

    if let Some(target) = target_id {
        if let Some(profile) = state.profiles.get_mut(&target) {
            profile.bounty_count = profile.bounty_count.saturating_sub(1);
            profile.dirty = true;
            profile.threat_score = threat_engine::compute_score(profile);
        }
    }

    state.push_event(RawEvent {
        event_type: "bounty_removed".into(),
        timestamp_ms,
        data: json.clone(),
    }, sse_tx);
}

fn handle_jump(state: &mut crate::types::DataStore, sse_tx: &Option<tokio::sync::broadcast::Sender<String>>, json: &serde_json::Value, timestamp_ms: u64) {
    let character_id = json_u64(json, "character_id")
        .or_else(|| json_u64(json, "characterId"));
    let system = json_str(json, "solar_system_id")
        .or_else(|| json_str(json, "solarSystemId"))
        .unwrap_or_default();

    if let Some(char_id) = character_id {
        let name = resolve_name(state, char_id);
        let profile = state.profiles.entry(char_id).or_insert_with(|| ThreatProfile {
            character_item_id: char_id,
            name,
            ..Default::default()
        });
        if profile.last_seen_system != system {
            profile.systems_visited += 1;
            profile.last_seen_system = system;
        }
        profile.dirty = true;
        profile.threat_score = threat_engine::compute_score(profile);
    }

    state.push_event(RawEvent {
        event_type: "jump".into(),
        timestamp_ms,
        data: json.clone(),
    }, sse_tx);
}

/// Look up a cached name, or return a fallback.
fn resolve_name(state: &crate::types::DataStore, character_item_id: u64) -> String {
    state.name_cache.get(&character_item_id).cloned()
        .unwrap_or_else(|| format!("Pilot #{character_item_id}"))
}

// === JSON helpers ===

fn json_u64(v: &serde_json::Value, key: &str) -> Option<u64> {
    v.get(key).and_then(|v| {
        v.as_u64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    })
}

fn json_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|v| v.as_str()).map(|s| s.to_string())
}

/// Convert protobuf `Value` to serde_json `Value`.
fn proto_value_to_json(v: &prost_types::Value) -> serde_json::Value {
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
