wit_bindgen::generate!({
    world: "name-resolver",
    path: "wit",
    generate_all,
});

use exports::wasmcloud::messaging::handler::Guest;
use serde::{Deserialize, Serialize};
use wasmcloud::messaging::consumer;
use wasi::keyvalue::store;
use wasi::logging::logging::{log, Level};

#[allow(dead_code)]
const WORLD_API_BASE: &str = "https://world-api-stillness.live.tech.evefrontier.com";
const MAX_PER_TICK: usize = 20;

/// Mirror of ThreatProfile stored in sentinel.profiles KV bucket.
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

/// Expected shape from World API character endpoint.
#[derive(Debug, Deserialize)]
struct CharacterResponse {
    name: Option<String>,
}

struct Component;
export!(Component);

impl Guest for Component {
    fn handle_message(msg: wasmcloud::messaging::types::BrokerMessage) -> Result<(), String> {
        if msg.subject != "sentinel.name-tick" {
            return Ok(());
        }

        log(Level::Info, "name-resolver", "name-tick received — resolving missing pilot names");

        let profiles_bucket = store::open("sentinel.profiles")
            .map_err(|e| format!("open sentinel.profiles: {e:?}"))?;
        let names_bucket = store::open("sentinel.names")
            .map_err(|e| format!("open sentinel.names: {e:?}"))?;

        // Collect all profile keys
        let mut all_keys: Vec<String> = Vec::new();
        let mut cursor: Option<u64> = None;
        loop {
            let resp = profiles_bucket
                .list_keys(cursor)
                .map_err(|e| format!("list-keys: {e:?}"))?;
            all_keys.extend(resp.keys);
            match resp.cursor {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }

        let mut resolved = 0usize;

        for key in &all_keys {
            if !key.starts_with("pilot:") {
                continue;
            }
            if resolved >= MAX_PER_TICK {
                break;
            }

            let mut profile: ThreatProfile =
                match profiles_bucket.get(key).map_err(|e| format!("kv get {key}: {e:?}"))? {
                    Some(bytes) => serde_json::from_slice(&bytes)
                        .map_err(|e| format!("deserialize {key}: {e}"))?,
                    None => continue,
                };

            if profile.name.is_some() {
                continue;
            }

            let item_id = profile.item_id;

            // Try World API first
            match fetch_character_name(item_id) {
                Ok(Some(name)) => {
                    log(
                        Level::Info,
                        "name-resolver",
                        &format!("resolved pilot {item_id} → {name} via World API"),
                    );

                    // Write to sentinel.names KV
                    let name_bytes = name.as_bytes().to_vec();
                    names_bucket
                        .set(&format!("name:{item_id}"), &name_bytes)
                        .map_err(|e| format!("kv set name:{item_id}: {e:?}"))?;

                    // Update profile
                    profile.name = Some(name);
                    profile.dirty = true;
                    let profile_bytes = serde_json::to_vec(&profile)
                        .map_err(|e| format!("serialize profile {item_id}: {e}"))?;
                    profiles_bucket
                        .set(key, &profile_bytes)
                        .map_err(|e| format!("kv set {key}: {e:?}"))?;

                    resolved += 1;
                }
                Ok(None) => {
                    // Not found via HTTP — request gRPC resolution via sui-bridge
                    log(
                        Level::Debug,
                        "name-resolver",
                        &format!("pilot {item_id} not found via World API — publishing name-request"),
                    );
                    publish_name_request(item_id)?;
                    resolved += 1;
                }
                Err(e) => {
                    log(
                        Level::Warn,
                        "name-resolver",
                        &format!("World API error for pilot {item_id}: {e}"),
                    );
                }
            }
        }

        log(
            Level::Info,
            "name-resolver",
            &format!("name-tick complete: processed {resolved} pilots"),
        );

        Ok(())
    }
}

/// GET /characters/{item_id} from the World API.
/// Returns Ok(Some(name)) on success, Ok(None) on 404, Err on other failures.
fn fetch_character_name(item_id: u64) -> Result<Option<String>, String> {
    use wasi::http::outgoing_handler;
    use wasi::http::types::{Fields, Method, OutgoingRequest, Scheme};

    let path = format!("/characters/{item_id}");
    let headers = Fields::new();
    let req = OutgoingRequest::new(headers);
    req.set_method(&Method::Get)
        .map_err(|()| "set method GET".to_string())?;
    req.set_scheme(Some(&Scheme::Https))
        .map_err(|()| "set scheme".to_string())?;
    req.set_authority(Some(
        "world-api-stillness.live.tech.evefrontier.com",
    ))
    .map_err(|()| "set authority".to_string())?;
    req.set_path_with_query(Some(&path))
        .map_err(|()| "set path".to_string())?;

    let future = outgoing_handler::handle(req, None)
        .map_err(|e| format!("http handle: {e:?}"))?;

    future.subscribe().block();

    let resp = match future.get() {
        Some(Ok(Ok(r))) => r,
        Some(Ok(Err(e))) => return Err(format!("HTTP error: {e:?}")),
        Some(Err(())) => return Err("response already consumed".to_string()),
        None => return Err("response not ready".to_string()),
    };

    let status = resp.status();

    if status == 404 {
        return Ok(None);
    }

    if status < 200 || status >= 300 {
        return Err(format!("World API returned HTTP {status} for pilot {item_id}"));
    }

    let body_bytes = read_incoming_body(resp)?;
    let parsed: CharacterResponse = serde_json::from_slice(&body_bytes)
        .map_err(|e| format!("parse character response: {e}"))?;

    Ok(parsed.name.filter(|n| !n.is_empty()))
}

/// Read the body from an incoming HTTP response into a Vec<u8>.
fn read_incoming_body(resp: wasi::http::types::IncomingResponse) -> Result<Vec<u8>, String> {
    let body = resp
        .consume()
        .map_err(|()| "consume response body".to_string())?;
    let stream = body
        .stream()
        .map_err(|()| "get body stream".to_string())?;

    let mut data = Vec::new();
    loop {
        match stream.blocking_read(4096) {
            Ok(chunk) if chunk.is_empty() => break,
            Ok(chunk) => data.extend_from_slice(&chunk),
            Err(wasi::io::streams::StreamError::Closed) => break,
            Err(e) => return Err(format!("read body: {e:?}")),
        }
    }
    drop(stream);
    wasi::http::types::IncomingBody::finish(body);
    Ok(data)
}

/// Publish a name resolution request to sentinel.name-requests for sui-bridge to handle.
fn publish_name_request(pilot_id: u64) -> Result<(), String> {
    let body = format!(r#"{{"pilot_id":{pilot_id}}}"#).into_bytes();
    consumer::publish(&wasmcloud::messaging::types::BrokerMessage {
        subject: "sentinel.name-requests".to_string(),
        reply_to: None,
        body,
    })
    .map_err(|e| format!("publish name-request for {pilot_id}: {e}"))
}
