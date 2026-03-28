mod config;
mod db;
mod demo;
mod google_rpc;
mod grpc;
mod names;
mod threat_engine;
mod publisher;
mod api;
mod types;

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

    tracing_subscriber::fmt()
        .with_env_filter(
            EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| EnvFilter::new("sentinel_backend=info,tower_http=info")),
        )
        .init();

    let config = AppConfig::from_env()?;
    tracing::info!("SENTINEL starting — streaming from {}", config.sui_grpc_url);

    let (sse_tx, _) = tokio::sync::broadcast::channel::<String>(256);

    // Connect to Postgres if DATABASE_URL is set
    let db_pool = if let Some(ref url) = config.database_url {
        match db::connect(url).await {
            Ok(pool) => {
                tracing::info!("Postgres persistence enabled");
                Some(pool)
            }
            Err(e) => {
                tracing::warn!("Failed to connect to Postgres, running without persistence: {e}");
                None
            }
        }
    } else {
        tracing::info!("No DATABASE_URL — running in-memory only");
        None
    };

    let state = Arc::new(RwLock::new(AppState {
        sse_tx: Some(sse_tx.clone()),
        db: db_pool.clone(),
        ..Default::default()
    }));

    // Load persisted data if DB is available
    if let Some(ref pool) = db_pool {
        let mut s = state.write().await;
        if let Err(e) = db::load_into(pool, &mut s.live).await {
            tracing::warn!("Failed to load persisted data: {e}");
        }
        if let Ok(Some(cp)) = db::load_checkpoint(pool).await {
            tracing::info!("Resuming from checkpoint {cp}");
            s.last_checkpoint = Some(cp);
        }
    }

    // Seed demo data
    demo::seed_demo_data(state.clone()).await;

    // Demo event loop (always running)
    let demo_state = state.clone();
    tokio::spawn(async move {
        demo::demo_event_loop(demo_state).await;
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

    // Database sync loop (persists dirty profiles + events)
    if let Some(pool) = db_pool {
        let sync_state = state.clone();
        tokio::spawn(async move {
            db_sync_loop(pool, sync_state).await;
        });
    }

    // HTTP API + SSE
    let app = api::router(state.clone(), sse_tx);

    let addr = format!("0.0.0.0:{}", config.api_port);
    tracing::info!("API listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Background loop that flushes dirty live profiles and checkpoint to Postgres.
async fn db_sync_loop(pool: sqlx::PgPool, state: Arc<RwLock<AppState>>) {
    let mut last_event_count: usize = 0;

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

        // Prune old events every cycle
        if last_event_count > 500 {
            if let Ok(pruned) = db::prune_events(&pool, 1000).await {
                if pruned > 0 {
                    tracing::debug!("Pruned {pruned} old events from database");
                }
            }
        }

        if !dirty_profiles.is_empty() || !new_events.is_empty() {
            tracing::debug!(
                "DB sync: {} profiles, {} events flushed",
                dirty_profiles.len(),
                new_events.len()
            );
        }
    }
}
