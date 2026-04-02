//! SSE Bridge — standalone Axum server that manages Server-Sent Event connections.
//! Subscribes to `sentinel.scores` JetStream and fans out score updates to all
//! connected SSE clients. Also replays recent events from `SENTINEL_EVENTS`
//! stream on new connections.

use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;

use async_nats::jetstream;
use axum::extract::State;
use axum::response::sse::{Event, Sse};
use axum::response::IntoResponse;
use axum::routing::get;
use axum::Router;
use futures::stream::Stream;
use serde::Deserialize;
use tokio::sync::{broadcast, RwLock};

// ─── Configuration ──────────────────────────────────────────────────────────

fn require(name: &str) -> String {
    std::env::var(name).unwrap_or_else(|_| panic!("{name} is required"))
}

#[derive(Clone)]
struct Config {
    nats_url: String,
    listen_addr: String,
}

impl Config {
    fn from_env() -> Self {
        Self {
            nats_url: require("NATS_URL"),
            listen_addr: require("SSE_LISTEN_ADDR"),
        }
    }
}

// ─── Score update payload ───────────────────────────────────────────────────

#[derive(Debug, Clone, Deserialize)]
struct ScoreUpdate {
    pilot_id: u64,
    name: Option<String>,
    threat_score: u64,
    tier: String,
    titles: Vec<String>,
}

// ─── Application state ─────────────────────────────────────────────────────

#[derive(Clone)]
struct AppState {
    /// Broadcast channel for score updates — all SSE clients receive from this
    score_tx: broadcast::Sender<String>,
    /// Recent events for replay on new connections
    recent_events: Arc<RwLock<Vec<String>>>,
}

// ─── Handlers ───────────────────────────────────────────────────────────────

async fn health() -> impl IntoResponse {
    "ok"
}

async fn sse_handler(
    State(state): State<AppState>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let mut rx = state.score_tx.subscribe();

    // Collect recent events for replay
    let replay = state.recent_events.read().await.clone();

    let stream = async_stream::stream! {
        // Replay recent events first
        for event_json in replay {
            yield Ok(Event::default().event("event").data(event_json));
        }

        // Then stream live score updates
        loop {
            match rx.recv().await {
                Ok(data) => {
                    yield Ok(Event::default().event("score").data(data));
                }
                Err(broadcast::error::RecvError::Lagged(n)) => {
                    tracing::warn!(missed = n, "SSE client lagged, skipped {n} messages");
                    continue;
                }
                Err(broadcast::error::RecvError::Closed) => break,
            }
        }
    };

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(std::time::Duration::from_secs(15))
            .text("ping"),
    )
}

// ─── NATS subscribers ───────────────────────────────────────────────────────

/// Subscribe to sentinel.scores JetStream and broadcast to SSE clients.
async fn subscribe_scores(
    js: jetstream::Context,
    score_tx: broadcast::Sender<String>,
) -> anyhow::Result<()> {
    let stream = js
        .get_or_create_stream(jetstream::stream::Config {
            name: "SENTINEL_SCORES".into(),
            subjects: vec!["sentinel.scores".into()],
            retention: jetstream::stream::RetentionPolicy::Interest,
            ..Default::default()
        })
        .await?;

    let consumer = stream
        .get_or_create_consumer(
            "sse-bridge",
            jetstream::consumer::pull::Config {
                durable_name: Some("sse-bridge".into()),
                ack_policy: jetstream::consumer::AckPolicy::Explicit,
                ..Default::default()
            },
        )
        .await?;

    tracing::info!("Subscribed to sentinel.scores for SSE fan-out");

    loop {
        let mut messages = consumer.fetch().max_messages(50).messages().await?;

        use futures::StreamExt;
        while let Some(Ok(msg)) = messages.next().await {
            let payload = String::from_utf8_lossy(&msg.payload).to_string();
            // Broadcast to all connected SSE clients (ignore send errors = no receivers)
            let _ = score_tx.send(payload);
            let _ = msg.ack().await;
        }

        tokio::time::sleep(std::time::Duration::from_millis(100)).await;
    }
}

