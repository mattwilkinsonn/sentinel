//! Sui Bridge — standalone binary that streams Sui checkpoints via gRPC,
//! parses game events (kills, bounties, jumps, character creation), serialises
//! them to the sentinel event format, and publishes to NATS `sentinel.events`
//! JetStream. Also handles `sentinel.name-requests` workqueue: resolves
//! character names via Sui gRPC object lookups and writes to `sentinel.names` KV.

use std::collections::HashMap;
use std::time::Duration;

use async_nats::jetstream;
use serde::{Deserialize, Serialize};
use tonic::transport::Channel;

mod google_rpc {
    include!(concat!(env!("OUT_DIR"), "/google.rpc.rs"));
}

pub mod sui_rpc {
    tonic::include_proto!("sui.rpc.v2");
}

use sui_rpc::subscription_service_client::SubscriptionServiceClient;
use sui_rpc::SubscribeCheckpointsRequest;

// ─── Configuration ──────────────────────────────────────────────────────────

#[derive(Clone)]
struct Config {
    nats_url: String,
    sui_grpc_url: String,
    world_package_id: String,
    bounty_board_package_id: String,
    sentinel_package_id: String,
}

fn require(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

impl Config {
    fn from_env() -> Self {
        Self {
            nats_url: require("NATS_URL"),
            sui_grpc_url: require("SUI_GRPC_URL"),
            world_package_id: require("WORLD_PACKAGE_ID"),
            bounty_board_package_id: require("BOUNTY_BOARD_PACKAGE_ID"),
            sentinel_package_id: require("SENTINEL_PACKAGE_ID"),
        }
    }
}

// ─── Event types (published to sentinel.events) ─────────────────────────────

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ThreatEvent {
    Kill(KillEvent),
    Bounty(BountyEvent),
    Gate(GateEvent),
}

#[derive(Debug, Serialize)]
struct KillEvent {
    victim_id: u64,
    attacker_ids: Vec<u64>,
    system_id: String,
    timestamp: u64,
    ship_type_id: u64,
}

#[derive(Debug, Serialize)]
struct BountyEvent {
    target_id: u64,
    poster_id: u64,
    amount_mist: u64,
    timestamp: u64,
}

#[derive(Debug, Serialize)]
struct GateEvent {
    pilot_id: u64,
    gate_object_id: String,
    permitted: bool,
    timestamp: u64,
}

// ─── Name request from name-resolver ────────────────────────────────────────

#[derive(Debug, Deserialize)]
struct NameRequest {
    pilot_id: u64,
}

// ─── Main ───────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sui_bridge=info".into()),
        )
        .init();

    let config = Config::from_env();
    tracing::info!(
        sui_grpc = %config.sui_grpc_url,
        nats = %config.nats_url,
        "Starting sui-bridge"
    );

    // Connect to NATS
    let nats = async_nats::connect(&config.nats_url).await?;
    let js = jetstream::new(nats.clone());

    // Ensure sentinel.events stream exists
    ensure_events_stream(&js).await?;

    // Ensure sentinel.names KV bucket exists
    let names_kv = js
        .create_key_value(jetstream::kv::Config {
            bucket: "sentinel-names".into(),
            history: 1,
            ..Default::default()
        })
        .await?;

    // Spawn name-request handler
    let config_clone = config.clone();
    let names_kv_clone = names_kv.clone();
    tokio::spawn(async move {
        if let Err(e) = handle_name_requests(&config_clone, &js, &names_kv_clone).await {
            tracing::error!(error = %e, "name-request handler failed: {e}");
        }
    });

    // Stream checkpoints forever (with reconnect)
    let js2 = jetstream::new(nats);
    stream_checkpoints(&config, &js2).await;

    Ok(())
}

// ─── JetStream setup ────────────────────────────────────────────────────────

async fn ensure_events_stream(js: &jetstream::Context) -> anyhow::Result<()> {
    js.get_or_create_stream(jetstream::stream::Config {
        name: "SENTINEL_EVENTS".into(),
        subjects: vec!["sentinel.events".into()],
        retention: jetstream::stream::RetentionPolicy::Limits,
        max_messages: 5000,
        ..Default::default()
    })
    .await?;
    tracing::info!("SENTINEL_EVENTS stream ready");
    Ok(())
}

