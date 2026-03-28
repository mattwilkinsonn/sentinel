//! HTTP API + SSE streaming for the dashboard.
//! Serves combined demo + live data — the client picks which to display.

use std::sync::Arc;
use axum::{
    Router,
    extract::State,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Json,
    },
    routing::get,
};
use futures::stream::Stream;
use tokio::sync::RwLock;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tower_http::cors::CorsLayer;

use crate::types::AppState;

type SharedState = Arc<RwLock<AppState>>;

pub fn router(state: SharedState, sse_tx: tokio::sync::broadcast::Sender<String>) -> Router {
    Router::new()
        .route("/api/data", get(get_combined_data))
        .route("/api/events/stream", get(sse_stream))
        .route("/api/health", get(health))
        .layer(CorsLayer::permissive())
        .with_state(AppRouterState {
            state,
            sse_tx,
        })
}

#[derive(Clone)]
struct AppRouterState {
    state: SharedState,
    sse_tx: tokio::sync::broadcast::Sender<String>,
}

/// GET /api/data — combined demo + live data in one response
async fn get_combined_data(State(app): State<AppRouterState>) -> impl IntoResponse {
    let state = app.state.read().await;

    let mut demo_profiles: Vec<_> = state.demo.profiles.values().cloned().collect();
    demo_profiles.sort_by(|a, b| b.threat_score.cmp(&a.threat_score));
    let demo_events: Vec<_> = state.demo.recent_events.iter().take(200).cloned().collect();

    let mut live_profiles: Vec<_> = state.live.profiles.values().cloned().collect();
    live_profiles.sort_by(|a, b| b.threat_score.cmp(&a.threat_score));
    let live_events: Vec<_> = state.live.recent_events.iter().take(200).cloned().collect();

    Json(serde_json::json!({
        "demo": {
            "threats": demo_profiles,
            "events": demo_events,
            "stats": state.demo.compute_stats(),
        },
        "live": {
            "threats": live_profiles,
            "events": live_events,
            "stats": state.live.compute_stats(),
        }
    }))
}

/// GET /api/events/stream — SSE stream of real-time events (both modes)
async fn sse_stream(
    State(app): State<AppRouterState>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = app.sse_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result: Result<String, _>| {
        result.ok().map(|data| Ok::<_, std::convert::Infallible>(Event::default().data(data)))
    });
    Sse::new(stream).keep_alive(KeepAlive::default())
}

/// GET /api/health — liveness check
async fn health(State(app): State<AppRouterState>) -> impl IntoResponse {
    let state = app.state.read().await;
    Json(serde_json::json!({
        "status": "ok",
        "demo_profiles": state.demo.profiles.len(),
        "live_profiles": state.live.profiles.len(),
        "last_checkpoint": state.last_checkpoint,
    }))
}