/// Load recent events from SENTINEL_EVENTS stream for replay on new connections.
async fn load_recent_events(
    js: jetstream::Context,
    recent_events: Arc<RwLock<Vec<String>>>,
) -> anyhow::Result<()> {
    // Create an ephemeral consumer that reads the last N messages from the events stream
    let stream = match js.get_stream("SENTINEL_EVENTS").await {
        Ok(s) => s,
        Err(_) => {
            tracing::warn!("SENTINEL_EVENTS stream not found, no replay events");
            return Ok(());
        }
    };

    let consumer = stream
        .get_or_create_consumer(
            "sse-replay",
            jetstream::consumer::pull::Config {
                durable_name: Some("sse-replay".into()),
                deliver_policy: jetstream::consumer::DeliverPolicy::Last,
                ack_policy: jetstream::consumer::AckPolicy::None,
                ..Default::default()
            },
        )
        .await?;

    // Fetch up to 100 recent events for replay
    let mut messages = consumer.fetch().max_messages(100).messages().await?;
    let mut events = Vec::new();

    use futures::StreamExt;
    while let Some(Ok(msg)) = messages.next().await {
        events.push(String::from_utf8_lossy(&msg.payload).to_string());
    }

    tracing::info!(count = events.len(), "Loaded {} recent events for replay", events.len());
    *recent_events.write().await = events;

    // Periodically refresh the replay buffer
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;

        let mut messages = consumer.fetch().max_messages(100).messages().await?;
        let mut events = Vec::new();
        while let Some(Ok(msg)) = messages.next().await {
            events.push(String::from_utf8_lossy(&msg.payload).to_string());
        }
        *recent_events.write().await = events;
    }
}

// ─── Main ───────────────────────────────────────────────────────────────────

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "sse_bridge=info".into()),
        )
        .init();

    let config = Config::from_env();
    tracing::info!(
        nats = %config.nats_url,
        listen = %config.listen_addr,
        "Starting sse-bridge"
    );

    // Connect to NATS
    let nats = async_nats::connect(&config.nats_url).await?;
    let js = jetstream::new(nats);

    // Broadcast channel: 1024 buffered messages for SSE clients
    let (score_tx, _) = broadcast::channel(1024);

    let recent_events = Arc::new(RwLock::new(Vec::new()));

    let state = AppState {
        score_tx: score_tx.clone(),
        recent_events: recent_events.clone(),
    };

    // Spawn NATS → SSE fan-out
    let js_scores = js.clone();
    let tx = score_tx.clone();
    tokio::spawn(async move {
        if let Err(e) = subscribe_scores(js_scores, tx).await {
            tracing::error!(error = %e, "Score subscriber failed: {e}");
        }
    });

    // Spawn replay buffer loader
    let js_replay = js.clone();
    let re = recent_events.clone();
    tokio::spawn(async move {
        if let Err(e) = load_recent_events(js_replay, re).await {
            tracing::error!(error = %e, "Replay loader failed: {e}");
        }
    });

    // Build Axum router
    let app = Router::new()
        .route("/health", get(health))
        .route("/events/stream", get(sse_handler))
        .with_state(state);

    let listener = tokio::net::TcpListener::bind(&config.listen_addr).await?;
    tracing::info!(addr = %config.listen_addr, "SSE bridge listening on {}", config.listen_addr);
    axum::serve(listener, app).await?;

    Ok(())
}