// ─── Checkpoint streaming ───────────────────────────────────────────────────

async fn stream_checkpoints(config: &Config, js: &jetstream::Context) {
    loop {
        tracing::info!(url = %config.sui_grpc_url, "Connecting to Sui gRPC...");
        match run_stream(config, js).await {
            Ok(()) => tracing::warn!("gRPC stream ended cleanly, reconnecting..."),
            Err(e) => tracing::error!(error = %e, "gRPC stream error: {e}, reconnecting in 2s..."),
        }
        tokio::time::sleep(Duration::from_secs(2)).await;
    }
}

async fn run_stream(
    config: &Config,
    js: &jetstream::Context,
) -> anyhow::Result<()> {
    let channel = connect_sui(&config.sui_grpc_url).await?;
    let mut client = SubscriptionServiceClient::new(channel);

    let request = SubscribeCheckpointsRequest {
        read_mask: Some(prost_types::FieldMask {
            paths: vec!["checkpoint.transactions".into()],
        }),
    };

    let mut stream = client.subscribe_checkpoints(request).await?.into_inner();
    tracing::info!("gRPC checkpoint stream connected");

    let mut checkpoint_count: u64 = 0;
    while let Some(response) = stream.message().await? {
        let cursor = response.cursor.unwrap_or(0);
        checkpoint_count += 1;

        if let Some(checkpoint) = response.checkpoint {
            let timestamp_ms = checkpoint_timestamp_ms(&checkpoint);
            process_checkpoint(config, js, &checkpoint, timestamp_ms).await?;
        }

        if checkpoint_count % 100 == 0 {
            tracing::debug!(checkpoint_count, cursor, "Processed {checkpoint_count} checkpoints, cursor={cursor}");
        }
    }

    Ok(())
}

async fn connect_sui(url: &str) -> anyhow::Result<Channel> {
    let channel = Channel::from_shared(url.to_string())?
        .tls_config(tonic::transport::ClientTlsConfig::new().with_webpki_roots())?
        .connect()
        .await?;
    Ok(channel)
}

fn checkpoint_timestamp_ms(checkpoint: &sui_rpc::Checkpoint) -> u64 {
    checkpoint
        .summary
        .as_ref()
        .and_then(|s| s.timestamp.as_ref())
        .map(|ts| (ts.seconds as u64) * 1000 + (ts.nanos as u64) / 1_000_000)
        .unwrap_or(0)
}

// ─── Event extraction and publishing ────────────────────────────────────────

async fn process_checkpoint(
    config: &Config,
    js: &jetstream::Context,
    checkpoint: &sui_rpc::Checkpoint,
    timestamp_ms: u64,
) -> anyhow::Result<()> {
    for tx in &checkpoint.transactions {
        let events = tx
            .events
            .as_ref()
            .map(|e| &e.events[..])
            .unwrap_or_default();

        for event in events {
            let event_type = event.event_type.as_deref().unwrap_or("");
            let package_id = event.package_id.as_deref().unwrap_or("");

            // Filter for our game packages
            if package_id != config.world_package_id
                && package_id != config.bounty_board_package_id
                && package_id != config.sentinel_package_id
            {
                continue;
            }

            let json_value = event
                .json
                .as_ref()
                .map(proto_value_to_json)
                .unwrap_or(serde_json::Value::Null);

            let threat_event = if event_type.contains("KillMailCreatedEvent")
                || event_type.contains("KillmailCreatedEvent")
            {
                parse_kill_event(&json_value, timestamp_ms)
            } else if event_type.contains("BountyPostedEvent") {
                parse_bounty_event(&json_value, timestamp_ms)
            } else if event_type.contains("JumpEvent") {
                parse_gate_event(&json_value, timestamp_ms)
            } else {
                None
            };

            if let Some(te) = threat_event {
                let payload = serde_json::to_vec(&te)?;
                js.publish("sentinel.events", payload.into()).await?;
                tracing::debug!(event_type, "Published event to sentinel.events");
            }
        }
    }
    Ok(())
}

