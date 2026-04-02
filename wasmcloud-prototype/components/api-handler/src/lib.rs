wit_bindgen::generate!({
    world: "api-handler",
    path: "wit",
    generate_all,
});

use exports::wasi::http::incoming_handler::Guest;
use serde::{Deserialize, Serialize};
use wasi::http::types::{
    Fields, IncomingRequest, OutgoingBody, OutgoingResponse, ResponseOutparam,
};
use wasi::keyvalue::store;

/// Mirror of ThreatProfile stored in sentinel.profiles KV bucket.
/// Key format: "pilot:{item_id}"
#[derive(Debug, Default, Serialize, Deserialize)]
struct ThreatProfile {
    item_id: u64,
    name: Option<String>,
    threat_score: u64,
    kill_count: u64,
    death_count: u64,
    bounty_count: u64,
    last_kill_ts: u64,
    last_seen_system: String,
    tribe_id: Option<String>,
    tribe_name: Option<String>,
    recent_kills_24h: u64,
    recent_deaths_24h: u64,
    systems_visited: u64,
    dirty: bool,
}

#[derive(Serialize)]
struct HealthResponse {
    status: &'static str,
    profiles: usize,
}

#[derive(Serialize)]
struct DataStats {
    total_pilots: usize,
    total_kills: u64,
    total_deaths: u64,
}

#[derive(Serialize)]
struct DataResponse {
    profiles: Vec<ThreatProfile>,
    stats: DataStats,
}

struct Component;
export!(Component);

impl Guest for Component {
    fn handle(request: IncomingRequest, response_out: ResponseOutparam) {
        let method = request.method();
        let path = request.path_with_query().unwrap_or_default();

        // Strip query string for routing.
        let path_only = path.split('?').next().unwrap_or("/");

        match (method, path_only) {
            (wasi::http::types::Method::Get, "/api/health") => {
                handle_health(response_out);
            }
            (wasi::http::types::Method::Get, "/api/data") => {
                handle_data(response_out);
            }
            (wasi::http::types::Method::Get, "/api/events/stream") => {
                // Derive a connection-id from the request authority + path.
                // In a real deployment this would use a UUID from the host, but
                // the SSE bridge only needs a stable per-connection string.
                let authority = request.authority().unwrap_or_default();
                let connection_id = format!("sse-{authority}-{path}");
                handle_sse_stream(response_out, &connection_id);
            }
            _ => {
                send_response(response_out, 404, "application/json", b"{\"error\":\"not found\"}");
            }
        }
    }
}

// ─── Route handlers ──────────────────────────────────────────────────────────

fn handle_health(response_out: ResponseOutparam) {
    let count = match open_profiles_bucket() {
        Ok(bucket) => list_all_keys(&bucket).len(),
        Err(_) => 0,
    };
    let body = serde_json::to_vec(&HealthResponse { status: "ok", profiles: count })
        .unwrap_or_else(|_| b"{\"status\":\"ok\",\"profiles\":0}".to_vec());
    send_response(response_out, 200, "application/json", &body);
}

fn handle_data(response_out: ResponseOutparam) {
    let mut profiles: Vec<ThreatProfile> = Vec::new();

    if let Ok(bucket) = open_profiles_bucket() {
        let keys = list_all_keys(&bucket);
        for key in keys.iter().take(200) {
            if let Ok(Some(bytes)) = bucket.get(key) {
                if let Ok(profile) = serde_json::from_slice::<ThreatProfile>(&bytes) {
                    profiles.push(profile);
                }
            }
        }
    }

    // Sort by threat_score descending.
    profiles.sort_by(|a, b| b.threat_score.cmp(&a.threat_score));

    let total_pilots = profiles.len();
    let total_kills: u64 = profiles.iter().map(|p| p.kill_count).sum();
    let total_deaths: u64 = profiles.iter().map(|p| p.death_count).sum();

    let response = DataResponse {
        profiles,
        stats: DataStats {
            total_pilots,
            total_kills,
            total_deaths,
        },
    };

    let body = serde_json::to_vec(&response)
        .unwrap_or_else(|_| b"{\"profiles\":[],\"stats\":{\"total_pilots\":0,\"total_kills\":0,\"total_deaths\":0}}".to_vec());
    send_response(response_out, 200, "application/json", &body);
}

fn handle_sse_stream(response_out: ResponseOutparam, connection_id: &str) {
    // Register the connection with the SSE bridge.
    // Ignore errors — the bridge may not be linked in all environments.
    let _ = sentinel::sse::server::register_connection(connection_id);

    // Return 200 with SSE content type. Actual streaming is handled by sse-bridge.
    send_response_with_headers(
        response_out,
        200,
        &[
            ("content-type", "text/event-stream"),
            ("cache-control", "no-cache"),
            ("connection", "keep-alive"),
        ],
        &[],
    );
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn open_profiles_bucket() -> Result<store::Bucket, String> {
    store::open("sentinel.profiles").map_err(|e| format!("open kv bucket: {e:?}"))
}

/// Collect all keys from the bucket using cursor-based pagination.
fn list_all_keys(bucket: &store::Bucket) -> Vec<String> {
    let mut keys = Vec::new();
    let mut cursor: Option<u64> = None;
    loop {
        match bucket.list_keys(cursor) {
            Ok(resp) => {
                keys.extend(resp.keys);
                match resp.cursor {
                    Some(c) => cursor = Some(c),
                    None => break,
                }
            }
            Err(_) => break,
        }
    }
    keys
}

fn send_response(response_out: ResponseOutparam, status: u16, content_type: &str, body: &[u8]) {
    send_response_with_headers(
        response_out,
        status,
        &[("content-type", content_type)],
        body,
    );
}

fn send_response_with_headers(
    response_out: ResponseOutparam,
    status: u16,
    headers: &[(&str, &str)],
    body: &[u8],
) {
    let fields = Fields::new();
    for (name, value) in headers {
        let _ = fields.append(&name.to_string(), &value.as_bytes().to_vec());
    }

    let response = OutgoingResponse::new(fields);
    let _ = response.set_status_code(status);

    if !body.is_empty() {
        if let Ok(outgoing_body) = response.body() {
            if let Ok(stream) = outgoing_body.write() {
                let _ = stream.blocking_write_and_flush(body);
                drop(stream);
            }
            let _ = OutgoingBody::finish(outgoing_body, None);
        }
    }

    ResponseOutparam::set(response_out, Ok(response));
}
