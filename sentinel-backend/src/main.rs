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

/// Pretty-log field formatter that writes only the `message` field.
/// Structured fields (error, cursor, etc.) are captured by the JSON subscriber
/// (CloudWatch/Logs Insights) but omitted from terminal output so values
/// don't appear twice alongside the human-readable message string.
struct MessageOnlyFields;

impl<'w> tracing_subscriber::fmt::FormatFields<'w> for MessageOnlyFields {
    fn format_fields<R: tracing_subscriber::field::RecordFields>(
        &self,
        mut writer: tracing_subscriber::fmt::format::Writer<'w>,
        fields: R,
    ) -> std::fmt::Result {
        use tracing_subscriber::field::Visit;

        struct MsgVisitor<'a> {
            writer: &'a mut dyn std::fmt::Write,
            result: std::fmt::Result,
        }

        impl Visit for MsgVisitor<'_> {
            fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn std::fmt::Debug) {
                if self.result.is_err() {
                    return;
                }
                if field.name() == "message" {
                    self.result = write!(self.writer, "{value:?}");
                }
            }

            fn record_str(&mut self, field: &tracing::field::Field, value: &str) {
                if self.result.is_err() {
                    return;
                }
                if field.name() == "message" {
                    self.result = write!(self.writer, "{value}");
                }
            }
        }

        let mut visitor = MsgVisitor {
            writer: &mut writer,
            result: Ok(()),
        };
        fields.record(&mut visitor);
        visitor.result
    }
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load .env from project root
    dotenvy::from_path("../.env").ok();
    dotenvy::dotenv().ok();

    let config = AppConfig::from_env()?;

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| {
        EnvFilter::new(format!(
            "{},sentinel_backend={}",
            config.crates_log_level, config.sentinel_log_level
        ))
    });
    match config.log_format {
        crate::config::LogFormat::Pretty => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(true)
            .fmt_fields(MessageOnlyFields)
            .init(),
        crate::config::LogFormat::Json => tracing_subscriber::fmt()
            .with_env_filter(filter)
            .with_ansi(false)
            .json()
            .init(),
    }
    tracing::info!(grpc_url = %config.sui_grpc_url, "SENTINEL starting — streaming from {}", config.sui_grpc_url);

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
            character_names = name_count,
            gate_names = gate_count,
            structure_types = struct_count,
            "Loaded {name_count} character names, {gate_count} gate names, \
             {struct_count} structure type mappings from DB cache"
        );
        if let Some(cp) = db::load_checkpoint(&db_pool).await? {
            tracing::info!(checkpoint = cp, "Resuming from checkpoint {cp}");
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

    // Periodic health summary
    let health_state = state.clone();
    tokio::spawn(async move {
        health_log_loop(health_state).await;
    });

    // HTTP API + SSE
    let app = api::router(state.clone(), sse_tx);

    let addr = format!("0.0.0.0:{api_port}");
    tracing::info!(addr, "API listening on {addr}");
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    axum::serve(listener, app).await?;

    Ok(())
}

/// Periodic health monitor.
/// - Checks gRPC stream staleness every 30s; warns immediately if no checkpoint
///   has arrived in >2 minutes (hung connection — distinct from a dropped connection,
///   which the reconnect loop already logs).
/// - Logs a full health summary every 5 minutes.
async fn health_log_loop(state: Arc<RwLock<AppState>>) {
    const STALL_THRESHOLD_SECS: u64 = 120;
    const SUMMARY_INTERVAL_SECS: u64 = 60;
    const CHECK_INTERVAL_SECS: u64 = 30;

    let mut ticks: u64 = 0;
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(CHECK_INTERVAL_SECS)).await;
        ticks += 1;

        let s = state.read().await;

        let checkpoint = s.last_checkpoint.unwrap_or(0);
        let stream_lag_secs = s
            .last_checkpoint_at
            .map(|t| t.elapsed().as_secs())
            .unwrap_or(u64::MAX);

        // Immediate error on hung stream — surfaces in CloudWatch for alarming
        if stream_lag_secs > STALL_THRESHOLD_SECS {
            tracing::error!(
                lag_secs = stream_lag_secs,
                cursor = checkpoint,
                "gRPC stream may be hung — no checkpoint in {}s (cursor: {checkpoint})",
                stream_lag_secs
            );
        }

        // Full summary every 5 minutes
        if ticks % (SUMMARY_INTERVAL_SECS / CHECK_INTERVAL_SECS) == 0 {
            let profiles = s.live.profiles.len();
            let unresolved = s
                .live
                .profiles
                .keys()
                .filter(|id| !s.live.name_cache.contains_key(id))
                .count();
            let events = s.live.recent_events.len();
            let stream_status = if stream_lag_secs > STALL_THRESHOLD_SECS {
                format!("hung ({}s)", stream_lag_secs)
            } else {
                format!("ok ({}s ago)", stream_lag_secs)
            };

            tracing::info!(
                grpc_status = %stream_status,
                cursor = checkpoint,
                profiles,
                unresolved_profiles = unresolved,
                buffered_events = events,
                "Health — gRPC: {stream_status} | cursor: {checkpoint} \
                 | profiles: {profiles} ({unresolved} unresolved) \
                 | buffered events: {events}"
            );
        }
    }
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
                tracing::warn!(character_id = p.character_item_id, error = %e, "Failed to persist profile {}: {e}", p.character_item_id);
            }
        }

        for e in &new_events {
            if let Err(e) = db::insert_event(&pool, e).await {
                tracing::warn!(error = %e, "Failed to persist event: {e}");
            }
        }

        last_event_count += new_events.len();

        if let Some(cp) = checkpoint {
            if let Err(e) = db::save_checkpoint(&pool, cp).await {
                tracing::warn!(error = %e, "Failed to save checkpoint: {e}");
            }
        }

        // Persist any new gate names
        {
            let s = state.read().await;
            for (gate_id, name) in &s.live.gate_name_cache {
                if !persisted_gates.contains(gate_id) {
                    if let Err(e) = db::upsert_gate_name(&pool, gate_id, name).await {
                        tracing::warn!(gate_id, error = %e, "Failed to persist gate name {gate_id}: {e}");
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
                    tracing::debug!(pruned, "Pruned {pruned} old events from database");
                }
            }
        }

        if !dirty_profiles.is_empty() || !new_events.is_empty() {
            tracing::info!(
                profiles = dirty_profiles.len(),
                events = new_events.len(),
                "DB sync: {} profiles, {} events flushed",
                dirty_profiles.len(),
                new_events.len()
            );
        }
    }
}

