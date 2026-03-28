mod config;
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
    let state = Arc::new(RwLock::new(AppState {
        sse_tx: Some(sse_tx.clone()),
        ..Default::default()
    }));

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

    // HTTP API + SSE
    let app = api::router(state.clone(), sse_tx);

    let addr = format!("0.0.0.0:{}", config.api_port);
    tracing::info!("API listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}
