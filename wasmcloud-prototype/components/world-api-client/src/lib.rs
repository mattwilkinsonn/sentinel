wit_bindgen::generate!({
    world: "world-api-client",
    path: "wit",
    generate_all,
});

use exports::wasmcloud::messaging::handler::Guest;
use serde::{Deserialize, Serialize};
use wasi::keyvalue::store;
use wasi::logging::logging::{log, Level};

#[allow(dead_code)]
const WORLD_API_BASE: &str = "https://world-api-stillness.live.tech.evefrontier.com";
const MAX_LOOKUPS_PER_TICK: usize = 10;

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

/// Expected shape from /solarsystems/{system_id}.
#[derive(Debug, Deserialize)]
struct SolarSystemResponse {
    name: Option<String>,
    region: Option<String>,
    security_class: Option<String>,
}

/// What we store in sentinel.systems under key "system:{system_id}".
#[derive(Debug, Serialize)]
struct SystemEntry {
    name: String,
    region: String,
    security_class: String,
}

/// Expected shape from /types/{type_id}.
#[derive(Debug, Deserialize)]
struct TypeResponse {
    name: Option<String>,
}

struct Component;
export!(Component);

impl Guest for Component {
    fn handle_message(msg: wasmcloud::messaging::types::BrokerMessage) -> Result<(), String> {
        if msg.subject != "sentinel.name-tick" {
            return Ok(());
        }

        log(
            Level::Info,
            "world-api-client",
            "name-tick received — resolving system and type metadata",
        );

        let profiles_bucket = store::open("sentinel.profiles")
            .map_err(|e| format!("open sentinel.profiles: {e:?}"))?;
        let systems_bucket = store::open("sentinel.systems")
            .map_err(|e| format!("open sentinel.systems: {e:?}"))?;
        let types_bucket = store::open("sentinel.cache.types")
            .map_err(|e| format!("open sentinel.cache.types: {e:?}"))?;

        // Collect all profile keys
        let mut all_keys: Vec<String> = Vec::new();
        let mut cursor: Option<u64> = None;
        loop {
            let resp = profiles_bucket
                .list_keys(cursor)
                .map_err(|e| format!("list-keys profiles: {e:?}"))?;
            all_keys.extend(resp.keys);
            match resp.cursor {
                Some(c) => cursor = Some(c),
                None => break,
            }
        }

        let mut lookups = 0usize;

        for key in &all_keys {
            if !key.starts_with("pilot:") {
                continue;
            }
            if lookups >= MAX_LOOKUPS_PER_TICK {
                break;
            }

            let profile: ThreatProfile =
                match profiles_bucket.get(key).map_err(|e| format!("kv get {key}: {e:?}"))? {
                    Some(bytes) => serde_json::from_slice(&bytes)
                        .map_err(|e| format!("deserialize {key}: {e}"))?,
                    None => continue,
                };

            // Resolve solar system if we have a system_id but no cached entry
            if !profile.last_seen_system.is_empty() {
                let sys_key = format!("system:{}", profile.last_seen_system);
                let already_cached = systems_bucket
                    .exists(&sys_key)
                    .map_err(|e| format!("exists {sys_key}: {e:?}"))?;

                if !already_cached {
                    match fetch_solar_system(&profile.last_seen_system) {
                        Ok(Some(entry)) => {
                            let bytes = serde_json::to_vec(&entry)
                                .map_err(|e| format!("serialize system entry: {e}"))?;
                            systems_bucket
                                .set(&sys_key, &bytes)
                                .map_err(|e| format!("kv set {sys_key}: {e:?}"))?;
                            log(
                                Level::Info,
                                "world-api-client",
                                &format!(
                                    "cached system {} → {}",
                                    profile.last_seen_system, entry.name
                                ),
                            );
                            lookups += 1;
                        }
                        Ok(None) => {
                            log(
                                Level::Debug,
                                "world-api-client",
                                &format!("system {} not found in World API", profile.last_seen_system),
                            );
                            lookups += 1;
                        }
                        Err(e) => {
                            log(
                                Level::Warn,
                                "world-api-client",
                                &format!("fetch system {}: {e}", profile.last_seen_system),
                            );
                        }
                    }
                }
            }

            if lookups >= MAX_LOOKUPS_PER_TICK {
                break;
            }
        }

        // Second pass: resolve any type IDs referenced in the name-tick payload (if any).
        // The message body may optionally contain a JSON array of type IDs to pre-cache.
        if let Ok(type_ids) = serde_json::from_slice::<Vec<u64>>(&msg.body) {
            for type_id in type_ids {
                if lookups >= MAX_LOOKUPS_PER_TICK {
                    break;
                }
                let type_key = format!("type:{type_id}");
                let already_cached = types_bucket
                    .exists(&type_key)
                    .map_err(|e| format!("exists {type_key}: {e:?}"))?;
                if already_cached {
                    continue;
                }
                match fetch_type_name(type_id) {
                    Ok(Some(name)) => {
                        types_bucket
                            .set(&type_key, &name.into_bytes())
                            .map_err(|e| format!("kv set {type_key}: {e:?}"))?;
                        log(
                            Level::Info,
                            "world-api-client",
                            &format!("cached type {type_id}"),
                        );
                        lookups += 1;
                    }
                    Ok(None) => {
                        lookups += 1;
                    }
                    Err(e) => {
                        log(
                            Level::Warn,
                            "world-api-client",
                            &format!("fetch type {type_id}: {e}"),
                        );
                    }
                }
            }
        }

        log(
            Level::Info,
            "world-api-client",
            &format!("name-tick complete: {lookups} World API lookups performed"),
        );

        Ok(())
    }
}