#[cfg(test)]
mod logging_tests {
    use super::MessageOnlyFields;
    use std::sync::{Arc, Mutex};

    // ---------------------------------------------------------------------------
    // Shared writer plumbing
    // ---------------------------------------------------------------------------

    struct SharedBuf(Arc<Mutex<Vec<u8>>>);

    impl std::io::Write for SharedBuf {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            self.0.lock().unwrap().extend_from_slice(buf);
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    #[derive(Clone)]
    struct MakeBuf(Arc<Mutex<Vec<u8>>>);

    impl<'a> tracing_subscriber::fmt::MakeWriter<'a> for MakeBuf {
        type Writer = SharedBuf;
        fn make_writer(&'a self) -> Self::Writer {
            SharedBuf(self.0.clone())
        }
    }

    fn capture_buf() -> (MakeBuf, Arc<Mutex<Vec<u8>>>) {
        let buf = Arc::new(Mutex::new(Vec::new()));
        (MakeBuf(buf.clone()), buf)
    }

    fn read_buf(buf: &Arc<Mutex<Vec<u8>>>) -> String {
        String::from_utf8(buf.lock().unwrap().clone()).unwrap()
    }

    // ---------------------------------------------------------------------------
    // JSON tests
    // ---------------------------------------------------------------------------

    /// Structured fields appear as discrete JSON keys in CloudWatch-style output.
    #[test]
    fn json_structured_fields_present() {
        let (writer, buf) = capture_buf();
        let sub = tracing_subscriber::fmt()
            .with_writer(writer)
            .json()
            .with_ansi(false)
            .finish();
        let _guard = tracing::subscriber::set_default(sub);

        tracing::info!(cursor = 319672639u64, profiles = 42usize, "Health check");

        let out = read_buf(&buf);
        let json: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(json["fields"]["message"], "Health check");
        assert_eq!(json["fields"]["cursor"], 319672639u64);
        assert_eq!(json["fields"]["profiles"], 42u64);
        assert_eq!(json["level"], "INFO");
    }

    /// Error field is a string and message still contains the human-readable text.
    #[test]
    fn json_error_field_and_message() {
        let (writer, buf) = capture_buf();
        let sub = tracing_subscriber::fmt()
            .with_writer(writer)
            .json()
            .with_ansi(false)
            .finish();
        let _guard = tracing::subscriber::set_default(sub);

        let err_msg = "connection refused";
        tracing::error!(
            error = err_msg,
            "gRPC stream error: {err_msg}, reconnecting in 2s..."
        );

        let out = read_buf(&buf);
        let json: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(json["level"], "ERROR");
        assert_eq!(json["fields"]["error"], "connection refused");
        assert!(
            json["fields"]["message"]
                .as_str()
                .unwrap()
                .contains("gRPC stream error"),
            "message field missing: {json}"
        );
    }

    /// Multiple numeric fields are all present as separate JSON keys.
    #[test]
    fn json_publisher_fields() {
        let (writer, buf) = capture_buf();
        let sub = tracing_subscriber::fmt()
            .with_writer(writer)
            .json()
            .with_ansi(false)
            .finish();
        let _guard = tracing::subscriber::set_default(sub);

        tracing::info!(
            backoff_secs = 30u64,
            consecutive_failures = 3u32,
            "Publisher backing off for 30s after 3 failures"
        );

        let out = read_buf(&buf);
        let json: serde_json::Value = serde_json::from_str(out.trim()).unwrap();
        assert_eq!(json["fields"]["backoff_secs"], 30u64);
        assert_eq!(json["fields"]["consecutive_failures"], 3u64);
    }

    // ---------------------------------------------------------------------------
    // Pretty (terminal) test
    // ---------------------------------------------------------------------------

    /// Pretty logs show only the message string — structured fields must not appear.
    #[test]
    fn pretty_log_message_only_no_fields() {
        let (writer, buf) = capture_buf();
        let sub = tracing_subscriber::fmt()
            .with_writer(writer)
            .fmt_fields(MessageOnlyFields)
            .with_ansi(false)
            .finish();
        let _guard = tracing::subscriber::set_default(sub);

        tracing::info!(
            cursor = 12345u64,
            profiles = 10usize,
            "Health — all systems nominal"
        );

        let out = read_buf(&buf);
        assert!(
            out.contains("Health — all systems nominal"),
            "message should appear: {out}"
        );
        assert!(
            !out.contains("cursor="),
            "cursor field must not appear in pretty output: {out}"
        );
        assert!(
            !out.contains("profiles="),
            "profiles field must not appear in pretty output: {out}"
        );
    }
}
