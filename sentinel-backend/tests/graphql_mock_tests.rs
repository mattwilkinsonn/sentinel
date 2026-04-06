//! Mock GraphQL tests for historical data loading.
//! Spins up a lightweight HTTP server returning canned GraphQL responses,
//! then runs the historical loaders against it.

use std::sync::Arc;
use tokio::sync::RwLock;

use axum::{Json, Router, extract::State as AxumState, routing::post};
use sentinel_backend::config::{AppConfig, LogFormat};
use sentinel_backend::types::AppState;

// === Mock GraphQL Server ===

/// Shared state for the mock GraphQL server.
#[derive(Clone)]
struct MockGraphQL {
    /// Queued responses: each request pops the next response.
    responses: Arc<std::sync::Mutex<Vec<serde_json::Value>>>,
}

async fn graphql_handler(
    AxumState(state): AxumState<MockGraphQL>,
    Json(_body): Json<serde_json::Value>,
) -> Json<serde_json::Value> {
    let mut responses = state.responses.lock().unwrap();
    if responses.is_empty() {
        // Return empty data if no more queued responses
        Json(serde_json::json!({
            "data": { "objects": { "nodes": [], "pageInfo": { "hasNextPage": false } } }
        }))
    } else {
        Json(responses.remove(0))
    }
}

