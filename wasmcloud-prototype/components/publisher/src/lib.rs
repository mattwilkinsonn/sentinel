wit_bindgen::generate!({
    world: "publisher",
    path: "wit",
    generate_all,
});

use exports::wasmcloud::messaging::handler::Guest;
use serde::{Deserialize, Serialize};
use wasi::keyvalue::store;
use wasi::logging::logging::{log, Level};

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

struct Component;
export!(Component);

impl Guest for Component {
    fn handle_message(msg: wasmcloud::messaging::types::BrokerMessage) -> Result<(), String> {
        if msg.subject != "sentinel.publish-tick" {
            return Ok(());
        }

        log(Level::Info, "publisher", "publish-tick received — scanning dirty profiles");

        let profiles_bucket = store::open("sentinel.profiles")
            .map_err(|e| format!("open sentinel.profiles: {e:?}"))?;
        let meta_bucket = store::open("sentinel.meta")
            .map_err(|e| format!("open sentinel.meta: {e:?}"))?;

        // Collect all keys using cursor pagination
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

        // Filter to dirty pilot keys, cap at 50 per tick
        let mut dirty_profiles: Vec<ThreatProfile> = Vec::new();
        for key in &all_keys {
            if !key.starts_with("pilot:") {
                continue;
            }
            if dirty_profiles.len() >= 50 {
                break;
            }
            match profiles_bucket.get(key).map_err(|e| format!("kv get {key}: {e:?}"))? {
                Some(bytes) => {
                    let profile: ThreatProfile = serde_json::from_slice(&bytes)
                        .map_err(|e| format!("deserialize {key}: {e}"))?;
                    if profile.dirty {
                        dirty_profiles.push(profile);
                    }
                }
                None => continue,
            }
        }

        if dirty_profiles.is_empty() {
            log(Level::Info, "publisher", "no dirty profiles — nothing to publish");
            return Ok(());
        }

        let n = dirty_profiles.len();
        log(
            Level::Info,
            "publisher",
            &format!("would publish {n} profiles (Sui transaction signing not yet implemented)"),
        );

        // Stub: log the batch instead of submitting a real transaction.
        // Real implementation would:
        //   1. Build a batch_update PTB with the profile data
        //   2. Sign with the publisher key from wasmcloud:secrets/store
        //   3. POST to the Sui JSON-RPC or gRPC endpoint
        stub_post_to_sui(n)?;

        // Mark each published profile as clean
        for mut profile in dirty_profiles {
            profile.dirty = false;
            let bytes = serde_json::to_vec(&profile)
                .map_err(|e| format!("serialize profile {}: {e}", profile.item_id))?;
            profiles_bucket
                .set(&format!("pilot:{}", profile.item_id), &bytes)
                .map_err(|e| format!("kv set pilot:{}: {e:?}", profile.item_id))?;
        }

        // Update publisher cursor timestamp in meta bucket
        let ts_bytes = b"0".to_vec();
        meta_bucket
            .set("publisher.cursor", &ts_bytes)
            .map_err(|e| format!("kv set publisher.cursor: {e:?}"))?;

        log(
            Level::Info,
            "publisher",
            &format!("publish-tick complete: cleared dirty flag on {n} profiles"),
        );

        Ok(())
    }
}

/// Placeholder for the real Sui RPC call.
/// Logs what would be published and performs a stubbed HTTP call to the fullnode.
fn stub_post_to_sui(n: usize) -> Result<(), String> {
    use wasi::http::outgoing_handler;
    use wasi::http::types::{Fields, Method, OutgoingBody, OutgoingRequest, Scheme};

    let url = "https://fullnode.testnet.sui.io";
    log(
        Level::Info,
        "publisher",
        &format!("stub: would POST batch_update of {n} profiles to {url}"),
    );

    let payload = format!(
        r#"{{"jsonrpc":"2.0","id":1,"method":"sui_dryRunTransactionBlock","params":["stub-batch-{n}"]}}"#
    );
    let body_bytes = payload.into_bytes();

    let headers = Fields::new();
    headers
        .append(&"content-type".to_string(), &b"application/json".to_vec())
        .map_err(|e| format!("set content-type: {e:?}"))?;

    let req = OutgoingRequest::new(headers);
    req.set_method(&Method::Post)
        .map_err(|()| "set method POST".to_string())?;
    req.set_scheme(Some(&Scheme::Https))
        .map_err(|()| "set scheme https".to_string())?;
    req.set_authority(Some("fullnode.testnet.sui.io"))
        .map_err(|()| "set authority".to_string())?;
    req.set_path_with_query(Some("/"))
        .map_err(|()| "set path".to_string())?;

    let out_body = req.body().map_err(|()| "get request body".to_string())?;
    {
        let stream = out_body.write().map_err(|()| "get body stream".to_string())?;
        write_bytes_to_stream(&stream, &body_bytes)?;
        drop(stream);
    }
    OutgoingBody::finish(out_body, None).map_err(|e| format!("finish body: {e:?}"))?;

    let future = outgoing_handler::handle(req, None)
        .map_err(|e| format!("http handle: {e:?}"))?;

    // Block until response arrives
    future.subscribe().block();

    match future.get() {
        Some(Ok(Ok(resp))) => {
            let status = resp.status();
            log(
                Level::Info,
                "publisher",
                &format!("stub Sui RPC responded with status {status}"),
            );
        }
        Some(Ok(Err(e))) => {
            log(
                Level::Warn,
                "publisher",
                &format!("stub Sui RPC HTTP error: {e:?}"),
            );
        }
        Some(Err(())) => {
            log(Level::Warn, "publisher", "stub Sui RPC: response already consumed");
        }
        None => {
            log(Level::Warn, "publisher", "stub Sui RPC: response not ready");
        }
    }

    Ok(())
}

fn write_bytes_to_stream(
    stream: &wasi::io::streams::OutputStream,
    data: &[u8],
) -> Result<(), String> {
    let mut offset = 0;
    while offset < data.len() {
        let n = stream
            .check_write()
            .map_err(|e| format!("check_write: {e:?}"))?;
        if n == 0 {
            stream.subscribe().block();
            continue;
        }
        let end = (offset + n as usize).min(data.len());
        let chunk = &data[offset..end];
        stream
            .write(chunk)
            .map_err(|e| format!("write: {e:?}"))?;
        offset = end;
    }
    stream.flush().map_err(|e| format!("flush: {e:?}"))?;
    Ok(())
}
