use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;

use crate::types::{DataStore, RawEvent, ThreatProfile};

const MIGRATIONS: &[&str] = &[
    include_str!("../migrations/001_init.sql"),
    include_str!("../migrations/002_published_score.sql"),
    include_str!("../migrations/003_drop_published_score.sql"),
    include_str!("../migrations/004_discord_alert_channels.sql"),
    include_str!("../migrations/005_structure_cache.sql"),
    include_str!("../migrations/006_nullable_profile_name.sql"),
    include_str!("../migrations/007_recent_deaths.sql"),
];

/// Run all migrations on an existing pool.
pub async fn run_migrations(pool: &PgPool) -> Result<(), sqlx::Error> {
    for sql in MIGRATIONS {
        sqlx::raw_sql(sql).execute(pool).await?;
    }
    Ok(())
}

/// Connect to Postgres and run migrations.
pub async fn connect(database_url: &str) -> Result<PgPool, sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .connect(database_url)
        .await?;

    run_migrations(&pool).await?;

    tracing::info!("Database connected and migrations applied");
    Ok(pool)
}

/// Load all threat profiles and recent events into the live DataStore.
pub async fn load_into(pool: &PgPool, store: &mut DataStore) -> Result<(), sqlx::Error> {
    // Load profiles
    let rows = sqlx::query_as::<_, ProfileRow>(
        "SELECT character_item_id, name, threat_score, kill_count, death_count, \
         bounty_count, last_kill_timestamp, last_seen_system, recent_kills_24h, \
         recent_deaths_24h, systems_visited, tribe_id, tribe_name, last_seen_system_name \
         FROM threat_profiles",
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
                last_seen_system_name: r.last_seen_system_name,
                tribe_id: r.tribe_id,
                tribe_name: r.tribe_name,
                recent_kills_24h: r.recent_kills_24h as u64,
                recent_deaths_24h: r.recent_deaths_24h as u64,
                systems_visited: r.systems_visited as u64,
                ..Default::default()
            },
        );
    }

    // Load recent events (most recent 200)
    let events = sqlx::query_as::<_, EventRow>(
        "SELECT event_type, timestamp_ms, data FROM raw_events \
         ORDER BY timestamp_ms DESC LIMIT 1000",
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
        profiles = store.profiles.len(),
        events = store.recent_events.len(),
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
          recent_deaths_24h, systems_visited, tribe_id, tribe_name, last_seen_system_name, updated_at) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14, NOW()) \
         ON CONFLICT (character_item_id) DO UPDATE SET \
          name = EXCLUDED.name, \
          threat_score = EXCLUDED.threat_score, \
          kill_count = EXCLUDED.kill_count, \
          death_count = EXCLUDED.death_count, \
          bounty_count = EXCLUDED.bounty_count, \
          last_kill_timestamp = EXCLUDED.last_kill_timestamp, \
          last_seen_system = EXCLUDED.last_seen_system, \
          recent_kills_24h = EXCLUDED.recent_kills_24h, \
          recent_deaths_24h = EXCLUDED.recent_deaths_24h, \
          systems_visited = EXCLUDED.systems_visited, \
          tribe_id = EXCLUDED.tribe_id, \
          tribe_name = EXCLUDED.tribe_name, \
          last_seen_system_name = EXCLUDED.last_seen_system_name, \
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
    .bind(p.recent_deaths_24h as i64)
    .bind(p.systems_visited as i64)
    .bind(&p.tribe_id)
    .bind(&p.tribe_name)
    .bind(&p.last_seen_system_name)
    .execute(pool)
    .await?;
    Ok(())
}

/// Fetch the most recent kill/structure_destroyed events from DB.
pub async fn recent_kills(pool: &PgPool, limit: i64) -> Result<Vec<RawEvent>, sqlx::Error> {
    let rows = sqlx::query_as::<_, EventRow>(
        "SELECT event_type, timestamp_ms, data FROM raw_events \
         WHERE event_type IN ('kill', 'structure_destroyed') \
         ORDER BY timestamp_ms DESC LIMIT $1",
    )
    .bind(limit)
    .fetch_all(pool)
    .await?;
    Ok(rows
        .into_iter()
        .map(|r| RawEvent {
            event_type: r.event_type,
            timestamp_ms: r.timestamp_ms as u64,
            data: r.data,
        })
        .collect())
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
    name: Option<String>,
    threat_score: i64,
    kill_count: i64,
    death_count: i64,
    bounty_count: i64,
    last_kill_timestamp: i64,
    last_seen_system: String,
    last_seen_system_name: String,
    tribe_id: String,
    tribe_name: String,
    recent_kills_24h: i64,
    recent_deaths_24h: i64,
    systems_visited: i64,
}

