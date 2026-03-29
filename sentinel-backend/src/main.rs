mod api;
mod config;
mod db;
mod demo;
#[cfg(feature = "discord")]
mod discord;
mod google_rpc;
mod grpc;
mod historical;
mod names;
mod publisher;
pub mod sui_client;
mod threat_engine;
mod types;
mod world_api;

use std::sync::Arc;
use tokio::sync::RwLock;
use tracing_subscriber::EnvFilter;

use crate::config::AppConfig;
use crate::types::AppState;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load .env from project root
    dotenvy::from_path("../.env").ok();
    dotenvy::dotenv().ok();

    let config = AppConfig::from_env()?;

    let subscriber = tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| {
            EnvFilter::new(format!(
                "{},sentinel_backend={}",
                config.crates_log_level, config.sentinel_log_level
            ))
        }))
        .with_ansi(matches!(
            config.log_format,
            crate::config::LogFormat::Pretty
        ));
    match config.log_format {
        crate::config::LogFormat::Pretty => subscriber.init(),
        crate::config::LogFormat::Json => subscriber.json().init(),
    }
    tracing::info!("SENTINEL starting — streaming from {}", config.sui_grpc_url);

    let (sse_tx, _) = tokio::sync::broadcast::channel::<String>(256);

    let db_pool = db::connect(&config.database_url).await?;
    tracing::info!("Postgres connected");

    let state = Arc::new(RwLock::new(AppState {
        sse_tx: Some(sse_tx.clone()),
        ..Default::default()
    }));

    // World API client (for resolving system names + tribes)
    let world_client = Arc::new(RwLock::new(
        world_api::WorldApiClient::new(&config.world_api_url, &db_pool).await,
    ));

    // Load persisted data under a scoped write lock.
    // The block ensures the lock is released before spawning background tasks.
    {
        let mut s = state.write().await;
        db::load_into(&db_pool, &mut s.live).await?;
        let name_count = db::load_character_names(&db_pool, &mut s.live).await?;
        let gate_count = db::load_gate_names(&db_pool, &mut s.live).await?;
        let struct_count = db::load_structure_types(&db_pool, &mut s.live).await?;
        tracing::info!(
            "Loaded {name_count} character names, {gate_count} gate names, \
             {struct_count} structure type mappings from DB cache"
        );
        if let Some(cp) = db::load_checkpoint(&db_pool).await? {
            tracing::info!("Resuming from checkpoint {cp}");
            s.last_checkpoint = Some(cp);
        }
    }

    // Seed demo data immediately (so dashboard is usable while historical loads)
    demo::seed_demo_data(state.clone()).await;

    // Demo event loop (always running)
    let demo_state = state.clone();
    tokio::spawn(async move {
        demo::demo_event_loop(demo_state).await;
    });

    // Load historical data in background (API serves demo data immediately)
    let hist_state = state.clone();
    let hist_config = config.clone();
    let hist_pool = db_pool.clone();
    let hist_world = world_client.clone();
    tokio::spawn(async move {
        historical::load_all(hist_config, hist_state, hist_pool, hist_world).await;
    });

    // gRPC checkpoint streamer (always running for live data)
    let streamer_state = state.clone();
    let streamer_config = config.clone();
    tokio::spawn(async move {
        grpc::stream_checkpoints(streamer_config, streamer_state).await;
    });

    // On-chain publisher
    let publisher_state = state.clone();
    let publisher_config = config.clone();
    tokio::spawn(async move {
        publisher::publish_loop(publisher_config, publisher_state).await;
    });

    // Name resolver
    let names_state = state.clone();
    let names_config = config.clone();
    tokio::spawn(async move {
        names::name_resolver_loop(names_config, names_state).await;
    });

    // Discord bot (optional, requires --features discord + DISCORD_TOKEN)
    #[cfg(feature = "discord")]
    let dc_config = config.clone();
    #[cfg(feature = "discord")]
    if dc_config.discord_token.is_some() {
        let dc_state = state.clone();
        let dc_sse = sse_tx.clone();
        let dc_pool = db_pool.clone();
        tokio::spawn(async move {
            discord::run_discord_bot(dc_config, dc_state, dc_sse, dc_pool).await;
        });
    }

    // Metadata resolver (system names + tribe affiliations via World API)
    let meta_state = state.clone();
    let meta_client = world_client.clone();
    let meta_pool = db_pool.clone();
    let api_port = config.api_port;
    tokio::spawn(async move {
        world_api::metadata_resolver_loop(
            meta_client,
            meta_state,
            meta_pool,
            config.sui_grpc_url.clone(),
            config.world_package_id.clone(),
        )
        .await;
    });

    // Database sync loop (persists dirty profiles + events)
    let initial_event_count = state.read().await.live.recent_events.len();
    let sync_state = state.clone();
    let sync_pool = db_pool.clone();
    tokio::spawn(async move {
        db_sync_loop(sync_pool, sync_state, initial_event_count).await;
    });

    // HTTP API + SSE
    let app = api::router(state.clone(), sse_tx);

    let addr = format!("0.0.0.0:{api_port}");
    tracing::info!("API listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Background loop that flushes dirty live profiles and checkpoint to Postgres.
async fn db_sync_loop(pool: sqlx::PgPool, state: Arc<RwLock<AppState>>, initial_events: usize) {
    let mut last_event_count: usize = initial_events;
    let mut persisted_gates: std::collections::HashSet<String> = {
        // Pre-populate with gates already in DB (loaded at startup)
        let s = state.read().await;
        s.live.gate_name_cache.keys().cloned().collect()
    };

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(5)).await;

        // Collect dirty profiles and new events under a short lock
        let (dirty_profiles, new_events, checkpoint) = {
            let mut s = state.write().await;
            let dirty: Vec<_> = s
                .live
                .profiles
                .values_mut()
                .filter(|p| p.dirty)
                .map(|p| {
                    p.dirty = false;
                    p.clone()
                })
                .collect();

            let current_count = s.live.recent_events.len();
            let new: Vec<_> = if current_count > last_event_count {
                s.live
                    .recent_events
                    .iter()
                    .take(current_count - last_event_count)
                    .cloned()
                    .collect()
            } else {
                Vec::new()
            };

            (dirty, new, s.last_checkpoint)
        };

        // Write outside the lock
        for p in &dirty_profiles {
            if let Err(e) = db::upsert_profile(&pool, p).await {
                tracing::warn!("Failed to persist profile {}: {e}", p.character_item_id);
            }
        }

        for e in &new_events {
            if let Err(e) = db::insert_event(&pool, e).await {
                tracing::warn!("Failed to persist event: {e}");
            }
        }

        last_event_count += new_events.len();

        if let Some(cp) = checkpoint {
            if let Err(e) = db::save_checkpoint(&pool, cp).await {
                tracing::warn!("Failed to save checkpoint: {e}");
            }
        }

        // Persist any new gate names
        {
            let s = state.read().await;
            for (gate_id, name) in &s.live.gate_name_cache {
                if !persisted_gates.contains(gate_id) {
                    if let Err(e) = db::upsert_gate_name(&pool, gate_id, name).await {
                        tracing::warn!("Failed to persist gate name {gate_id}: {e}");
                    } else {
                        persisted_gates.insert(gate_id.clone());
                    }
                }
            }
        }

        // Prune old events every cycle
        if last_event_count > 500 {
            if let Ok(pruned) = db::prune_events(&pool, 1000).await {
                if pruned > 0 {
                    tracing::debug!("Pruned {pruned} old events from database");
                }
            }
        }

        if !dirty_profiles.is_empty() || !new_events.is_empty() {
            tracing::info!(
                "DB sync: {} profiles, {} events flushed",
                dirty_profiles.len(),
                new_events.len()
            );
        }
    }
}