fn parse_kill_event(json: &serde_json::Value, timestamp_ms: u64) -> Option<ThreatEvent> {
    let killer_id = json_item_id(json, "killer_character_id")
        .or_else(|| json_item_id(json, "killer_id"))
        .or_else(|| json_u64(json, "killerId"))?;
    let victim_id = json_item_id(json, "victim_character_id")
        .or_else(|| json_item_id(json, "victim_id"))
        .or_else(|| json_u64(json, "victimId"))?;
    let system_id = json_item_id_str(json, "solar_system_id")
        .or_else(|| json_str(json, "solarSystemId"))
        .unwrap_or_default();

    Some(ThreatEvent::Kill(KillEvent {
        victim_id,
        attacker_ids: vec![killer_id],
        system_id,
        timestamp: timestamp_ms,
        ship_type_id: json_u64(json, "ship_type_id").unwrap_or(0),
    }))
}

fn parse_bounty_event(json: &serde_json::Value, timestamp_ms: u64) -> Option<ThreatEvent> {
    let target_id = json_item_id(json, "target_item_id")
        .or_else(|| json_u64(json, "targetItemId"))?;
    let poster_id = json_item_id(json, "poster_id").unwrap_or(0);
    let amount_mist = json_u64(json, "amount").unwrap_or(0);

    Some(ThreatEvent::Bounty(BountyEvent {
        target_id,
        poster_id,
        amount_mist,
        timestamp: timestamp_ms,
    }))
}

fn parse_gate_event(json: &serde_json::Value, timestamp_ms: u64) -> Option<ThreatEvent> {
    let pilot_id = json_item_id(json, "character_id")
        .or_else(|| json_item_id(json, "character_key"))
        .or_else(|| json_u64(json, "characterId"))?;
    let gate_object_id = json_str(json, "destination_gate_id").unwrap_or_default();

    Some(ThreatEvent::Gate(GateEvent {
        pilot_id,
        gate_object_id,
        permitted: true,
        timestamp: timestamp_ms,
    }))
}

// ─── Name request handler ───────────────────────────────────────────────────

async fn handle_name_requests(
    config: &Config,
    js: &jetstream::Context,
    names_kv: &jetstream::kv::Store,
) -> anyhow::Result<()> {
    // Create a durable consumer on sentinel.name-requests
    let stream = js
        .get_or_create_stream(jetstream::stream::Config {
            name: "SENTINEL_NAME_REQUESTS".into(),
            subjects: vec!["sentinel.name-requests".into()],
            retention: jetstream::stream::RetentionPolicy::WorkQueue,
            ..Default::default()
        })
        .await?;

    let consumer = stream
        .get_or_create_consumer(
            "name-resolver",
            jetstream::consumer::pull::Config {
                durable_name: Some("name-resolver".into()),
                ack_policy: jetstream::consumer::AckPolicy::Explicit,
                ..Default::default()
            },
        )
        .await?;

    tracing::info!("Listening for name requests on sentinel.name-requests");

    // Cache to avoid repeated lookups
    let mut name_cache: HashMap<u64, String> = HashMap::new();

    loop {
        let mut messages = consumer.fetch().max_messages(10).messages().await?;

        use futures::StreamExt;
        while let Some(Ok(msg)) = messages.next().await {
            match serde_json::from_slice::<NameRequest>(&msg.payload) {
                Ok(req) => {
                    if let Some(cached) = name_cache.get(&req.pilot_id) {
                        // Already resolved, write to KV and ack
                        let _ = names_kv
                            .put(
                                format!("pilot:{}", req.pilot_id),
                                bytes::Bytes::from(cached.clone()),
                            )
                            .await;
                        let _ = msg.ack().await;
                        continue;
                    }

                    // Resolve via Sui gRPC object lookup
                    match resolve_name_grpc(config, req.pilot_id).await {
                        Ok(name) => {
                            tracing::info!(pilot_id = req.pilot_id, name = %name, "Resolved name");
                            name_cache.insert(req.pilot_id, name.clone());
                            let _ = names_kv
                                .put(
                                    format!("pilot:{}", req.pilot_id),
                                    bytes::Bytes::from(name.clone()),
                                )
                                .await;
                            let _ = msg.ack().await;
                        }
                        Err(e) => {
                            tracing::warn!(
                                pilot_id = req.pilot_id,
                                error = %e,
                                "Failed to resolve name, will retry"
                            );
                            // Don't ack — message will be redelivered
                        }
                    }
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Invalid name request payload");
                    let _ = msg.ack().await; // ack bad messages to avoid infinite retry
                }
            }
        }

        // Brief pause between fetch batches
        tokio::time::sleep(Duration::from_secs(1)).await;
    }
}

