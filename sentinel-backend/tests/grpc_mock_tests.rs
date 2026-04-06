//! Mock gRPC service tests for the Sui client operations.
//! Tests core gRPC functions against in-process mock servers.

use std::net::SocketAddr;
use tokio::net::TcpListener;
use tonic::transport::{Channel, Server};

use sentinel_backend::grpc::sui_rpc;
use sui_rpc::ledger_service_server::{LedgerService, LedgerServiceServer};
use sui_rpc::state_service_server::{StateService, StateServiceServer};

// === Mock Ledger Service ===

#[derive(Default)]
struct MockLedgerService {
    checkpoint_height: u64,
    objects: std::collections::HashMap<String, sui_rpc::Object>,
    checkpoints: std::collections::HashMap<u64, sui_rpc::Checkpoint>,
}

#[tonic::async_trait]
impl LedgerService for MockLedgerService {
    async fn get_service_info(
        &self,
        _request: tonic::Request<sui_rpc::GetServiceInfoRequest>,
    ) -> Result<tonic::Response<sui_rpc::GetServiceInfoResponse>, tonic::Status> {
        Ok(tonic::Response::new(sui_rpc::GetServiceInfoResponse {
            chain_id: Some("test-chain".into()),
            chain: Some("testnet".into()),
            epoch: Some(1),
            checkpoint_height: Some(self.checkpoint_height),
            timestamp: None,
            lowest_available_checkpoint: Some(0),
            lowest_available_checkpoint_objects: Some(0),
            server: Some("mock".into()),
        }))
    }

    async fn get_object(
        &self,
        request: tonic::Request<sui_rpc::GetObjectRequest>,
    ) -> Result<tonic::Response<sui_rpc::GetObjectResponse>, tonic::Status> {
        let req = request.into_inner();
        let id = req.object_id.unwrap_or_default();
        match self.objects.get(&id) {
            Some(obj) => Ok(tonic::Response::new(sui_rpc::GetObjectResponse {
                object: Some(obj.clone()),
            })),
            None => Err(tonic::Status::not_found(format!("object {id} not found"))),
        }
    }

    async fn batch_get_objects(
        &self,
        request: tonic::Request<sui_rpc::BatchGetObjectsRequest>,
    ) -> Result<tonic::Response<sui_rpc::BatchGetObjectsResponse>, tonic::Status> {
        let req = request.into_inner();
        let results: Vec<sui_rpc::GetObjectResult> = req
            .requests
            .iter()
            .map(|r| {
                let id = r.object_id.clone().unwrap_or_default();
                match self.objects.get(&id) {
                    Some(obj) => sui_rpc::GetObjectResult {
                        result: Some(sui_rpc::get_object_result::Result::Object(obj.clone())),
                    },
                    None => sui_rpc::GetObjectResult { result: None },
                }
            })
            .collect();

        Ok(tonic::Response::new(sui_rpc::BatchGetObjectsResponse {
            objects: results,
        }))
    }

    async fn get_transaction(
        &self,
        _request: tonic::Request<sui_rpc::GetTransactionRequest>,
    ) -> Result<tonic::Response<sui_rpc::GetTransactionResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("not needed for tests"))
    }

    async fn batch_get_transactions(
        &self,
        _request: tonic::Request<sui_rpc::BatchGetTransactionsRequest>,
    ) -> Result<tonic::Response<sui_rpc::BatchGetTransactionsResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("not needed for tests"))
    }

    async fn get_checkpoint(
        &self,
        request: tonic::Request<sui_rpc::GetCheckpointRequest>,
    ) -> Result<tonic::Response<sui_rpc::GetCheckpointResponse>, tonic::Status> {
        let req = request.into_inner();
        let seq = match req.checkpoint_id {
            Some(sui_rpc::get_checkpoint_request::CheckpointId::SequenceNumber(n)) => n,
            _ => return Err(tonic::Status::invalid_argument("need sequence_number")),
        };
        match self.checkpoints.get(&seq) {
            Some(cp) => Ok(tonic::Response::new(sui_rpc::GetCheckpointResponse {
                checkpoint: Some(cp.clone()),
            })),
            None => Err(tonic::Status::not_found(format!(
                "checkpoint {seq} not found"
            ))),
        }
    }

    async fn get_epoch(
        &self,
        _request: tonic::Request<sui_rpc::GetEpochRequest>,
    ) -> Result<tonic::Response<sui_rpc::GetEpochResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("not needed for tests"))
    }
}

// === Mock State Service ===

