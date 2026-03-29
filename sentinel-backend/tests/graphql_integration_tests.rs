//! Integration tests against the real Sui testnet GraphQL endpoint.
//! These tests make actual network calls and may be slow or flaky.
//!
//! Run with: cargo test --test graphql_integration_tests -- --ignored

use std::sync::Arc;
use tokio::sync::RwLock;

use sentinel_backend::config::AppConfig;
use sentinel_backend::types::AppState;

const TESTNET_GRAPHQL: &str = "https://sui-testnet.mystenlabs.com/graphql";

/// World package ID on testnet (EVE Frontier).
const WORLD_PACKAGE_ID: &str =
    "0x28b497559d65ab320d9da4613bf2498d5946b2c0ae3597ccfda3072ce127448c";

fn testnet_config() -> AppConfig {
    AppConfig {
        sui_grpc_url: "https://fullnode.testnet.sui.io:443".into(),
        sui_graphql_url: TESTNET_GRAPHQL.into(),
        sentinel_package_id: String::new(),
        threat_registry_id: String::new(),
        admin_private_key: String::new(),
        world_package_id: WORLD_PACKAGE_ID.into(),
        bounty_board_package_id: String::new(),
        publish_interval_ms: 30_000,
        api_port: 3001,
        database_url: "unused".into(),
        world_api_url: "unused".into(),
    }
}

#[tokio::test]
#[ignore]
async fn real_graphql_load_killmails() {
    let config = testnet_config();
    let state = Arc::new(RwLock::new(AppState::default()));

    let count = sentinel_backend::historical::load_historical_killmails(&config, &state)
        .await
        .unwrap();

    println!("Loaded {count} killmails from testnet");
    // There should be at least some killmails on testnet
    let s = state.read().await;
    println!("Profiles created: {}", s.live.profiles.len());
    // Even if count is 0, the query should succeed without error
}

#[tokio::test]
#[ignore]
async fn real_graphql_load_character_events() {
    let config = testnet_config();
    let state = Arc::new(RwLock::new(AppState::default()));

    let count = sentinel_backend::historical::load_character_events(&config, &state)
        .await
        .unwrap();

    println!("Loaded {count} character creation events from testnet");
    let s = state.read().await;
    println!("Profiles: {}, new_pilot_events: {}", s.live.profiles.len(), s.live.new_pilot_events.len());
}

#[tokio::test]
#[ignore]
async fn real_graphql_load_jump_events() {
    let config = testnet_config();
    let state = Arc::new(RwLock::new(AppState::default()));

    let count = sentinel_backend::historical::load_jump_events(&config, &state)
        .await
        .unwrap();

    println!("Loaded {count} jump events from testnet");
    let s = state.read().await;
    println!("Profiles: {}, recent_events: {}", s.live.profiles.len(), s.live.recent_events.len());
}

#[tokio::test]
#[ignore]
async fn real_graphql_load_character_names() {
    let config = testnet_config();
    let state = Arc::new(RwLock::new(AppState::default()));

    // First load some characters so there are profiles to resolve names for
    let _ = sentinel_backend::historical::load_character_events(&config, &state).await;

    let profile_count = state.read().await.live.profiles.len();
    if profile_count == 0 {
        println!("No profiles to resolve names for, skipping");
        return;
    }

    let pool = sqlx::PgPool::connect_lazy("postgresql://invalid:invalid@localhost/invalid")
        .unwrap();

    let count = sentinel_backend::historical::load_character_names_graphql(&config, &state, &pool)
        .await
        .unwrap();

    println!("Resolved {count} character names from testnet");
    let s = state.read().await;
    let resolved = s.live.profiles.values().filter(|p| !p.name.starts_with("Pilot #")).count();
    println!("Resolved names: {resolved}/{}", s.live.profiles.len());
}

#[tokio::test]
#[ignore]
async fn real_graphql_endpoint_responds() {
    let http = reqwest::Client::new();
    let query = r#"{ checkpoint { sequenceNumber } }"#;

    let resp = http
        .post(TESTNET_GRAPHQL)
        .json(&serde_json::json!({ "query": query }))
        .send()
        .await
        .unwrap();

    assert!(resp.status().is_success(), "GraphQL endpoint returned {}", resp.status());
    let json: serde_json::Value = resp.json().await.unwrap();
    let seq = json["data"]["checkpoint"]["sequenceNumber"]
        .as_u64()
        .expect("expected sequenceNumber in response");
    println!("Current testnet checkpoint: {seq}");
    assert!(seq > 1000);
}
