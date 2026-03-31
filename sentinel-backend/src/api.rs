//! HTTP API + SSE streaming for the dashboard.
//! Serves combined demo + live data — the client picks which to display.

use axum::{
    Router,
    extract::State,
    response::{
        IntoResponse, Json,
        sse::{Event, KeepAlive, Sse},
    },
    routing::get,
};
use futures::stream::Stream;
use std::sync::Arc;
use tokio::sync::RwLock;
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;
use tower_http::cors::CorsLayer;

use crate::config::AppConfig;
use crate::threat_engine;
use crate::types::AppState;

type SharedState = Arc<RwLock<AppState>>;

/// Build the Axum router. Mounts `/api/data`, `/api/events/stream`, and `/api/health`.
/// CORS is permissive — the frontend and backend run on different origins in all environments.
pub fn router(
    config: AppConfig,
    state: SharedState,
    sse_tx: tokio::sync::broadcast::Sender<String>,
) -> Router {
    Router::new()
        .route("/api/data", get(get_combined_data))
        .route("/api/events/stream", get(sse_stream))
        .route("/api/health", get(health))
        .layer(CorsLayer::permissive())
        .with_state(AppRouterState {
            config,
            state,
            sse_tx,
        })
}

#[derive(Clone)]
struct AppRouterState {
    config: AppConfig,
    state: SharedState,
    sse_tx: tokio::sync::broadcast::Sender<String>,
}

/// GET /api/data — combined demo + live data in one response
async fn get_combined_data(State(app): State<AppRouterState>) -> impl IntoResponse {
    let state = app.state.read().await;

    let enrich = |profiles: &std::collections::HashMap<u64, crate::types::ThreatProfile>| {
        let mut enriched: Vec<serde_json::Value> = profiles
            .values()
            .map(|p| {
                let mut v = serde_json::to_value(p).unwrap_or_default();
                v["titles"] = serde_json::json!(threat_engine::earned_titles(p));
                v["threat_tier"] = serde_json::json!(threat_engine::threat_tier(p.threat_score));
                v
            })
            .collect();
        enriched.sort_by(|a, b| b["threat_score"].as_u64().cmp(&a["threat_score"].as_u64()));
        enriched
    };

    let demo_profiles = enrich(&state.demo.profiles);
    let demo_events: Vec<_> = state.demo.recent_events.iter().cloned().collect();
    let demo_new_pilots: Vec<_> = state.demo.new_pilot_events.iter().cloned().collect();

    let live_profiles = enrich(&state.live.profiles);
    let live_events: Vec<_> = state.live.recent_events.iter().cloned().collect();
    let live_new_pilots: Vec<_> = state.live.new_pilot_events.iter().cloned().collect();

    // Name lookup map for the frontend — use the full name_cache (live + demo) so
    // kill victims and other non-tracked participants resolve correctly.
    // Filter out "" entries (permanently-unresolvable chars marked during scan).
    let name_map: std::collections::HashMap<u64, &str> = state
        .live
        .name_cache
        .iter()
        .chain(state.demo.name_cache.iter())
        .filter(|(_, v)| !v.is_empty())
        .map(|(k, v)| (*k, v.as_str()))
        .collect();

    let system_map = &state.live.system_name_cache;

    let cap = app.config.max_recent_events;
    let mut live_stats = state.live.compute_stats(cap);
    // Resolve top system name from cache
    if !live_stats.top_system.is_empty() {
        if let Some(name) = system_map.get(&live_stats.top_system) {
            if !name.is_empty() {
                live_stats.top_system = name.clone();
            }
        }
    }

    Json(serde_json::json!({
        "demo": {
            "threats": demo_profiles,
            "events": demo_events,
            "new_pilots": demo_new_pilots,
            "stats": state.demo.compute_stats(cap),
        },
        "live": {
            "threats": live_profiles,
            "events": live_events,
            "new_pilots": live_new_pilots,
            "stats": live_stats,
        },
        "names": name_map,
        "systems": system_map,
    }))
}