#[derive(Default)]
struct MockStateService {
    dynamic_fields: Vec<sui_rpc::DynamicField>,
    owned_objects: Vec<sui_rpc::Object>,
}

#[tonic::async_trait]
impl StateService for MockStateService {
    async fn list_dynamic_fields(
        &self,
        _request: tonic::Request<sui_rpc::ListDynamicFieldsRequest>,
    ) -> Result<tonic::Response<sui_rpc::ListDynamicFieldsResponse>, tonic::Status> {
        Ok(tonic::Response::new(sui_rpc::ListDynamicFieldsResponse {
            dynamic_fields: self.dynamic_fields.clone(),
            next_page_token: None,
        }))
    }

    async fn list_owned_objects(
        &self,
        _request: tonic::Request<sui_rpc::ListOwnedObjectsRequest>,
    ) -> Result<tonic::Response<sui_rpc::ListOwnedObjectsResponse>, tonic::Status> {
        Ok(tonic::Response::new(sui_rpc::ListOwnedObjectsResponse {
            objects: self.owned_objects.clone(),
            next_page_token: None,
        }))
    }

    async fn get_coin_info(
        &self,
        _request: tonic::Request<sui_rpc::GetCoinInfoRequest>,
    ) -> Result<tonic::Response<sui_rpc::GetCoinInfoResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("not needed"))
    }

    async fn get_balance(
        &self,
        _request: tonic::Request<sui_rpc::GetBalanceRequest>,
    ) -> Result<tonic::Response<sui_rpc::GetBalanceResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("not needed"))
    }

    async fn list_balances(
        &self,
        _request: tonic::Request<sui_rpc::ListBalancesRequest>,
    ) -> Result<tonic::Response<sui_rpc::ListBalancesResponse>, tonic::Status> {
        Err(tonic::Status::unimplemented("not needed"))
    }
}

// === Helpers ===

/// Start a mock gRPC server and return the channel to connect to it.
async fn start_mock_ledger(svc: MockLedgerService) -> Channel {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
        Server::builder()
            .add_service(LedgerServiceServer::new(svc))
            .serve_with_incoming(incoming)
            .await
            .unwrap();
    });

    // Give the server a moment to start
    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .unwrap()
}

async fn start_mock_state(svc: MockStateService) -> Channel {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr: SocketAddr = listener.local_addr().unwrap();

    tokio::spawn(async move {
        let incoming = tokio_stream::wrappers::TcpListenerStream::new(listener);
        Server::builder()
            .add_service(StateServiceServer::new(svc))
            .serve_with_incoming(incoming)
            .await
            .unwrap();
    });

    tokio::time::sleep(std::time::Duration::from_millis(50)).await;

    Channel::from_shared(format!("http://{addr}"))
        .unwrap()
        .connect()
        .await
        .unwrap()
}

fn make_json_proto_value(json: &serde_json::Value) -> prost_types::Value {
    json_to_proto(json)
}

fn json_to_proto(v: &serde_json::Value) -> prost_types::Value {
    use prost_types::value::Kind;
    let kind = match v {
        serde_json::Value::Null => Kind::NullValue(0),
        serde_json::Value::Bool(b) => Kind::BoolValue(*b),
        serde_json::Value::Number(n) => Kind::NumberValue(n.as_f64().unwrap_or(0.0)),
        serde_json::Value::String(s) => Kind::StringValue(s.clone()),
        serde_json::Value::Array(arr) => Kind::ListValue(prost_types::ListValue {
            values: arr.iter().map(json_to_proto).collect(),
        }),
        serde_json::Value::Object(map) => Kind::StructValue(prost_types::Struct {
            fields: map
                .iter()
                .map(|(k, v)| (k.clone(), json_to_proto(v)))
                .collect(),
        }),
    };
    prost_types::Value { kind: Some(kind) }
}

// === Tests: get_latest_checkpoint ===

#[tokio::test]
async fn get_latest_checkpoint_returns_height() {
    let svc = MockLedgerService {
        checkpoint_height: 42_000,
        ..Default::default()
    };
    let channel = start_mock_ledger(svc).await;
    let height = sentinel_backend::sui_client::get_latest_checkpoint(channel)
        .await
        .unwrap();
    assert_eq!(height, 42_000);
}

// === Tests: get_checkpoint ===