#[derive(sqlx::FromRow)]
struct EventRow {
    event_type: String,
    timestamp_ms: i64,
    data: serde_json::Value,
}

/// Load character names from DB cache into the DataStore name_cache.
pub async fn load_character_names(
    pool: &PgPool,
    store: &mut DataStore,
) -> Result<usize, sqlx::Error> {
    let rows = sqlx::query_as::<_, (i64, String)>(
        "SELECT character_item_id, name FROM character_name_cache",
    )
    .fetch_all(pool)
    .await?;
    let count = rows.len();
    for (id, name) in rows {
        store.name_cache.insert(id as u64, name);
    }
    Ok(count)
}

/// Load gate names from DB cache into the DataStore.
pub async fn load_gate_names(pool: &PgPool, store: &mut DataStore) -> Result<usize, sqlx::Error> {
    let rows = sqlx::query_as::<_, (String, String)>("SELECT gate_id, name FROM gate_name_cache")
        .fetch_all(pool)
        .await?;
    let count = rows.len();
    for (gate_id, name) in rows {
        store.gate_name_cache.insert(gate_id, name);
    }
    Ok(count)
}

/// Save a gate name to the DB cache.
pub async fn upsert_gate_name(pool: &PgPool, gate_id: &str, name: &str) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO gate_name_cache (gate_id, name) VALUES ($1, $2) \
         ON CONFLICT (gate_id) DO UPDATE SET name = EXCLUDED.name, fetched_at = NOW()",
    )
    .bind(gate_id)
    .bind(name)
    .execute(pool)
    .await?;
    Ok(())
}

/// Load all guild alert channel mappings from the database.
pub async fn load_alert_channels(
    pool: &PgPool,
) -> Result<std::collections::HashMap<u64, u64>, sqlx::Error> {
    let rows =
        sqlx::query_as::<_, (i64, i64)>("SELECT guild_id, channel_id FROM discord_alert_channels")
            .fetch_all(pool)
            .await?;
    Ok(rows
        .into_iter()
        .map(|(g, c)| (g as u64, c as u64))
        .collect())
}

/// Set the alert channel for a guild.
pub async fn set_alert_channel(
    pool: &PgPool,
    guild_id: u64,
    channel_id: u64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO discord_alert_channels (guild_id, channel_id) VALUES ($1, $2) \
         ON CONFLICT (guild_id) DO UPDATE SET channel_id = EXCLUDED.channel_id, created_at = NOW()",
    )
    .bind(guild_id as i64)
    .bind(channel_id as i64)
    .execute(pool)
    .await?;
    Ok(())
}

/// Remove the alert channel for a guild.
pub async fn clear_alert_channel(pool: &PgPool, guild_id: u64) -> Result<(), sqlx::Error> {
    sqlx::query("DELETE FROM discord_alert_channels WHERE guild_id = $1")
        .bind(guild_id as i64)
        .execute(pool)
        .await?;
    Ok(())
}

/// Load structure type cache from DB into the DataStore.
pub async fn load_structure_types(
    pool: &PgPool,
    store: &mut DataStore,
) -> Result<usize, sqlx::Error> {
    let rows = sqlx::query_as::<_, (i64, i64)>("SELECT item_id, type_id FROM structure_type_cache")
        .fetch_all(pool)
        .await?;
    let count = rows.len();
    for (item_id, type_id) in rows {
        store
            .structure_type_cache
            .insert(item_id as u64, type_id as u64);
    }
    Ok(count)
}

/// Save a structure item_id → type_id mapping to the DB cache.
pub async fn upsert_structure_type(
    pool: &PgPool,
    item_id: u64,
    type_id: u64,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO structure_type_cache (item_id, type_id) VALUES ($1, $2) \
         ON CONFLICT (item_id) DO UPDATE SET type_id = EXCLUDED.type_id, fetched_at = NOW()",
    )
    .bind(item_id as i64)
    .bind(type_id as i64)
    .execute(pool)
    .await?;
    Ok(())
}

/// Save a character name to the DB cache.
pub async fn upsert_character_name(
    pool: &PgPool,
    character_item_id: u64,
    name: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO character_name_cache (character_item_id, name) VALUES ($1, $2) \
         ON CONFLICT (character_item_id) DO UPDATE SET name = EXCLUDED.name, fetched_at = NOW()",
    )
    .bind(character_item_id as i64)
    .bind(name)
    .execute(pool)
    .await?;
    Ok(())
}