/// GET /solarsystems/{system_id} and parse the response.
fn fetch_solar_system(system_id: &str) -> Result<Option<SystemEntry>, String> {
    let path = format!("/solarsystems/{system_id}");
    let body_bytes = http_get("world-api-stillness.live.tech.evefrontier.com", &path)?;

    if body_bytes.is_empty() {
        return Ok(None);
    }

    let parsed: SolarSystemResponse = serde_json::from_slice(&body_bytes)
        .map_err(|e| format!("parse solar system response: {e}"))?;

    let name = parsed.name.unwrap_or_default();
    if name.is_empty() {
        return Ok(None);
    }

    Ok(Some(SystemEntry {
        name,
        region: parsed.region.unwrap_or_default(),
        security_class: parsed.security_class.unwrap_or_default(),
    }))
}

/// GET /types/{type_id} and return the name.
fn fetch_type_name(type_id: u64) -> Result<Option<String>, String> {
    let path = format!("/types/{type_id}");
    let body_bytes = http_get("world-api-stillness.live.tech.evefrontier.com", &path)?;

    if body_bytes.is_empty() {
        return Ok(None);
    }

    let parsed: TypeResponse = serde_json::from_slice(&body_bytes)
        .map_err(|e| format!("parse type response: {e}"))?;

    Ok(parsed.name.filter(|n| !n.is_empty()))
}

/// Perform a blocking HTTPS GET and return the response body bytes.
/// Returns empty Vec on 404; errors on other non-2xx statuses.
fn http_get(authority: &str, path: &str) -> Result<Vec<u8>, String> {
    use wasi::http::outgoing_handler;
    use wasi::http::types::{Fields, Method, OutgoingRequest, Scheme};

    let headers = Fields::new();
    let req = OutgoingRequest::new(headers);
    req.set_method(&Method::Get)
        .map_err(|()| "set method GET".to_string())?;
    req.set_scheme(Some(&Scheme::Https))
        .map_err(|()| "set scheme".to_string())?;
    req.set_authority(Some(authority))
        .map_err(|()| "set authority".to_string())?;
    req.set_path_with_query(Some(path))
        .map_err(|()| "set path".to_string())?;

    let future = outgoing_handler::handle(req, None)
        .map_err(|e| format!("http handle {path}: {e:?}"))?;

    future.subscribe().block();

    let resp = match future.get() {
        Some(Ok(Ok(r))) => r,
        Some(Ok(Err(e))) => return Err(format!("HTTP error on {path}: {e:?}")),
        Some(Err(())) => return Err("response already consumed".to_string()),
        None => return Err("response not ready".to_string()),
    };

    let status = resp.status();

    if status == 404 {
        return Ok(Vec::new());
    }

    if status < 200 || status >= 300 {
        return Err(format!(
            "World API {path} returned HTTP {status}"
        ));
    }

    read_incoming_body(resp)
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
