//! Integration tests for Postgres persistence.
//!
//! Requires a running Postgres instance. Set DATABASE_URL or use:
//!   docker compose up postgres -d
//!   DATABASE_URL=postgresql://sentinel:sentinel@localhost/sentinel cargo test

use serial_test::serial;
use sqlx::PgPool;

mod common {
    use sqlx::PgPool;

    /// Run all migrations and truncate all app tables.
    pub async fn setup(pool: &PgPool) {
        sentinel_backend::db::run_migrations(pool)
            .await
            .expect("migrations failed");

        // Truncate all app tables for a clean slate
        let tables = sqlx::query_scalar::<_, String>(
            "SELECT tablename FROM pg_tables WHERE schemaname = 'public'",
        )
        .fetch_all(pool)
        .await
        .expect("failed to list tables");

        if !tables.is_empty() {
            let truncate = format!("TRUNCATE {} CASCADE", tables.join(", "));
            sqlx::raw_sql(&truncate)
                .execute(pool)
                .await
                .expect("truncate failed");
        }
    }
}

fn database_url() -> String {
    std::env::var("DATABASE_URL")
        .unwrap_or_else(|_| "postgresql://sentinel:sentinel@localhost/sentinel".into())
}

async fn pool() -> PgPool {
    PgPool::connect(&database_url())
        .await
        .expect("Failed to connect to Postgres — is it running?")
}

// ── Profile persistence ──

#[tokio::test]
#[serial]
async fn upsert_and_load_profile() {
    let pool = pool().await;
    common::setup(&pool).await;

    let profile = sentinel_backend::types::ThreatProfile {
        character_item_id: 42,
        name: "Test Pilot".into(),
        threat_score: 5000,
        kill_count: 10,
        death_count: 3,
        bounty_count: 2,
        last_kill_timestamp: 1700000000000,
        last_seen_system: "J-1042".into(),
        recent_kills_24h: 4,
        systems_visited: 6,
        ..Default::default()
    };

    sentinel_backend::db::upsert_profile(&pool, &profile)
        .await
        .unwrap();

    let mut store = sentinel_backend::types::DataStore::default();
    sentinel_backend::db::load_into(&pool, &mut store)
        .await
        .unwrap();

    assert_eq!(store.profiles.len(), 1);
    let loaded = store.profiles.get(&42).unwrap();
    assert_eq!(loaded.name, "Test Pilot");
    assert_eq!(loaded.threat_score, 5000);
    assert_eq!(loaded.kill_count, 10);
    assert_eq!(loaded.death_count, 3);
    assert_eq!(loaded.bounty_count, 2);
    assert_eq!(loaded.last_seen_system, "J-1042");
    assert_eq!(loaded.recent_kills_24h, 4);
    assert_eq!(loaded.systems_visited, 6);
}

#[tokio::test]
#[serial]
async fn upsert_updates_existing_profile() {
    let pool = pool().await;
    common::setup(&pool).await;

    let mut profile = sentinel_backend::types::ThreatProfile {
        character_item_id: 99,
        name: "Original".into(),
        kill_count: 1,
        ..Default::default()
    };

    sentinel_backend::db::upsert_profile(&pool, &profile)
        .await
        .unwrap();

    profile.name = "Updated".into();
    profile.kill_count = 5;
    profile.threat_score = 3000;

    sentinel_backend::db::upsert_profile(&pool, &profile)
        .await
        .unwrap();

    let mut store = sentinel_backend::types::DataStore::default();
    sentinel_backend::db::load_into(&pool, &mut store)
        .await
        .unwrap();

    assert_eq!(store.profiles.len(), 1);
    let loaded = store.profiles.get(&99).unwrap();
    assert_eq!(loaded.name, "Updated");
    assert_eq!(loaded.kill_count, 5);
    assert_eq!(loaded.threat_score, 3000);
}

// ── Event persistence ──