/// GET /api/events/stream — SSE stream of real-time events (both modes)
async fn sse_stream(
    State(app): State<AppRouterState>,
) -> Sse<impl Stream<Item = Result<Event, std::convert::Infallible>>> {
    let rx = app.sse_tx.subscribe();
    let stream = BroadcastStream::new(rx).filter_map(|result: Result<String, _>| {
        result
            .ok()
            .map(|data| Ok::<_, std::convert::Infallible>(Event::default().data(data)))
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::config::LogFormat;
    use crate::types::{AppState, RawEvent, ThreatProfile};
    use axum::body::Body;
    use axum::http::{Request, StatusCode};
    use tower::ServiceExt;

    fn test_config() -> AppConfig {
        AppConfig {
            sui_grpc_url: "http://unused".into(),
            sui_graphql_url: "http://unused".into(),
            sentinel_package_id: "0x0".into(),
            threat_registry_id: "0x0".into(),
            publisher_private_key: "unused".into(),
            world_package_id: "0x0".into(),
            bounty_board_package_id: "0x0".into(),
            publish_interval_ms: 60000,
            publish_score_threshold_bp: 500,
            api_port: 3001,
            database_url: "unused".into(),
            world_api_url: "http://unused".into(),
            sentinel_log_level: tracing::Level::INFO,
            crates_log_level: tracing::Level::WARN,
            log_format: LogFormat::Pretty,
            discord_token: "unused".into(),
            max_recent_events: 1000,
        }
    }

    fn test_app() -> (Router, tokio::sync::broadcast::Sender<String>) {
        let (sse_tx, _) = tokio::sync::broadcast::channel::<String>(16);
        let state = Arc::new(RwLock::new(AppState {
            sse_tx: Some(sse_tx.clone()),
            ..Default::default()
        }));
        (router(test_config(), state, sse_tx.clone()), sse_tx)
    }

    fn test_app_with_data() -> (Router, tokio::sync::broadcast::Sender<String>) {
        let (sse_tx, _) = tokio::sync::broadcast::channel::<String>(16);
        let mut state = AppState {
            sse_tx: Some(sse_tx.clone()),
            last_checkpoint: Some(42),
            ..Default::default()
        };
        state.demo.profiles.insert(
            1,
            ThreatProfile {
                character_item_id: 1,
                name: Some("Test Pilot".to_string()),
                threat_score: 5000,
                kill_count: 10,
                ..Default::default()
            },
        );
        state.live.profiles.insert(
            2,
            ThreatProfile {
                character_item_id: 2,
                name: Some("Live Pilot".to_string()),
                threat_score: 3000,
                ..Default::default()
            },
        );
        let sse = state.sse_tx.clone();
        state.demo.push_event(
            RawEvent {
                event_type: "kill".into(),
                timestamp_ms: 1000,
                data: serde_json::json!({}),
            },
            &sse,
            1000,
        );

        let shared = Arc::new(RwLock::new(state));
        (router(test_config(), shared, sse_tx.clone()), sse_tx)
    }

    #[tokio::test]
    async fn health_returns_ok() {
        let (app, _) = test_app();
        let resp = app
            .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["status"], "ok");
        assert_eq!(json["demo_profiles"], 0);
        assert_eq!(json["live_profiles"], 0);
    }

    #[tokio::test]
    async fn health_reflects_state() {
        let (app, _) = test_app_with_data();
        let resp = app
            .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["demo_profiles"], 1);
        assert_eq!(json["live_profiles"], 1);
        assert_eq!(json["last_checkpoint"], 42);
    }

    #[tokio::test]
    async fn data_returns_combined_response() {
        let (app, _) = test_app_with_data();
        let resp = app
            .oneshot(Request::get("/api/data").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        // Demo data
        assert_eq!(json["demo"]["threats"].as_array().unwrap().len(), 1);
        assert_eq!(json["demo"]["threats"][0]["name"], "Test Pilot");
        assert_eq!(json["demo"]["events"].as_array().unwrap().len(), 1);
        assert!(json["demo"]["stats"]["total_tracked"].as_u64().unwrap() > 0);

        // Live data
        assert_eq!(json["live"]["threats"].as_array().unwrap().len(), 1);
        assert_eq!(json["live"]["threats"][0]["name"], "Live Pilot");
    }

    #[tokio::test]
    async fn data_empty_state() {
        let (app, _) = test_app();
        let resp = app
            .oneshot(Request::get("/api/data").body(Body::empty()).unwrap())
            .await
            .unwrap();

        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(json["demo"]["threats"].as_array().unwrap().len(), 0);
        assert_eq!(json["live"]["threats"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn data_sorts_by_threat_score_descending() {
        let (sse_tx, _) = tokio::sync::broadcast::channel::<String>(16);
        let mut state = AppState {
            sse_tx: Some(sse_tx.clone()),
            ..Default::default()
        };
        state.demo.profiles.insert(
            1,
            ThreatProfile {
                character_item_id: 1,
                threat_score: 1000,
                ..Default::default()
            },
        );
        state.demo.profiles.insert(
            2,
            ThreatProfile {
                character_item_id: 2,
                threat_score: 5000,
                ..Default::default()
            },
        );
        state.demo.profiles.insert(
            3,
            ThreatProfile {
                character_item_id: 3,
                threat_score: 3000,
                ..Default::default()
            },
        );

        let shared = Arc::new(RwLock::new(state));
        let app = router(test_config(), shared, sse_tx.clone());

        let resp = app
            .oneshot(Request::get("/api/data").body(Body::empty()).unwrap())
            .await
            .unwrap();
        let body = axum::body::to_bytes(resp.into_body(), usize::MAX)
            .await
            .unwrap();
        let json: serde_json::Value = serde_json::from_slice(&body).unwrap();

        let scores: Vec<u64> = json["demo"]["threats"]
            .as_array()
            .unwrap()
            .iter()
            .map(|t| t["threat_score"].as_u64().unwrap())
            .collect();
        assert_eq!(scores, vec![5000, 3000, 1000]);
    }

    #[tokio::test]
    async fn sse_stream_returns_event_stream_content_type() {
        let (app, _sse_tx) = test_app();

        let resp = app
            .oneshot(
                Request::get("/api/events/stream")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(resp.status(), StatusCode::OK);
        assert_eq!(
            resp.headers().get("content-type").unwrap(),
            "text/event-stream"
        );
    }

    #[tokio::test]
    async fn cors_headers_present() {
        let (app, _) = test_app();
        let resp = app
            .oneshot(Request::get("/api/health").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert!(resp.headers().contains_key("access-control-allow-origin"));
    }
}
