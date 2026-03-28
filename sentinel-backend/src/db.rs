use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

use crate::types::{DataStore, RawEvent, ThreatProfile};

/// Connect to Postgres and run migrations.
pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await?;

    // Run migrations from embedded SQL
    sqlx::raw_sql(include_str!("../migrations/001_init.sql"))
        .execute(&pool)
        .await?;

    tracing::info!("Database connected and migrations applied");
    Ok(pool)
}

/// Load all threat profiles and recent events into the live DataStore.
pub async fn load_into(pool: &PgPool, store: &mut DataStore) -> Result<(), sqlx::Error> {
    // Load profiles
    let rows = sqlx::query_as::<_, ProfileRow>(
        "SELECT character_item_id, name, threat_score, kill_count, death_count, \
         bounty_count, last_kill_timestamp, last_seen_system, recent_kills_24h, \
         systems_visited FROM threat_profiles",
    )
    .fetch_all(pool)
    .await?;

    for r in rows {
        store.profiles.insert(
            r.character_item_id as u64,
            ThreatProfile {
                character_item_id: r.character_item_id as u64,
                name: r.name,
                threat_score: r.threat_score as u64,
                kill_count: r.kill_count as u64,
                death_count: r.death_count as u64,
                bounty_count: r.bounty_count as u64,
                last_kill_timestamp: r.last_kill_timestamp as u64,
                last_seen_system: r.last_seen_system,
                recent_kills_24h: r.recent_kills_24h as u64,
                systems_visited: r.systems_visited as u64,
                dirty: false,
            },
        );
    }

    // Load recent events (most recent 200)
    let events = sqlx::query_as::<_, EventRow>(
        "SELECT event_type, timestamp_ms, data FROM raw_events \
         ORDER BY timestamp_ms DESC LIMIT 200",
    )
    .fetch_all(pool)
    .await?;

    for e in events {
        store.recent_events.push_back(RawEvent {
            event_type: e.event_type,
            timestamp_ms: e.timestamp_ms as u64,
            data: e.data,
        });
    }

    tracing::info!(
        "Loaded {} profiles and {} events from database",
        store.profiles.len(),
        store.recent_events.len()
    );
    Ok(())
}

/// Load the last checkpoint cursor.
pub async fn load_checkpoint(pool: &PgPool) -> Result<Option<u64>, sqlx::Error> {
    let row =
        sqlx::query_scalar::<_, i64>("SELECT last_checkpoint FROM checkpoint_cursor WHERE id = 1")
            .fetch_optional(pool)
            .await?;
    Ok(row.map(|v| v as u64))
}

/// Upsert a threat profile.
pub async fn upsert_profile(pool: &PgPool, p: &ThreatProfile) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO threat_profiles \
         (character_item_id, name, threat_score, kill_count, death_count, \
          bounty_count, last_kill_timestamp, last_seen_system, recent_kills_24h, \
          systems_visited, updated_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10, NOW()) \
         ON CONFLICT (character_item_id) DO UPDATE SET \
          name = EXCLUDED.name, \
          threat_score = EXCLUDED.threat_score, \
          kill_count = EXCLUDED.kill_count, \
          death_count = EXCLUDED.death_count, \
          bounty_count = EXCLUDED.bounty_count, \
          last_kill_timestamp = EXCLUDED.last_kill_timestamp, \
          last_seen_system = EXCLUDED.last_seen_system, \
          recent_kills_24h = EXCLUDED.recent_kills_24h, \
          systems_visited = EXCLUDED.systems_visited, \
          updated_at = NOW()",
    )
    .bind(p.character_item_id as i64)
    .bind(&p.name)
    .bind(p.threat_score as i64)
    .bind(p.kill_count as i64)
    .bind(p.death_count as i64)
    .bind(p.bounty_count as i64)
    .bind(p.last_kill_timestamp as i64)
    .bind(&p.last_seen_system)
    .bind(p.recent_kills_24h as i64)
    .bind(p.systems_visited as i64)
    .execute(pool)
    .await?;
    Ok(())
}

/// Insert a raw event.
pub async fn insert_event(pool: &PgPool, e: &RawEvent) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO raw_events (event_type, timestamp_ms, data) VALUES ($1, $2, $3)")
        .bind(&e.event_type)
        .bind(e.timestamp_ms as i64)
        .bind(&e.data)
        .execute(pool)
        .await?;
    Ok(())
}

/// Save checkpoint cursor.
pub async fn save_checkpoint(pool: &PgPool, checkpoint: u64) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO checkpoint_cursor (id, last_checkpoint) VALUES (1, $1) \
         ON CONFLICT (id) DO UPDATE SET last_checkpoint = EXCLUDED.last_checkpoint",
    )
    .bind(checkpoint as i64)
    .execute(pool)
    .await?;
    Ok(())
}

/// Prune old events, keeping only the most recent N.
pub async fn prune_events(pool: &PgPool, keep: i64) -> Result<u64, sqlx::Error> {
    let result = sqlx::query(
        "DELETE FROM raw_events WHERE id NOT IN \
         (SELECT id FROM raw_events ORDER BY timestamp_ms DESC LIMIT $1)",
    )
    .bind(keep)
    .execute(pool)
    .await?;
    Ok(result.rows_affected())
}

#[derive(sqlx::FromRow)]
struct ProfileRow {
    character_item_id: i64,
    name: String,
    threat_score: i64,
    kill_count: i64,
    death_count: i64,
    bounty_count: i64,
    last_kill_timestamp: i64,
    last_seen_system: String,
    recent_kills_24h: i64,
    systems_visited: i64,
}

#[derive(sqlx::FromRow)]
struct EventRow {
    event_type: String,
    timestamp_ms: i64,
    data: serde_json::Value,
}