/// Resolve a character name by looking up the object on the Sui network.
/// Uses LedgerService.GetObject with JSON read_mask to get the character's name field.
async fn resolve_name_grpc(config: &Config, pilot_id: u64) -> anyhow::Result<String> {
    let channel = connect_sui(&config.sui_grpc_url).await?;
    let mut client = sui_rpc::ledger_service_client::LedgerServiceClient::new(channel);

    let request = sui_rpc::GetObjectRequest {
        object_id: Some(format!("0x{:064x}", pilot_id)),
        version: None,
        read_mask: Some(prost_types::FieldMask {
            paths: vec!["json".into()],
        }),
    };

    let response = client.get_object(request).await?.into_inner();

    if let Some(object) = response.object {
        if let Some(json) = object.json {
            let parsed = proto_value_to_json(&json);
            // Character objects have metadata.name or a top-level name field
            let name = parsed
                .get("metadata")
                .and_then(|m| m.get("name"))
                .and_then(|n| n.as_str())
                .or_else(|| parsed.get("name").and_then(|n| n.as_str()));
            if let Some(n) = name {
                if !n.is_empty() {
                    return Ok(n.to_string());
                }
            }
        }
    }

    // Fallback: use the numeric ID as a string
    Ok(pilot_id.to_string())
}

// ─── JSON helpers (ported from sentinel-backend) ────────────────────────────

fn json_u64(v: &serde_json::Value, key: &str) -> Option<u64> {
    v.get(key).and_then(|v| {
        v.as_u64()
            .or_else(|| v.as_str().and_then(|s| s.parse().ok()))
    })
}

fn json_item_id(v: &serde_json::Value, key: &str) -> Option<u64> {
    v.get(key).and_then(|val| {
        val.get("item_id")
            .and_then(|id| {
                id.as_str()
                    .and_then(|s| s.parse().ok())
                    .or_else(|| id.as_u64())
            })
            .or_else(|| val.as_u64())
            .or_else(|| val.as_str().and_then(|s| s.parse().ok()))
    })
}