#[tokio::test]
async fn get_checkpoint_returns_checkpoint_data() {
    let mut checkpoints = std::collections::HashMap::new();
    checkpoints.insert(
        100,
        sui_rpc::Checkpoint {
            sequence_number: Some(100),
            digest: Some("test_digest".into()),
            summary: None,
            signature: None,
            contents: None,
            transactions: vec![],
            objects: None,
        },
    );

    let svc = MockLedgerService {
        checkpoint_height: 100,
        checkpoints,
        ..Default::default()
    };
    let channel = start_mock_ledger(svc).await;
    let mut client = sui_rpc::ledger_service_client::LedgerServiceClient::new(channel);
    let resp = sentinel_backend::sui_client::get_checkpoint(&mut client, 100)
        .await
        .unwrap();
    let cp = resp.checkpoint.unwrap();
    assert_eq!(cp.sequence_number, Some(100));
    assert_eq!(cp.digest.as_deref(), Some("test_digest"));
}

#[tokio::test]
async fn get_checkpoint_missing_returns_error() {
    let svc = MockLedgerService::default();
    let channel = start_mock_ledger(svc).await;
    let mut client = sui_rpc::ledger_service_client::LedgerServiceClient::new(channel);
    let result = sentinel_backend::sui_client::get_checkpoint(&mut client, 999).await;
    assert!(result.is_err());
}

// === Tests: get_object_json ===

#[tokio::test]
async fn get_object_json_returns_parsed_json() {
    let json_value = serde_json::json!({
        "metadata": {"name": "Vex Nightburn"},
        "key": {"item_id": "12345"}
    });

    let mut objects = std::collections::HashMap::new();
    objects.insert(
        "0xabc".to_string(),
        sui_rpc::Object {
            object_id: Some("0xabc".into()),
            json: Some(make_json_proto_value(&json_value)),
            ..Default::default()
        },
    );

    let svc = MockLedgerService {
        objects,
        ..Default::default()
    };
    let channel = start_mock_ledger(svc).await;
    let json = sentinel_backend::sui_client::get_object_json(channel, "0xabc")
        .await
        .unwrap();

    assert_eq!(json["metadata"]["name"], "Vex Nightburn");
    assert_eq!(json["key"]["item_id"], "12345");
}

#[tokio::test]
async fn get_object_json_missing_object_returns_error() {
    let svc = MockLedgerService::default();
    let channel = start_mock_ledger(svc).await;
    let result = sentinel_backend::sui_client::get_object_json(channel, "0xnotfound").await;
    assert!(result.is_err());
}

// === Tests: batch_get_objects_json ===

#[tokio::test]
async fn batch_get_objects_returns_multiple() {
    let mut objects = std::collections::HashMap::new();
    for i in 1..=3 {
        let id = format!("0x{i:064x}");
        objects.insert(
            id.clone(),
            sui_rpc::Object {
                object_id: Some(id),
                json: Some(make_json_proto_value(&serde_json::json!({
                    "key": {"item_id": i.to_string()},
                    "metadata": {"name": format!("Pilot {i}")},
                }))),
                ..Default::default()
            },
        );
    }

    let svc = MockLedgerService {
        objects,
        ..Default::default()
    };
    let channel = start_mock_ledger(svc).await;

    let ids: Vec<String> = (1..=3).map(|i| format!("0x{i:064x}")).collect();
    let results = sentinel_backend::sui_client::batch_get_objects_json(channel, &ids)
        .await
        .unwrap();

    assert_eq!(results.len(), 3);
    for (id, json) in &results {
        assert!(!id.is_empty());
        assert!(json["metadata"]["name"].is_string());
    }
}

#[tokio::test]
async fn batch_get_objects_empty_input_returns_empty() {
    // Empty input short-circuits without making any RPC call
    let svc = MockLedgerService::default();
    let channel = start_mock_ledger(svc).await;
    let results = sentinel_backend::sui_client::batch_get_objects_json(channel, &[])
        .await
        .unwrap();
    assert!(results.is_empty());
}

// === Tests: list_dynamic_fields ===