/// Start a mock GraphQL server and return its URL.
async fn start_mock_graphql(responses: Vec<serde_json::Value>) -> String {
    let state = MockGraphQL {
        responses: Arc::new(std::sync::Mutex::new(responses)),
    };
    let app = Router::new()
        .route("/graphql", post(graphql_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    tokio::spawn(async move {
        axum::serve(listener, app).await.unwrap();
    });

    format!("http://{}/graphql", addr)
}

fn test_config(graphql_url: &str) -> AppConfig {
    AppConfig {
        sui_grpc_url: "http://unused".into(),
        sui_graphql_url: graphql_url.into(),
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
        log_format: LogFormat::Pretty,
        discord_token: "unused".into(),
        max_recent_events: 1000,
        public_url: None,
    }
}

// === Killmail Tests ===

#[tokio::test]
async fn load_killmails_parses_single_page() {
    let response = serde_json::json!({
        "data": {
            "objects": {
                "nodes": [
                    {
                        "asMoveObject": {
                            "contents": {
                                "json": {
                                    "killer_id": { "item_id": "100", "tenant": "stillness" },
                                    "victim_id": { "item_id": "200", "tenant": "stillness" },
                                    "loss_type": { "@variant": "SHIP" },
                                    "kill_timestamp": "1700000000",
                                    "solar_system_id": { "item_id": "30004759" },
                                    "id": "km-001"
                                }
                            }
                        }
                    },
                    {
                        "asMoveObject": {
                            "contents": {
                                "json": {
                                    "killer_id": { "item_id": "300" },
                                    "victim_id": { "item_id": "400" },
                                    "loss_type": { "@variant": "STRUCTURE" },
                                    "kill_timestamp": "1700001000",
                                    "solar_system_id": { "item_id": "30004760" },
                                    "id": "km-002"
                                }
                            }
                        }
                    }
                ],
                "pageInfo": { "hasNextPage": false }
            }
        }
    });

    let url = start_mock_graphql(vec![response]).await;
    let config = test_config(&url);
    let state = Arc::new(RwLock::new(AppState::default()));

    let count = sentinel_backend::historical::load_historical_killmails(&config, &state)
        .await
        .unwrap();

    assert_eq!(count, 2);

    let s = state.read().await;
    // Killer 100 should have a profile
    assert!(s.live.profiles.contains_key(&100));
    assert_eq!(s.live.profiles[&100].kill_count, 1);
    // Victim 200 (ship kill) should have death_count
    assert!(s.live.profiles.contains_key(&200));
    assert_eq!(s.live.profiles[&200].death_count, 1);
    // Victim 400 (structure kill) should NOT have death_count incremented
    // (structure kills don't track victim deaths)
    assert!(!s.live.profiles.contains_key(&400));
    // Killer 300 should exist
    assert!(s.live.profiles.contains_key(&300));
    assert_eq!(s.live.profiles[&300].kill_count, 1);
}

#[tokio::test]
async fn load_killmails_handles_pagination() {
    let page1 = serde_json::json!({
        "data": {
            "objects": {
                "nodes": [{
                    "asMoveObject": {
                        "contents": {
                            "json": {
                                "killer_id": { "item_id": "10" },
                                "victim_id": { "item_id": "20" },
                                "loss_type": { "@variant": "SHIP" },
                                "kill_timestamp": "1700000000",
                                "solar_system_id": { "item_id": "100" },
                                "id": "km-p1"
                            }
                        }
                    }
                }],
                "pageInfo": { "hasNextPage": true, "endCursor": "cursor1" }
            }
        }
    });
    let page2 = serde_json::json!({
        "data": {
            "objects": {
                "nodes": [{
                    "asMoveObject": {
                        "contents": {
                            "json": {
                                "killer_id": { "item_id": "30" },
                                "victim_id": { "item_id": "40" },
                                "loss_type": { "@variant": "SHIP" },
                                "kill_timestamp": "1700001000",
                                "solar_system_id": { "item_id": "200" },
                                "id": "km-p2"
                            }
                        }
                    }
                }],
                "pageInfo": { "hasNextPage": false }
            }
        }
    });

    let url = start_mock_graphql(vec![page1, page2]).await;
    let config = test_config(&url);
    let state = Arc::new(RwLock::new(AppState::default()));

    let count = sentinel_backend::historical::load_historical_killmails(&config, &state)
        .await
        .unwrap();

    assert_eq!(count, 2);
    let s = state.read().await;
    assert!(s.live.profiles.contains_key(&10));
    assert!(s.live.profiles.contains_key(&30));
}

#[tokio::test]
async fn load_killmails_deduplicates_by_id() {
    let response = serde_json::json!({
        "data": {
            "objects": {
                "nodes": [
                    {
                        "asMoveObject": {
                            "contents": {
                                "json": {
                                    "killer_id": { "item_id": "100" },
                                    "victim_id": { "item_id": "200" },
                                    "loss_type": { "@variant": "SHIP" },
                                    "kill_timestamp": "1700000000",
                                    "solar_system_id": { "item_id": "100" },
                                    "id": "same-id"
                                }
                            }
                        }
                    },
                    {
                        "asMoveObject": {
                            "contents": {
                                "json": {
                                    "killer_id": { "item_id": "100" },
                                    "victim_id": { "item_id": "200" },
                                    "loss_type": { "@variant": "SHIP" },
                                    "kill_timestamp": "1700000000",
                                    "solar_system_id": { "item_id": "100" },
                                    "id": "same-id"
                                }
                            }
                        }
                    }
                ],
                "pageInfo": { "hasNextPage": false }
            }
        }
    });

    let url = start_mock_graphql(vec![response]).await;
    let config = test_config(&url);
    let state = Arc::new(RwLock::new(AppState::default()));

    let count = sentinel_backend::historical::load_historical_killmails(&config, &state)
        .await
        .unwrap();

    // Only 1 counted because the duplicate is skipped
    assert_eq!(count, 1);
    let s = state.read().await;
    assert_eq!(s.live.profiles[&100].kill_count, 1);
}

#[tokio::test]
async fn load_killmails_empty_response() {
    let response = serde_json::json!({
        "data": {
            "objects": {
                "nodes": [],
                "pageInfo": { "hasNextPage": false }
            }
        }
    });

    let url = start_mock_graphql(vec![response]).await;
    let config = test_config(&url);
    let state = Arc::new(RwLock::new(AppState::default()));

    let count = sentinel_backend::historical::load_historical_killmails(&config, &state)
        .await
        .unwrap();

    assert_eq!(count, 0);
    assert!(state.read().await.live.profiles.is_empty());
}

#[tokio::test]
async fn load_killmails_skips_empty_package() {
    let config = AppConfig {
        world_package_id: String::new(),
        ..test_config("http://unused")
    };
    let state = Arc::new(RwLock::new(AppState::default()));

    let count = sentinel_backend::historical::load_historical_killmails(&config, &state)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

// === Character Event Tests ===

#[tokio::test]
async fn load_character_events_creates_profiles() {
    let response = serde_json::json!({
        "data": {
            "events": {
                "nodes": [
                    {
                        "contents": {
                            "json": { "key": { "item_id": "42" } }
                        },
                        "timestamp": "2024-01-15T10:30:00Z"
                    },
                    {
                        "contents": {
                            "json": { "key": { "item_id": "99" } }
                        },
                        "timestamp": "2024-01-15T11:00:00Z"
                    }
                ],
                "pageInfo": { "hasNextPage": false }
            }
        }
    });

    let url = start_mock_graphql(vec![response]).await;
    let config = test_config(&url);
    let state = Arc::new(RwLock::new(AppState::default()));

    let count = sentinel_backend::historical::load_character_events(&config, &state)
        .await
        .unwrap();

    assert_eq!(count, 2);
    let s = state.read().await;
    assert!(s.live.profiles.contains_key(&42));
    assert!(s.live.profiles.contains_key(&99));
    assert!(s.live.profiles[&42].name.is_none());
    // Should have new_character events
    assert_eq!(s.live.new_pilot_events.len(), 2);
    assert_eq!(s.live.new_pilot_events[0].event_type, "new_character");
}

#[tokio::test]
async fn load_character_events_skips_when_existing() {
    let url = start_mock_graphql(vec![]).await;
    let config = test_config(&url);
    let state = Arc::new(RwLock::new(AppState::default()));

    // Pre-populate with an existing new_character event
    {
        let mut s = state.write().await;
        s.live.push_event(
            sentinel_backend::types::RawEvent {
                event_type: "new_character".into(),
                timestamp_ms: 1000,
                data: serde_json::json!({}),
            },
            &None,
            1000,
        );
    }

    let count = sentinel_backend::historical::load_character_events(&config, &state)
        .await
        .unwrap();

    // Should skip since events already exist
    assert_eq!(count, 0);
}

// === Jump Event Tests ===

#[tokio::test]
async fn load_jump_events_creates_profiles_and_events() {
    // First response: jump events page
    let events_response = serde_json::json!({
        "data": {
            "events": {
                "nodes": [
                    {
                        "contents": {
                            "json": {
                                "character_key": { "item_id": "55" },
                                "source_gate_id": "0xgate1",
                                "destination_gate_id": "0xgate2"
                            }
                        },
                        "timestamp": "2024-02-01T12:00:00Z"
                    }
                ],
                "pageInfo": { "hasNextPage": false }
            }
        }
    });

    // Gate resolution responses (resolve_gate_name_graphql makes separate queries)
    let gate1_response = serde_json::json!({
        "data": {
            "object": {
                "asMoveObject": {
                    "contents": {
                        "json": {
                            "metadata": { "name": "Stargate Alpha" },
                            "key": { "item_id": "1001" }
                        }
                    }
                }
            }
        }
    });
    let gate2_response = serde_json::json!({
        "data": {
            "object": {
                "asMoveObject": {
                    "contents": {
                        "json": {
                            "metadata": { "name": "Stargate Beta" },
                            "key": { "item_id": "1002" }
                        }
                    }
                }
            }
        }
    });

    let url = start_mock_graphql(vec![events_response, gate1_response, gate2_response]).await;
    let config = test_config(&url);
    let state = Arc::new(RwLock::new(AppState::default()));

    let count = sentinel_backend::historical::load_jump_events(&config, &state)
        .await
        .unwrap();

    assert_eq!(count, 1);
    let s = state.read().await;
    assert!(s.live.profiles.contains_key(&55));
    assert_eq!(s.live.profiles[&55].systems_visited, 1);
    // Should have a jump event
    assert_eq!(s.live.recent_events.len(), 1);
    assert_eq!(s.live.recent_events[0].event_type, "jump");
    assert_eq!(
        s.live.recent_events[0].data["source_gate"],
        "Stargate Alpha"
    );
    assert_eq!(s.live.recent_events[0].data["dest_gate"], "Stargate Beta");
}

#[tokio::test]
async fn load_jump_events_skips_when_existing() {
    let url = start_mock_graphql(vec![]).await;
    let config = test_config(&url);
    let state = Arc::new(RwLock::new(AppState::default()));

    {
        let mut s = state.write().await;
        s.live.push_event(
            sentinel_backend::types::RawEvent {
                event_type: "jump".into(),
                timestamp_ms: 1000,
                data: serde_json::json!({}),
            },
            &None,
            1000,
        );
    }

    let count = sentinel_backend::historical::load_jump_events(&config, &state)
        .await
        .unwrap();
    assert_eq!(count, 0);
}

// === Character Name Tests ===

#[tokio::test]
async fn load_character_names_resolves_names() {
    let response = serde_json::json!({
        "data": {
            "objects": {
                "nodes": [
                    {
                        "asMoveObject": {
                            "contents": {
                                "json": {
                                    "key": { "item_id": "42" },
                                    "metadata": { "name": "Captain Kirk" },
                                    "tribe_id": "7"
                                }
                            }
                        }
                    },
                    {
                        "asMoveObject": {
                            "contents": {
                                "json": {
                                    "key": { "item_id": "99" },
                                    "metadata": { "name": "Spock" },
                                    "tribe_id": "3"
                                }
                            }
                        }
                    }
                ],
                "pageInfo": { "hasNextPage": false }
            }
        }
    });

    let url = start_mock_graphql(vec![response]).await;
    let config = test_config(&url);
    let state = Arc::new(RwLock::new(AppState::default()));

    // Pre-populate profiles with unresolved names
    {
        let mut s = state.write().await;
        s.live.profiles.insert(
            42,
            sentinel_backend::types::ThreatProfile {
                character_item_id: 42,
                name: None,
                ..Default::default()
            },
        );
        s.live.profiles.insert(
            99,
            sentinel_backend::types::ThreatProfile {
                character_item_id: 99,
                name: None,
                ..Default::default()
            },
        );
    }

    // Note: this function takes a pool, but we can't easily mock that.
    // The DB upsert calls will fail silently (fire-and-forget in the code).
    // We test the in-memory name resolution, not DB persistence.
    // Use a dummy pool that will fail on connect.
    let pool =
        sqlx::PgPool::connect_lazy("postgresql://invalid:invalid@localhost/invalid").unwrap();

    let count = sentinel_backend::historical::load_character_names_graphql(&config, &state, &pool)
        .await
        .unwrap();

    assert_eq!(count, 2);
    let s = state.read().await;
    assert_eq!(s.live.profiles[&42].name, Some("Captain Kirk".to_string()));
    assert_eq!(s.live.profiles[&99].name, Some("Spock".to_string()));
    assert_eq!(s.live.profiles[&42].tribe_id, "7");
    assert_eq!(s.live.profiles[&99].tribe_id, "3");
    // Name cache should be populated
    assert_eq!(s.live.name_cache[&42], "Captain Kirk");
    assert_eq!(s.live.name_cache[&99], "Spock");
}

#[tokio::test]
async fn load_character_names_skips_when_all_resolved() {
    let url = start_mock_graphql(vec![]).await;
    let config = test_config(&url);
    let state = Arc::new(RwLock::new(AppState::default()));

    // Pre-populate with already-resolved names
    {
        let mut s = state.write().await;
        s.live.profiles.insert(
            42,
            sentinel_backend::types::ThreatProfile {
                character_item_id: 42,
                name: Some("Already Resolved".to_string()),
                ..Default::default()
            },
        );
    }

    let pool =
        sqlx::PgPool::connect_lazy("postgresql://invalid:invalid@localhost/invalid").unwrap();

    let count = sentinel_backend::historical::load_character_names_graphql(&config, &state, &pool)
        .await
        .unwrap();

    assert_eq!(count, 0);
}

#[tokio::test]
async fn load_killmails_sets_last_seen_system() {
    let response = serde_json::json!({
        "data": {
            "objects": {
                "nodes": [{
                    "asMoveObject": {
                        "contents": {
                            "json": {
                                "killer_id": { "item_id": "100" },
                                "victim_id": { "item_id": "200" },
                                "loss_type": { "@variant": "SHIP" },
                                "kill_timestamp": "1700000000",
                                "solar_system_id": { "item_id": "30004759" },
                                "id": "km-sys"
                            }
                        }
                    }
                }],
                "pageInfo": { "hasNextPage": false }
            }
        }
    });

    let url = start_mock_graphql(vec![response]).await;
    let config = test_config(&url);
    let state = Arc::new(RwLock::new(AppState::default()));

    sentinel_backend::historical::load_historical_killmails(&config, &state)
        .await
        .unwrap();

    let s = state.read().await;
    assert_eq!(s.live.profiles[&100].last_seen_system, "30004759");
}

#[tokio::test]
async fn load_killmails_converts_seconds_to_millis() {
    let response = serde_json::json!({
        "data": {
            "objects": {
                "nodes": [{
                    "asMoveObject": {
                        "contents": {
                            "json": {
                                "killer_id": { "item_id": "100" },
                                "victim_id": { "item_id": "200" },
                                "loss_type": { "@variant": "SHIP" },
                                "kill_timestamp": "1700000000",
                                "solar_system_id": { "item_id": "100" },
                                "id": "km-ts"
                            }
                        }
                    }
                }],
                "pageInfo": { "hasNextPage": false }
            }
        }
    });

    let url = start_mock_graphql(vec![response]).await;
    let config = test_config(&url);
    let state = Arc::new(RwLock::new(AppState::default()));

    sentinel_backend::historical::load_historical_killmails(&config, &state)
        .await
        .unwrap();

    let s = state.read().await;
    // 1700000000 seconds should become 1700000000000 milliseconds
    assert_eq!(s.live.profiles[&100].last_kill_timestamp, 1700000000000);
}