fn json_item_id_str(v: &serde_json::Value, key: &str) -> Option<String> {
    v.get(key).and_then(|val| {
        val.get("item_id")
            .and_then(|id| id.as_str().map(|s| s.to_string()))
            .or_else(|| val.as_str().map(|s| s.to_string()))
    })
}

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
    use prost_types::value::Kind;

    // ─── json_u64 ───────────────────────────────────────────────────────

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

    #[test]
    fn json_u64_returns_none_for_non_numeric_string() {
        let v = serde_json::json!({"id": "abc"});
        assert_eq!(json_u64(&v, "id"), None);
    }

    // ─── json_item_id ───────────────────────────────────────────────────

    #[test]
    fn json_item_id_nested_string() {
        let v = serde_json::json!({"killer_id": {"item_id": "12345", "tenant": "test"}});
        assert_eq!(json_item_id(&v, "killer_id"), Some(12345));
    }

    #[test]
    fn json_item_id_nested_number() {
        let v = serde_json::json!({"killer_id": {"item_id": 12345}});
        assert_eq!(json_item_id(&v, "killer_id"), Some(12345));
    }

    #[test]
    fn json_item_id_plain_number() {
        let v = serde_json::json!({"killer_id": 42});
        assert_eq!(json_item_id(&v, "killer_id"), Some(42));
    }

    #[test]
    fn json_item_id_plain_string() {
        let v = serde_json::json!({"killer_id": "42"});
        assert_eq!(json_item_id(&v, "killer_id"), Some(42));
    }

    #[test]
    fn json_item_id_missing() {
        let v = serde_json::json!({});
        assert_eq!(json_item_id(&v, "killer_id"), None);
    }

    // ─── json_item_id_str ───────────────────────────────────────────────

    #[test]
    fn json_item_id_str_nested() {
        let v = serde_json::json!({"solar_system_id": {"item_id": "J-1042"}});
        assert_eq!(json_item_id_str(&v, "solar_system_id"), Some("J-1042".into()));
    }

    #[test]
    fn json_item_id_str_plain() {
        let v = serde_json::json!({"solar_system_id": "J-1042"});
        assert_eq!(json_item_id_str(&v, "solar_system_id"), Some("J-1042".into()));
    }

    // ─── json_str ───────────────────────────────────────────────────────

    #[test]
    fn json_str_present() {
        let v = serde_json::json!({"name": "hello"});
        assert_eq!(json_str(&v, "name"), Some("hello".into()));
    }

    #[test]
    fn json_str_missing() {
        let v = serde_json::json!({});
        assert_eq!(json_str(&v, "name"), None);
    }

    // ─── parse_kill_event ───────────────────────────────────────────────

    #[test]
    fn parse_kill_with_nested_ids() {
        let json = serde_json::json!({
            "killer_character_id": {"item_id": "100"},
            "victim_character_id": {"item_id": "200"},
            "solar_system_id": {"item_id": "J-1042"},
            "ship_type_id": 42
        });
        let event = parse_kill_event(&json, 1000).unwrap();
        match event {
            ThreatEvent::Kill(k) => {
                assert_eq!(k.attacker_ids, vec![100]);
                assert_eq!(k.victim_id, 200);
                assert_eq!(k.system_id, "J-1042");
                assert_eq!(k.timestamp, 1000);
                assert_eq!(k.ship_type_id, 42);
            }
            _ => panic!("expected Kill"),
        }
    }

    #[test]
    fn parse_kill_with_plain_ids() {
        let json = serde_json::json!({
            "killerId": 100,
            "victimId": 200,
            "solarSystemId": "X-4419"
        });
        let event = parse_kill_event(&json, 2000).unwrap();
        match event {
            ThreatEvent::Kill(k) => {
                assert_eq!(k.attacker_ids, vec![100]);
                assert_eq!(k.victim_id, 200);
                assert_eq!(k.system_id, "X-4419");
            }
            _ => panic!("expected Kill"),
        }
    }

    #[test]
    fn parse_kill_returns_none_without_killer() {
        let json = serde_json::json!({"victim_character_id": 200});
        assert!(parse_kill_event(&json, 1000).is_none());
    }

    #[test]
    fn parse_kill_returns_none_without_victim() {
        let json = serde_json::json!({"killer_character_id": 100});
        assert!(parse_kill_event(&json, 1000).is_none());
    }

    // ─── parse_bounty_event ─────────────────────────────────────────────

    #[test]
    fn parse_bounty_with_nested_id() {
        let json = serde_json::json!({
            "target_item_id": {"item_id": "300"},
            "poster_id": {"item_id": "400"},
            "amount": 5000
        });
        let event = parse_bounty_event(&json, 1000).unwrap();
        match event {
            ThreatEvent::Bounty(b) => {
                assert_eq!(b.target_id, 300);
                assert_eq!(b.poster_id, 400);
                assert_eq!(b.amount_mist, 5000);
            }
            _ => panic!("expected Bounty"),
        }
    }

    #[test]
    fn parse_bounty_with_plain_id() {
        let json = serde_json::json!({"targetItemId": 300});
        let event = parse_bounty_event(&json, 1000).unwrap();
        match event {
            ThreatEvent::Bounty(b) => {
                assert_eq!(b.target_id, 300);
                assert_eq!(b.poster_id, 0); // fallback
                assert_eq!(b.amount_mist, 0); // fallback
            }
            _ => panic!("expected Bounty"),
        }
    }

    #[test]
    fn parse_bounty_returns_none_without_target() {
        let json = serde_json::json!({"poster_id": 400});
        assert!(parse_bounty_event(&json, 1000).is_none());
    }

    // ─── parse_gate_event ───────────────────────────────────────────────

    #[test]
    fn parse_gate_with_nested_id() {
        let json = serde_json::json!({
            "character_id": {"item_id": "500"},
            "destination_gate_id": "0xabc"
        });
        let event = parse_gate_event(&json, 1000).unwrap();
        match event {
            ThreatEvent::Gate(g) => {
                assert_eq!(g.pilot_id, 500);
                assert_eq!(g.gate_object_id, "0xabc");
                assert!(g.permitted);
            }
            _ => panic!("expected Gate"),
        }
    }

    #[test]
    fn parse_gate_with_plain_id() {
        let json = serde_json::json!({"characterId": 500});
        let event = parse_gate_event(&json, 1000).unwrap();
        match event {
            ThreatEvent::Gate(g) => {
                assert_eq!(g.pilot_id, 500);
                assert_eq!(g.gate_object_id, ""); // missing → default
            }
            _ => panic!("expected Gate"),
        }
    }

    #[test]
    fn parse_gate_returns_none_without_character() {
        let json = serde_json::json!({"destination_gate_id": "0xabc"});
        assert!(parse_gate_event(&json, 1000).is_none());
    }

    // ─── proto_value_to_json ────────────────────────────────────────────

    #[test]
    fn proto_null() {
        let v = prost_types::Value { kind: Some(Kind::NullValue(0)) };
        assert_eq!(proto_value_to_json(&v), serde_json::Value::Null);
    }

    #[test]
    fn proto_number() {
        let v = prost_types::Value { kind: Some(Kind::NumberValue(42.5)) };
        assert_eq!(proto_value_to_json(&v), serde_json::json!(42.5));
    }

    #[test]
    fn proto_string() {
        let v = prost_types::Value { kind: Some(Kind::StringValue("hello".into())) };
        assert_eq!(proto_value_to_json(&v), serde_json::json!("hello"));
    }

    #[test]
    fn proto_bool() {
        let v = prost_types::Value { kind: Some(Kind::BoolValue(true)) };
        assert_eq!(proto_value_to_json(&v), serde_json::json!(true));
    }

    #[test]
    fn proto_struct() {
        let mut fields = std::collections::BTreeMap::new();
        fields.insert(
            "name".to_string(),
            prost_types::Value { kind: Some(Kind::StringValue("test".into())) },
        );
        fields.insert(
            "count".to_string(),
            prost_types::Value { kind: Some(Kind::NumberValue(3.0)) },
        );
        let v = prost_types::Value {
            kind: Some(Kind::StructValue(prost_types::Struct { fields })),
        };
        let json = proto_value_to_json(&v);
        assert_eq!(json["name"], "test");
        assert_eq!(json["count"], 3.0);
    }

    #[test]
    fn proto_list() {
        let v = prost_types::Value {
            kind: Some(Kind::ListValue(prost_types::ListValue {
                values: vec![
                    prost_types::Value { kind: Some(Kind::NumberValue(1.0)) },
                    prost_types::Value { kind: Some(Kind::NumberValue(2.0)) },
                ],
            })),
        };
        let json = proto_value_to_json(&v);
        assert_eq!(json, serde_json::json!([1.0, 2.0]));
    }

    #[test]
    fn proto_none_kind() {
        let v = prost_types::Value { kind: None };
        assert_eq!(proto_value_to_json(&v), serde_json::Value::Null);
    }

    // ─── ThreatEvent serialization ──────────────────────────────────────

    #[test]
    fn kill_event_serializes_with_tag() {
        let event = ThreatEvent::Kill(KillEvent {
            victim_id: 100,
            attacker_ids: vec![200],
            system_id: "J-1042".into(),
            timestamp: 1000,
            ship_type_id: 42,
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "kill");
        assert_eq!(json["victim_id"], 100);
        assert_eq!(json["attacker_ids"][0], 200);
    }

    #[test]
    fn bounty_event_serializes_with_tag() {
        let event = ThreatEvent::Bounty(BountyEvent {
            target_id: 100,
            poster_id: 200,
            amount_mist: 5000,
            timestamp: 1000,
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "bounty");
        assert_eq!(json["target_id"], 100);
    }

    #[test]
    fn gate_event_serializes_with_tag() {
        let event = ThreatEvent::Gate(GateEvent {
            pilot_id: 100,
            gate_object_id: "0xabc".into(),
            permitted: true,
            timestamp: 1000,
        });
        let json = serde_json::to_value(&event).unwrap();
        assert_eq!(json["type"], "gate");
        assert_eq!(json["pilot_id"], 100);
    }
}