#[tokio::test]
async fn list_dynamic_fields_returns_fields() {
    // Create a BCS-encoded ThreatEntryKey {character_item_id: 42}
    let key_bcs = bcs::to_bytes(&42u64).unwrap();
    // Create a BCS-encoded ThreatEntryValue (character_item_id=42, threat_score=5000, ...)
    // For simplicity, we just encode the first two fields since the test deserializer only reads those
    let mut value_bcs = Vec::new();
    value_bcs.extend_from_slice(&bcs::to_bytes(&42u64).unwrap()); // character_item_id
    value_bcs.extend_from_slice(&bcs::to_bytes(&5000u64).unwrap()); // threat_score

    let fields = vec![sui_rpc::DynamicField {
        kind: Some(sui_rpc::dynamic_field::DynamicFieldKind::Field.into()),
        parent: Some("0xregistry".into()),
        field_id: Some("0xfield1".into()),
        field_object: None,
        name: Some(sui_rpc::Bcs {
            name: Some("ThreatEntryKey".into()),
            value: Some(key_bcs),
        }),
        value: Some(sui_rpc::Bcs {
            name: Some("ThreatEntry".into()),
            value: Some(value_bcs),
        }),
        value_type: None,
        child_id: None,
        child_object: None,
    }];

    let svc = MockStateService {
        dynamic_fields: fields,
        ..Default::default()
    };
    let channel = start_mock_state(svc).await;

    let result = sentinel_backend::sui_client::list_dynamic_fields(channel, "0xregistry", 50)
        .await
        .unwrap();

    assert_eq!(result.len(), 1);
    let field = &result[0];
    assert_eq!(field.parent.as_deref(), Some("0xregistry"));

    // Verify BCS deserialization
    let key_bytes = field.name.as_ref().unwrap().value.as_ref().unwrap();
    let key: u64 = bcs::from_bytes(key_bytes).unwrap();
    assert_eq!(key, 42);
}

// === Tests: process_checkpoint_events ===