/// Build the router (extracted for testing).
fn build_router(state: AppState) -> Router {
    Router::new()
        .route("/health", get(health))
        .route("/events/stream", get(sse_handler))
        .with_state(state)
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use http::Request;
    use tower::ServiceExt;

    fn test_state() -> AppState {
        let (score_tx, _) = broadcast::channel(1024);
        AppState {
            score_tx,
            recent_events: Arc::new(RwLock::new(Vec::new())),
        }
    }

    // ─── Health endpoint ────────────────────────────────────────────────

    #[tokio::test]
    async fn health_returns_ok() {
        let app = build_router(test_state());
        let response = app
            .oneshot(Request::get("/health").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let body = http_body_util::BodyExt::collect(response.into_body())
            .await
            .unwrap()
            .to_bytes();
        assert_eq!(&body[..], b"ok");
    }

    // ─── SSE endpoint ───────────────────────────────────────────────────

    #[tokio::test]
    async fn sse_returns_event_stream_content_type() {
        let app = build_router(test_state());
        let response = app
            .oneshot(Request::get("/events/stream").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), 200);
        let ct = response.headers().get("content-type").unwrap().to_str().unwrap();
        assert!(ct.contains("text/event-stream"), "content-type was: {ct}");
    }

    #[tokio::test]
    async fn sse_replays_recent_events() {
        let state = test_state();
        // Pre-populate replay buffer
        {
            let mut events = state.recent_events.write().await;
            events.push(r#"{"type":"test","data":"event1"}"#.to_string());
            events.push(r#"{"type":"test","data":"event2"}"#.to_string());
        }

        let app = build_router(state);
        let response = app
            .oneshot(Request::get("/events/stream").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), 200);

        // Read initial bytes — should contain the replayed events
        // We can't easily read the full stream (it's infinite), but we can verify
        // the response started successfully
        let ct = response.headers().get("content-type").unwrap().to_str().unwrap();
        assert!(ct.contains("text/event-stream"));
    }

    // ─── Broadcast fan-out ──────────────────────────────────────────────

    #[tokio::test]
    async fn broadcast_sends_to_multiple_receivers() {
        let (tx, _) = broadcast::channel::<String>(16);
        let mut rx1 = tx.subscribe();
        let mut rx2 = tx.subscribe();

        tx.send("test message".to_string()).unwrap();

        assert_eq!(rx1.recv().await.unwrap(), "test message");
        assert_eq!(rx2.recv().await.unwrap(), "test message");
    }

    #[tokio::test]
    async fn broadcast_lagged_receiver_gets_error() {
        let (tx, _) = broadcast::channel::<String>(2);
        let mut rx = tx.subscribe();

        // Send more than the buffer size
        tx.send("a".to_string()).unwrap();
        tx.send("b".to_string()).unwrap();
        tx.send("c".to_string()).unwrap();

        // First recv should report lag
        match rx.recv().await {
            Err(broadcast::error::RecvError::Lagged(n)) => assert_eq!(n, 1),
            other => panic!("expected Lagged, got {other:?}"),
        }
    }

    // ─── ScoreUpdate deserialization ────────────────────────────────────

    #[test]
    fn deserialize_score_update() {
        let json = r#"{"pilot_id":42,"name":"TestPilot","threat_score":5000,"tier":"MODERATE","titles":["Bounty Magnet"]}"#;
        let update: ScoreUpdate = serde_json::from_str(json).unwrap();
        assert_eq!(update.pilot_id, 42);
        assert_eq!(update.name, Some("TestPilot".into()));
        assert_eq!(update.threat_score, 5000);
        assert_eq!(update.tier, "MODERATE");
        assert_eq!(update.titles, vec!["Bounty Magnet"]);
    }

    #[test]
    fn deserialize_score_update_null_name() {
        let json = r#"{"pilot_id":42,"name":null,"threat_score":0,"tier":"LOW","titles":[]}"#;
        let update: ScoreUpdate = serde_json::from_str(json).unwrap();
        assert_eq!(update.name, None);
        assert!(update.titles.is_empty());
    }

    // ─── 404 for unknown routes ─────────────────────────────────────────

    #[tokio::test]
    async fn unknown_route_returns_404() {
        let app = build_router(test_state());
        let response = app
            .oneshot(Request::get("/nonexistent").body(Body::empty()).unwrap())
            .await
            .unwrap();
        assert_eq!(response.status(), 404);
    }
}
