//! Integration tests asserting that actual application log calls emit the expected
//! structured fields in JSON format and the expected message text in pretty format.
//!
//! These tests call real application functions (no mocking) and capture tracing
//! output via a shared buffer subscriber, then assert on the JSON structure or
//! plain-text content.

use std::sync::{Arc, Mutex};

use sentinel_backend::config::{AppConfig, LogFormat};
use sentinel_backend::grpc::sui_rpc;
use sentinel_backend::types::AppState;

// ---------------------------------------------------------------------------
// Capture infrastructure
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

fn capture() -> (MakeBuf, Arc<Mutex<Vec<u8>>>) {
    let buf = Arc::new(Mutex::new(Vec::new()));
    (MakeBuf(buf.clone()), buf)
}

fn read(buf: &Arc<Mutex<Vec<u8>>>) -> String {
    String::from_utf8(buf.lock().unwrap().clone()).unwrap()
}

fn test_config() -> AppConfig {
    AppConfig {
        sui_grpc_url: "http://localhost:9000".into(),
        sui_graphql_url: "http://localhost:9001".into(),
        sentinel_package_id: "0xsentinel".into(),
        threat_registry_id: "0xregistry".into(),
        publisher_private_key: String::new(),
        world_package_id: "0xworld".into(),
        bounty_board_package_id: "0xbounty".into(),
        publish_interval_ms: 5000,
        publish_score_threshold_bp: 100,
        api_port: 8080,
        database_url: "postgres://localhost/test".into(),
        world_api_url: "http://localhost:9002".into(),
        sentinel_log_level: tracing::Level::DEBUG,
        crates_log_level: tracing::Level::WARN,
        log_format: LogFormat::Json,
        discord_token: "test-token".into(),
        max_recent_events: 1000,
    }
}

fn one_event_checkpoint(event_type: &str, package_id: &str) -> sui_rpc::Checkpoint {
    sui_rpc::Checkpoint {
        transactions: vec![sui_rpc::ExecutedTransaction {
            events: Some(sui_rpc::TransactionEvents {
                events: vec![sui_rpc::Event {
                    event_type: Some(event_type.into()),
                    package_id: Some(package_id.into()),
                    ..Default::default()
                }],
                ..Default::default()
            }),
            ..Default::default()
        }],
        ..Default::default()
    }
}

// ---------------------------------------------------------------------------
// JSON — structured field assertions on process_checkpoint_events
// ---------------------------------------------------------------------------

/// A matching event must emit a DEBUG log with `event_type`, `package_id`,
/// and `timestamp_ms` as discrete JSON fields.
#[test]
fn dispatched_event_log_has_structured_fields() {
    let (writer, buf) = capture();
    let sub = tracing_subscriber::fmt()
        .with_writer(writer)
        .json()
        .with_ansi(false)
        // Enable DEBUG so the dispatch log is captured
        .with_max_level(tracing::Level::DEBUG)
        .finish();
    let _guard = tracing::subscriber::set_default(sub);

    let config = test_config();
    let mut state = AppState::default();
    let checkpoint = one_event_checkpoint("KillMailCreatedEvent", &config.world_package_id);

    sentinel_backend::grpc::process_checkpoint_events(
        &config,
        &mut state.live,
        &None,
        &checkpoint,
        1_234_567_890,
    );

    let out = read(&buf);
    let line = out
        .lines()
        .find(|l| l.contains("event dispatched"))
        .unwrap_or_else(|| panic!("no 'event dispatched' log line found in:\n{out}"));
    let json: serde_json::Value = serde_json::from_str(line).unwrap();

    assert_eq!(json["fields"]["event_type"], "KillMailCreatedEvent");
    assert_eq!(json["fields"]["package_id"], config.world_package_id);
    assert_eq!(json["fields"]["timestamp_ms"], 1_234_567_890u64);
    assert_eq!(json["level"], "DEBUG");
}