#[tokio::test]
async fn process_checkpoint_events_handles_killmail() {
    let config = sentinel_backend::config::AppConfig {
        sui_grpc_url: "http://unused".into(),
        sui_graphql_url: "http://unused".into(),
        sentinel_package_id: "0xsentinel".into(),
        threat_registry_id: String::new(),
        publisher_private_key: String::new(),
        world_package_id: "0xworld".into(),
        bounty_board_package_id: "0xbounty".into(),
        publish_interval_ms: 30_000,
        publish_score_threshold_bp: 100,
        api_port: 3001,
        database_url: "unused".into(),
        world_api_url: "unused".into(),
        sentinel_log_level: tracing::Level::INFO,
        crates_log_level: tracing::Level::WARN,
        log_format: sentinel_backend::config::LogFormat::Pretty,
        discord_token: "unused".into(),
        max_recent_events: 1000,
        public_url: None,
    };

    let kill_event_json = serde_json::json!({
        "killer_id": {"item_id": "100"},
        "victim_id": {"item_id": "200"},
        "solar_system_id": {"item_id": "J-1042"},
    });

    let checkpoint = sui_rpc::Checkpoint {
        sequence_number: Some(1),
        summary: Some(sui_rpc::CheckpointSummary {
            timestamp: Some(prost_types::Timestamp {
                seconds: 1700000000,
                nanos: 0,
            }),
            ..Default::default()
        }),
        transactions: vec![sui_rpc::ExecutedTransaction {
            events: Some(sui_rpc::TransactionEvents {
                events: vec![sui_rpc::Event {
                    package_id: Some("0xworld".into()),
                    module: Some("killmail".into()),
                    event_type: Some("0xworld::killmail::KillMailCreatedEvent".into()),
                    json: Some(make_json_proto_value(&kill_event_json)),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        }],
        ..Default::default()
    };

    let mut store = sentinel_backend::types::DataStore::default();
    let timestamp_ms = sentinel_backend::grpc::checkpoint_timestamp_ms(&checkpoint);

    sentinel_backend::grpc::process_checkpoint_events(
        &config,
        &mut store,
        &None,
        &checkpoint,
        timestamp_ms,
    );

    assert_eq!(store.profiles.len(), 2);
    let killer = store.profiles.get(&100).unwrap();
    assert_eq!(killer.kill_count, 1);
    assert!(killer.threat_score > 0);

    let victim = store.profiles.get(&200).unwrap();
    assert_eq!(victim.death_count, 1);

    assert_eq!(store.recent_events.len(), 1);
    assert_eq!(store.recent_events[0].event_type, "kill");
}

#[tokio::test]
async fn process_checkpoint_events_handles_character_created() {
    let config = sentinel_backend::config::AppConfig {
        sui_grpc_url: "http://unused".into(),
        sui_graphql_url: "http://unused".into(),
        sentinel_package_id: "0xsentinel".into(),
        threat_registry_id: String::new(),
        publisher_private_key: String::new(),
        world_package_id: "0xworld".into(),
        bounty_board_package_id: "0xbounty".into(),
        publish_interval_ms: 30_000,
        publish_score_threshold_bp: 100,
        api_port: 3001,
        database_url: "unused".into(),
        world_api_url: "unused".into(),
        sentinel_log_level: tracing::Level::INFO,
        crates_log_level: tracing::Level::WARN,
        log_format: sentinel_backend::config::LogFormat::Pretty,
        discord_token: "unused".into(),
        max_recent_events: 1000,
        public_url: None,
    };

    let event_json = serde_json::json!({
        "key": {"item_id": "42"},
    });

    let checkpoint = sui_rpc::Checkpoint {
        sequence_number: Some(1),
        summary: Some(sui_rpc::CheckpointSummary {
            timestamp: Some(prost_types::Timestamp {
                seconds: 1700000000,
                nanos: 0,
            }),
            ..Default::default()
        }),
        transactions: vec![sui_rpc::ExecutedTransaction {
            events: Some(sui_rpc::TransactionEvents {
                events: vec![sui_rpc::Event {
                    package_id: Some("0xworld".into()),
                    module: Some("character".into()),
                    event_type: Some("0xworld::character::CharacterCreatedEvent".into()),
                    json: Some(make_json_proto_value(&event_json)),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        }],
        ..Default::default()
    };

    let mut store = sentinel_backend::types::DataStore::default();
    let timestamp_ms = sentinel_backend::grpc::checkpoint_timestamp_ms(&checkpoint);

    sentinel_backend::grpc::process_checkpoint_events(
        &config,
        &mut store,
        &None,
        &checkpoint,
        timestamp_ms,
    );

    assert!(store.profiles.contains_key(&42));
    assert!(store.profiles.get(&42).unwrap().name.is_none());
    assert_eq!(store.new_pilot_events.len(), 1);
    assert_eq!(store.new_pilot_events[0].event_type, "new_character");
}

#[tokio::test]
async fn process_checkpoint_events_filters_wrong_package() {
    let config = sentinel_backend::config::AppConfig {
        sui_grpc_url: "http://unused".into(),
        sui_graphql_url: "http://unused".into(),
        sentinel_package_id: "0xsentinel".into(),
        threat_registry_id: String::new(),
        publisher_private_key: String::new(),
        world_package_id: "0xworld".into(),
        bounty_board_package_id: "0xbounty".into(),
        publish_interval_ms: 30_000,
        publish_score_threshold_bp: 100,
        api_port: 3001,
        database_url: "unused".into(),
        world_api_url: "unused".into(),
        sentinel_log_level: tracing::Level::INFO,
        crates_log_level: tracing::Level::WARN,
        log_format: sentinel_backend::config::LogFormat::Pretty,
        discord_token: "unused".into(),
        max_recent_events: 1000,
        public_url: None,
    };

    let checkpoint = sui_rpc::Checkpoint {
        sequence_number: Some(1),
        summary: None,
        transactions: vec![sui_rpc::ExecutedTransaction {
            events: Some(sui_rpc::TransactionEvents {
                events: vec![sui_rpc::Event {
                    package_id: Some("0xother_package".into()),
                    event_type: Some("0xother::killmail::KillMailCreatedEvent".into()),
                    json: Some(make_json_proto_value(&serde_json::json!({}))),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        }],
        ..Default::default()
    };

    let mut store = sentinel_backend::types::DataStore::default();
    sentinel_backend::grpc::process_checkpoint_events(&config, &mut store, &None, &checkpoint, 0);

    // Event from wrong package should be ignored
    assert!(store.profiles.is_empty());
    assert!(store.recent_events.is_empty());
}

// === Tests: historical checkpoint replay ===

#[tokio::test]
async fn checkpoint_replay_processes_events_across_checkpoints() {
    let config = sentinel_backend::config::AppConfig {
        sui_grpc_url: "unused".into(),
        sui_graphql_url: "http://unused".into(),
        sentinel_package_id: "0xsentinel".into(),
        threat_registry_id: String::new(),
        publisher_private_key: String::new(),
        world_package_id: "0xworld".into(),
        bounty_board_package_id: "0xbounty".into(),
        publish_interval_ms: 30_000,
        publish_score_threshold_bp: 100,
        api_port: 3001,
        database_url: "unused".into(),
        world_api_url: "unused".into(),
        sentinel_log_level: tracing::Level::INFO,
        crates_log_level: tracing::Level::WARN,
        log_format: sentinel_backend::config::LogFormat::Pretty,
        discord_token: "unused".into(),
        max_recent_events: 1000,
        public_url: None,
    };

    // Create 3 checkpoints with events
    let mut checkpoints = std::collections::HashMap::new();
    for seq in 10..=12 {
        let kill_json = serde_json::json!({
            "killer_id": {"item_id": (seq * 10).to_string()},
            "victim_id": {"item_id": ((seq * 10) + 1).to_string()},
            "solar_system_id": {"item_id": "30016543"},
        });

        checkpoints.insert(
            seq,
            sui_rpc::Checkpoint {
                sequence_number: Some(seq),
                summary: Some(sui_rpc::CheckpointSummary {
                    timestamp: Some(prost_types::Timestamp {
                        seconds: 1700000000 + seq as i64,
                        nanos: 0,
                    }),
                    ..Default::default()
                }),
                transactions: vec![sui_rpc::ExecutedTransaction {
                    events: Some(sui_rpc::TransactionEvents {
                        events: vec![sui_rpc::Event {
                            package_id: Some("0xworld".into()),
                            module: Some("killmail".into()),
                            event_type: Some("0xworld::killmail::KillMailCreatedEvent".into()),
                            json: Some(make_json_proto_value(&kill_json)),
                            ..Default::default()
                        }],
                        ..Default::default()
                    }),
                    ..Default::default()
                }],
                ..Default::default()
            },
        );
    }

    let svc = MockLedgerService {
        checkpoint_height: 12,
        checkpoints,
        ..Default::default()
    };
    let channel = start_mock_ledger(svc).await;

    // Simulate replay from checkpoint 9 (so we process 10, 11, 12)
    let state = std::sync::Arc::new(tokio::sync::RwLock::new(
        sentinel_backend::types::AppState {
            last_checkpoint: Some(9),
            ..Default::default()
        },
    ));

    let mut client = sui_rpc::ledger_service_client::LedgerServiceClient::new(channel.clone());

    // Manually replay to test the processing
    for seq in 10..=12 {
        let resp = sentinel_backend::sui_client::get_checkpoint(&mut client, seq)
            .await
            .unwrap();
        if let Some(ref cp) = resp.checkpoint {
            let ts = sentinel_backend::grpc::checkpoint_timestamp_ms(cp);
            let mut s = state.write().await;
            sentinel_backend::grpc::process_checkpoint_events(&config, &mut s.live, &None, cp, ts);
        }
    }

    let s = state.read().await;
    // 3 checkpoints * 2 profiles each (killer + victim) = 6 profiles
    assert_eq!(s.live.profiles.len(), 6);
    assert_eq!(s.live.recent_events.len(), 3);
}

// === Tests: resolve_gate_name ===

#[tokio::test]
async fn resolve_gate_name_fetches_from_grpc() {
    let gate_json = serde_json::json!({
        "metadata": {"name": "Stargate Alpha"},
        "key": {"item_id": "999"},
    });

    let mut objects = std::collections::HashMap::new();
    objects.insert(
        "0xgate1".to_string(),
        sui_rpc::Object {
            object_id: Some("0xgate1".into()),
            json: Some(make_json_proto_value(&gate_json)),
            ..Default::default()
        },
    );

    let svc = MockLedgerService {
        objects,
        ..Default::default()
    };
    let channel = start_mock_ledger(svc).await;

    let mut cache = std::collections::HashMap::new();
    let name =
        sentinel_backend::historical::resolve_gate_name(channel, "0xgate1", &mut cache).await;

    assert_eq!(name, "Stargate Alpha");
    // Should be cached now
    assert_eq!(cache.get("0xgate1").unwrap(), "Stargate Alpha");
}

#[tokio::test]
async fn resolve_gate_name_uses_cache() {
    let mut cache = std::collections::HashMap::new();
    cache.insert("0xgate1".to_string(), "Cached Gate".to_string());

    // Channel won't be used — cache hit. Use a mock server channel to satisfy types.
    let svc = MockLedgerService::default();
    let channel = start_mock_ledger(svc).await;
    let name =
        sentinel_backend::historical::resolve_gate_name(channel, "0xgate1", &mut cache).await;

    assert_eq!(name, "Cached Gate");
}

#[tokio::test]
async fn resolve_gate_name_falls_back_on_error() {
    let svc = MockLedgerService::default(); // no objects
    let channel = start_mock_ledger(svc).await;

    let mut cache = std::collections::HashMap::new();
    let name =
        sentinel_backend::historical::resolve_gate_name(channel, "0xunknown", &mut cache).await;

    // Falls back to truncated ID
    assert!(name.starts_with("Gate "));
    assert!(cache.contains_key("0xunknown"));
}