#[tokio::test]
#[serial]
async fn insert_and_load_events() {
    let pool = pool().await;
    common::setup(&pool).await;

    let events = vec![
        sentinel_backend::types::RawEvent {
            event_type: "kill".into(),
            timestamp_ms: 1000,
            data: serde_json::json!({"killer": 1, "victim": 2}),
        },
        sentinel_backend::types::RawEvent {
            event_type: "jump".into(),
            timestamp_ms: 2000,
            data: serde_json::json!({"character_id": 1, "system": "J-1042"}),
        },
        sentinel_backend::types::RawEvent {
            event_type: "bounty_posted".into(),
            timestamp_ms: 3000,
            data: serde_json::json!({"target": 2}),
        },
    ];

    for e in &events {
        sentinel_backend::db::insert_event(&pool, e).await.unwrap();
    }

    let mut store = sentinel_backend::types::DataStore::default();
    sentinel_backend::db::load_into(&pool, &mut store)
        .await
        .unwrap();

    assert_eq!(store.recent_events.len(), 3);
    // Loaded newest first
    assert_eq!(store.recent_events[0].event_type, "bounty_posted");
    assert_eq!(store.recent_events[0].timestamp_ms, 3000);
    assert_eq!(store.recent_events[2].event_type, "kill");
}

// ── Checkpoint persistence ──

#[tokio::test]
#[serial]
async fn save_and_load_checkpoint() {
    let pool = pool().await;
    common::setup(&pool).await;

    // No checkpoint initially
    let cp = sentinel_backend::db::load_checkpoint(&pool).await.unwrap();
    assert_eq!(cp, None);

    // Save checkpoint
    sentinel_backend::db::save_checkpoint(&pool, 12345)
        .await
        .unwrap();

    let cp = sentinel_backend::db::load_checkpoint(&pool).await.unwrap();
    assert_eq!(cp, Some(12345));

    // Update checkpoint
    sentinel_backend::db::save_checkpoint(&pool, 99999)
        .await
        .unwrap();

    let cp = sentinel_backend::db::load_checkpoint(&pool).await.unwrap();
    assert_eq!(cp, Some(99999));
}

// ── Event pruning ──

#[tokio::test]
#[serial]
async fn prune_events_keeps_most_recent() {
    let pool = pool().await;
    common::setup(&pool).await;

    for i in 0..10 {
        sentinel_backend::db::insert_event(
            &pool,
            &sentinel_backend::types::RawEvent {
                event_type: "kill".into(),
                timestamp_ms: i * 1000,
                data: serde_json::json!({"seq": i}),
            },
        )
        .await
        .unwrap();
    }

    let pruned = sentinel_backend::db::prune_events(&pool, 3).await.unwrap();
    assert_eq!(pruned, 7);

    let mut store = sentinel_backend::types::DataStore::default();
    sentinel_backend::db::load_into(&pool, &mut store)
        .await
        .unwrap();

    assert_eq!(store.recent_events.len(), 3);
    // Most recent kept
    assert_eq!(store.recent_events[0].timestamp_ms, 9000);
    assert_eq!(store.recent_events[1].timestamp_ms, 8000);
    assert_eq!(store.recent_events[2].timestamp_ms, 7000);
}

// ── Multiple profiles ──

#[tokio::test]
#[serial]
async fn load_multiple_profiles() {
    let pool = pool().await;
    common::setup(&pool).await;

    for id in [1, 2, 3, 4, 5] {
        sentinel_backend::db::upsert_profile(
            &pool,
            &sentinel_backend::types::ThreatProfile {
                character_item_id: id,
                name: format!("Pilot #{id}"),
                threat_score: id * 1000,
                ..Default::default()
            },
        )
        .await
        .unwrap();
    }

    let mut store = sentinel_backend::types::DataStore::default();
    sentinel_backend::db::load_into(&pool, &mut store)
        .await
        .unwrap();

    assert_eq!(store.profiles.len(), 5);
    assert_eq!(store.profiles.get(&3).unwrap().threat_score, 3000);
    assert_eq!(store.profiles.get(&5).unwrap().name, "Pilot #5");
}