/// An event from an unknown package must be silently dropped — no dispatch log.
#[test]
fn unknown_package_event_produces_no_dispatch_log() {
    let (writer, buf) = capture();
    let sub = tracing_subscriber::fmt()
        .with_writer(writer)
        .json()
        .with_ansi(false)
        .with_max_level(tracing::Level::DEBUG)
        .finish();
    let _guard = tracing::subscriber::set_default(sub);

    let config = test_config();
    let mut state = AppState::default();
    let checkpoint = one_event_checkpoint("SomeEvent", "0xunknownpkg");

    sentinel_backend::grpc::process_checkpoint_events(
        &config,
        &mut state.live,
        &None,
        &checkpoint,
        999,
    );

    let out = read(&buf);
    assert!(
        !out.contains("event dispatched"),
        "unknown package events must not produce a dispatch log: {out}"
    );
}

/// Multiple events from the same checkpoint each emit their own log line.
#[test]
fn multiple_events_each_produce_a_log_line() {
    let (writer, buf) = capture();
    let sub = tracing_subscriber::fmt()
        .with_writer(writer)
        .json()
        .with_ansi(false)
        .with_max_level(tracing::Level::DEBUG)
        .finish();
    let _guard = tracing::subscriber::set_default(sub);

    let config = test_config();
    let mut state = AppState::default();

    let checkpoint = sui_rpc::Checkpoint {
        transactions: vec![sui_rpc::ExecutedTransaction {
            events: Some(sui_rpc::TransactionEvents {
                events: vec![
                    sui_rpc::Event {
                        event_type: Some("KillMailCreatedEvent".into()),
                        package_id: Some(config.world_package_id.clone()),
                        ..Default::default()
                    },
                    sui_rpc::Event {
                        event_type: Some("BountyPostedEvent".into()),
                        package_id: Some(config.bounty_board_package_id.clone()),
                        ..Default::default()
                    },
                ],
                ..Default::default()
            }),
            ..Default::default()
        }],
        ..Default::default()
    };

    sentinel_backend::grpc::process_checkpoint_events(
        &config,
        &mut state.live,
        &None,
        &checkpoint,
        42,
    );

    let out = read(&buf);
    let dispatch_lines: Vec<_> = out
        .lines()
        .filter(|l| l.contains("event dispatched"))
        .collect();
    assert_eq!(
        dispatch_lines.len(),
        2,
        "expected 2 dispatch log lines, got {}:\n{out}",
        dispatch_lines.len()
    );

    let types: Vec<&str> = dispatch_lines
        .iter()
        .filter_map(|l| serde_json::from_str::<serde_json::Value>(l).ok())
        .filter_map(|j| {
            j["fields"]["event_type"]
                .as_str()
                .map(|s| s.to_owned())
                .map(|s| Box::leak(s.into_boxed_str()) as &str)
        })
        .collect();
    assert!(
        types.contains(&"KillMailCreatedEvent"),
        "kill event missing: {types:?}"
    );
    assert!(
        types.contains(&"BountyPostedEvent"),
        "bounty event missing: {types:?}"
    );
}

// ---------------------------------------------------------------------------
// Pretty format — message text reaches the output
// ---------------------------------------------------------------------------

/// In pretty (terminal) mode the log output must contain the human-readable
/// message text. The production binary's `MessageOnlyFields` formatter (tested
/// in main.rs unit tests) then strips the `key=value` suffix; here we verify
/// the message string itself is present and correct.
#[test]
fn pretty_dispatched_event_contains_message_text() {
    let (writer, buf) = capture();
    let sub = tracing_subscriber::fmt()
        .with_writer(writer)
        .with_ansi(false)
        .with_max_level(tracing::Level::DEBUG)
        .finish();
    let _guard = tracing::subscriber::set_default(sub);

    let config = test_config();
    let mut state = AppState::default();
    let checkpoint = one_event_checkpoint("KillMailCreatedEvent", &config.world_package_id);

    sentinel_backend::grpc::process_checkpoint_events(
        &config,
        &mut state.live,
        &None,
        &checkpoint,
        777,
    );

    let out = read(&buf);
    assert!(
        out.contains("event dispatched"),
        "message text 'event dispatched' must appear in pretty output: {out}"
    );
    assert!(
        out.contains("KillMailCreatedEvent"),
        "event type must appear in pretty output: {out}"
    );
}
