//! Historical data loading from Sui GraphQL.
//! Fetches past killmails, character events, and jumps to seed threat profiles.

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::AppConfig;
use crate::threat_engine;
use crate::types::{AppState, RawEvent, ThreatProfile};

/// Load all historical data in sequence, then sort events.
pub async fn load_all(config: AppConfig, state: Arc<RwLock<AppState>>) {
    macro_rules! load {
        ($fn:expr, $name:expr) => {
            if let Err(e) = $fn.await {
                tracing::warn!("{} failed: {e}", $name);
            }
        };
    }

    load!(
        load_historical_killmails(&config, &state),
        "Historical killmails"
    );
    load!(load_character_events(&config, &state), "Character events");
    load!(load_jump_events(&config, &state), "Jump events");
    load!(load_character_names(&config, &state), "Character names");

    // Sort events newest first
    let mut s = state.write().await;
    s.live
        .recent_events
        .make_contiguous()
        .sort_by(|a, b| b.timestamp_ms.cmp(&a.timestamp_ms));
    s.live
        .new_pilot_events
        .make_contiguous()
        .sort_by(|a, b| b.timestamp_ms.cmp(&a.timestamp_ms));
    tracing::info!("Historical data load complete");
}

/// Extract a numeric item_id from nested {"item_id": "123", "tenant": "..."} or plain value.
fn extract_item_id(v: &serde_json::Value) -> Option<u64> {
    // Nested: {"item_id": "123", "tenant": "stillness"}
    v.get("item_id")
        .and_then(|id| {
            id.as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .or_else(|| id.as_u64())
        })
        // Fallback: plain value
        .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
        .or_else(|| v.as_u64())
}

/// Extract item_id as string (for solar_system_id).
fn extract_item_id_str(v: &serde_json::Value) -> String {
    // Nested: {"item_id": "30016543", "tenant": "stillness"}
    if let Some(id) = v.get("item_id") {
        return id.as_str().unwrap_or("").to_string();
    }
    // Plain string or number
    v.as_str().map(|s| s.to_string()).unwrap_or_default()
}

/// Load character names from Sui GraphQL for all known profiles.
pub async fn load_character_names(
    config: &AppConfig,
    state: &Arc<RwLock<AppState>>,
) -> Result<usize, Box<dyn std::error::Error>> {
    let character_type = format!("{}::character::Character", config.world_package_id);
    tracing::info!("Loading character names (type: {character_type})");

    let http = reqwest::Client::new();
    let mut cursor: Option<String> = None;
    let mut total = 0;

    loop {
        let after_clause = cursor
            .as_ref()
            .map(|c| format!(r#", after: "{}""#, c))
            .unwrap_or_default();

        let query = format!(
            r#"{{
                objects(filter: {{ type: "{character_type}" }}, first: 50{after_clause}) {{
                    nodes {{
                        asMoveObject {{
                            contents {{
                                json
                            }}
                        }}
                    }}
                    pageInfo {{
                        hasNextPage
                        endCursor
                    }}
                }}
            }}"#
        );

        let resp = http
            .post(&config.sui_graphql_url)
            .json(&serde_json::json!({ "query": query }))
            .send()
            .await?;

        let json: serde_json::Value = resp.json().await?;
        let nodes = json["data"]["objects"]["nodes"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        if nodes.is_empty() {
            break;
        }

        let mut s = state.write().await;
        for node in &nodes {
            let contents = &node["asMoveObject"]["contents"]["json"];
            if contents.is_null() {
                continue;
            }

            // Character object: {key: {item_id}, metadata: {name}, tribe_id, ...}
            let item_id =
                extract_item_id(&contents["key"]).or_else(|| extract_item_id(&contents["item_id"]));
            let name = contents["metadata"]["name"]
                .as_str()
                .or_else(|| contents["name"].as_str())
                .unwrap_or("")
                .to_string();
            let tribe_id = contents["tribe_id"]
                .as_u64()
                .or_else(|| contents["tribe_id"].as_str().and_then(|s| s.parse().ok()))
                .map(|id| id.to_string())
                .unwrap_or_default();

            if let Some(id) = item_id {
                if !name.is_empty() {
                    s.live.name_cache.insert(id, name.clone());
                }
                if let Some(profile) = s.live.profiles.get_mut(&id) {
                    if !name.is_empty() && profile.name.starts_with("Pilot #") {
                        profile.name = name;
                        profile.dirty = true;
                    }
                    if !tribe_id.is_empty() && profile.tribe_id.is_empty() {
                        profile.tribe_id = tribe_id;
                        profile.dirty = true;
                    }
                }
                total += 1;
            }
        }

        let page_info = &json["data"]["objects"]["pageInfo"];
        if !page_info["hasNextPage"].as_bool().unwrap_or(false) {
            break;
        }
        cursor = page_info["endCursor"].as_str().map(|s| s.to_string());

        if total % 100 == 0 && total > 0 {
            tracing::info!("Loaded {total} character names so far...");
        }
    }

    tracing::info!("Character name load complete: {total} names resolved");
    Ok(total)
}

/// Load historical killmails from Sui GraphQL and seed live profiles.
pub async fn load_historical_killmails(
    config: &AppConfig,
    state: &Arc<RwLock<AppState>>,
) -> Result<usize, Box<dyn std::error::Error>> {
    if config.world_package_id.is_empty() {
        tracing::warn!("WORLD_PACKAGE_ID not set, skipping historical killmail load");
        return Ok(0);
    }

    let killmail_type = format!("{}::killmail::Killmail", config.world_package_id);
    tracing::info!("Loading historical killmails (type: {killmail_type})");

    let http = reqwest::Client::new();
    let mut cursor: Option<String> = None;
    let mut total = 0;

    // Track existing profiles so we don't double-count stats from DB
    let existing_profiles: std::collections::HashSet<u64> = {
        let s = state.read().await;
        s.live.profiles.keys().copied().collect()
    };
    // If we already have kill events from DB, skip adding them (avoid duplicates)
    let has_existing_kills = {
        let s = state.read().await;
        let has = s.live.recent_events.iter().any(|e| e.event_type == "kill");
        tracing::info!(
            "Kill dedup check: {} events in deque, has_existing_kills={}",
            s.live.recent_events.len(),
            has
        );
        has
    };
    // Track seen killmail object IDs to deduplicate within this load
    let mut seen_ids: std::collections::HashSet<String> = std::collections::HashSet::new();

    loop {
        let after_clause = cursor
            .as_ref()
            .map(|c| format!(r#", after: "{}""#, c))
            .unwrap_or_default();

        let query = format!(
            r#"{{
                objects(filter: {{ type: "{killmail_type}" }}, first: 50{after_clause}) {{
                    nodes {{
                        asMoveObject {{
                            contents {{
                                json
                            }}
                        }}
                    }}
                    pageInfo {{
                        hasNextPage
                        endCursor
                    }}
                }}
            }}"#
        );

        let resp = http
            .post(&config.sui_graphql_url)
            .json(&serde_json::json!({ "query": query }))
            .send()
            .await?;

        let json: serde_json::Value = resp.json().await?;

        let nodes = json["data"]["objects"]["nodes"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        if nodes.is_empty() {
            break;
        }

        let mut s = state.write().await;
        for node in &nodes {
            let contents = &node["asMoveObject"]["contents"]["json"];
            if contents.is_null() {
                continue;
            }

            // Killmail fields are nested: {"item_id": "123", "tenant": "stillness"}
            let killer_id = extract_item_id(&contents["killer_id"]);
            let victim_id = extract_item_id(&contents["victim_id"]);
            let timestamp_secs = contents["kill_timestamp"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .or_else(|| contents["kill_timestamp"].as_u64())
                .unwrap_or(0);
            // Convert seconds to milliseconds
            let timestamp = if timestamp_secs < 10_000_000_000 {
                timestamp_secs * 1000
            } else {
                timestamp_secs
            };
            let system = extract_item_id_str(&contents["solar_system_id"]);

            // Deduplicate by killmail object ID
            let killmail_id = contents["id"]
                .as_str()
                .or_else(|| contents["key"]["item_id"].as_str())
                .unwrap_or("")
                .to_string();
            if killmail_id.is_empty() || !seen_ids.insert(killmail_id) {
                continue;
            }

            // Only update stats for profiles NOT already loaded from DB
            if let Some(kid) = killer_id {
                let is_new = !existing_profiles.contains(&kid);
                let profile = s.live.profiles.entry(kid).or_insert_with(|| ThreatProfile {
                    character_item_id: kid,
                    name: format!("Pilot #{kid}"),
                    ..Default::default()
                });
                if is_new {
                    profile.kill_count += 1;
                }
                profile.last_kill_timestamp = profile.last_kill_timestamp.max(timestamp);
                if !system.is_empty() {
                    profile.last_seen_system = system.clone();
                }
                profile.threat_score = threat_engine::compute_score(profile);
            }

            if let Some(vid) = victim_id {
                let is_new = !existing_profiles.contains(&vid);
                let profile = s.live.profiles.entry(vid).or_insert_with(|| ThreatProfile {
                    character_item_id: vid,
                    name: format!("Pilot #{vid}"),
                    ..Default::default()
                });
                if is_new {
                    profile.death_count += 1;
                }
                if !system.is_empty() {
                    profile.last_seen_system = system.clone();
                }
                profile.threat_score = threat_engine::compute_score(profile);
            }

            // Skip event if DB already has kill events
            if has_existing_kills {
                total += 1;
                continue;
            }
            s.live.push_event(
                RawEvent {
                    event_type: "kill".into(),
                    timestamp_ms: timestamp,
                    data: serde_json::json!({
                        "killer_character_id": killer_id,
                        "target_item_id": victim_id,
                        "solar_system_id": system,
                    }),
                },
                &None,
            );

            total += 1;
        }

        let page_info = &json["data"]["objects"]["pageInfo"];
        let has_next = page_info["hasNextPage"].as_bool().unwrap_or(false);

        if !has_next {
            break;
        }

        cursor = page_info["endCursor"].as_str().map(|s| s.to_string());

        tracing::info!("Loaded {total} historical killmails so far...");
    }

    tracing::info!(
        "Historical load complete: {total} killmails, {} profiles seeded",
        state.read().await.live.profiles.len()
    );

    Ok(total)
}

/// Load historical character creation events from Sui GraphQL.
pub async fn load_character_events(
    config: &AppConfig,
    state: &Arc<RwLock<AppState>>,
) -> Result<usize, Box<dyn std::error::Error>> {
    if config.world_package_id.is_empty() {
        return Ok(0);
    }

    let event_type = format!(
        "{}::character::CharacterCreatedEvent",
        config.world_package_id
    );
    tracing::info!("Loading character creation events");

    let has_existing_events = {
        let s = state.read().await;
        s.live
            .recent_events
            .iter()
            .any(|e| e.event_type == "new_character")
    };
    if has_existing_events {
        tracing::info!("Skipping character events — already have them from DB");
        return Ok(0);
    }

    let http = reqwest::Client::new();
    let mut cursor: Option<String> = None;
    let mut total = 0;

    loop {
        let after_clause = cursor
            .as_ref()
            .map(|c| format!(r#", after: "{}""#, c))
            .unwrap_or_default();

        let query = format!(
            r#"{{
                events(filter: {{ type: "{event_type}" }}, first: 50{after_clause}) {{
                    nodes {{
                        contents {{ json }}
                        timestamp
                    }}
                    pageInfo {{
                        hasNextPage
                        endCursor
                    }}
                }}
            }}"#
        );

        let resp = http
            .post(&config.sui_graphql_url)
            .json(&serde_json::json!({ "query": query }))
            .send()
            .await?;

        let json: serde_json::Value = resp.json().await?;
        let nodes = json["data"]["events"]["nodes"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        if nodes.is_empty() {
            break;
        }

        let mut s = state.write().await;
        for node in &nodes {
            let contents = &node["contents"]["json"];
            if contents.is_null() {
                continue;
            }

            let char_id = extract_item_id(&contents["key"]);
            let timestamp_str = node["timestamp"].as_str().unwrap_or("");
            let timestamp = chrono::DateTime::parse_from_rfc3339(timestamp_str)
                .map(|dt| dt.timestamp_millis() as u64)
                .unwrap_or(0);

            if let Some(id) = char_id {
                // Create profile if new
                s.live.profiles.entry(id).or_insert_with(|| ThreatProfile {
                    character_item_id: id,
                    name: format!("Pilot #{id}"),
                    ..Default::default()
                });

                s.live.push_event(
                    RawEvent {
                        event_type: "new_character".into(),
                        timestamp_ms: timestamp,
                        data: serde_json::json!({ "character_id": id }),
                    },
                    &None,
                );

                total += 1;
            }
        }

        let page_info = &json["data"]["events"]["pageInfo"];
        if !page_info["hasNextPage"].as_bool().unwrap_or(false) {
            break;
        }
        cursor = page_info["endCursor"].as_str().map(|s| s.to_string());
    }

    tracing::info!("Loaded {total} character creation events");
    Ok(total)
}

/// Resolve a gate's display name from its Sui object ID.
async fn resolve_gate_name(
    http: &reqwest::Client,
    graphql_url: &str,
    gate_id: &str,
    cache: &mut std::collections::HashMap<String, String>,
) -> String {
    if let Some(name) = cache.get(gate_id) {
        return name.clone();
    }
    let query = format!(
        r#"{{ object(address: "{gate_id}") {{ asMoveObject {{ contents {{ json }} }} }} }}"#
    );
    let name = match http
        .post(graphql_url)
        .json(&serde_json::json!({ "query": query }))
        .send()
        .await
    {
        Ok(resp) => {
            let json: serde_json::Value = resp.json().await.unwrap_or_default();
            let contents = &json["data"]["object"]["asMoveObject"]["contents"]["json"];
            // Try metadata.name, then fall back to key.item_id
            let name = contents["metadata"]["name"]
                .as_str()
                .filter(|s| !s.is_empty());
            let item_id = contents["key"]["item_id"].as_str();
            match (name, item_id) {
                (Some(n), _) => n.to_string(),
                (None, Some(id)) => format!("Gate #{id}"),
                _ => {
                    tracing::debug!("Could not resolve gate {gate_id}");
                    format!("Gate {}", &gate_id[..8.min(gate_id.len())])
                }
            }
        }
        Err(e) => {
            tracing::debug!("Failed to query gate {gate_id}: {e}");
            format!("Gate {}", &gate_id[..8.min(gate_id.len())])
        }
    };
    cache.insert(gate_id.to_string(), name.clone());
    name
}

/// Load historical jump events from Sui GraphQL.
pub async fn load_jump_events(
    config: &AppConfig,
    state: &Arc<RwLock<AppState>>,
) -> Result<usize, Box<dyn std::error::Error>> {
    if config.world_package_id.is_empty() {
        return Ok(0);
    }

    let event_type = format!("{}::gate::JumpEvent", config.world_package_id);
    tracing::info!("Loading jump events");

    let has_existing = {
        let s = state.read().await;
        s.live.recent_events.iter().any(|e| e.event_type == "jump")
    };
    if has_existing {
        tracing::info!("Skipping jump events — already have them from DB");
        return Ok(0);
    }

    let http = reqwest::Client::new();
    let mut cursor: Option<String> = None;
    let mut total = 0;
    let mut gate_name_cache: std::collections::HashMap<String, String> =
        std::collections::HashMap::new();

    loop {
        let after_clause = cursor
            .as_ref()
            .map(|c| format!(r#", after: "{}""#, c))
            .unwrap_or_default();

        let query = format!(
            r#"{{
                events(filter: {{ type: "{event_type}" }}, first: 50{after_clause}) {{
                    nodes {{
                        contents {{ json }}
                        timestamp
                    }}
                    pageInfo {{
                        hasNextPage
                        endCursor
                    }}
                }}
            }}"#
        );

        let resp = http
            .post(&config.sui_graphql_url)
            .json(&serde_json::json!({ "query": query }))
            .send()
            .await?;

        let json: serde_json::Value = resp.json().await?;
        let nodes = json["data"]["events"]["nodes"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        if nodes.is_empty() {
            break;
        }

        // Resolve gate names outside the state lock
        let mut jump_data: Vec<(Option<u64>, u64, String, String)> = Vec::new();
        for node in &nodes {
            let contents = &node["contents"]["json"];
            if contents.is_null() {
                continue;
            }

            let char_id = extract_item_id(&contents["character_key"]);
            let timestamp_str = node["timestamp"].as_str().unwrap_or("");
            let timestamp = chrono::DateTime::parse_from_rfc3339(timestamp_str)
                .map(|dt| dt.timestamp_millis() as u64)
                .unwrap_or(0);

            let source_gate = contents["source_gate_id"].as_str().unwrap_or("");
            let dest_gate = contents["destination_gate_id"].as_str().unwrap_or("");

            let source_name = resolve_gate_name(
                &http,
                &config.sui_graphql_url,
                source_gate,
                &mut gate_name_cache,
            )
            .await;
            let dest_name = resolve_gate_name(
                &http,
                &config.sui_graphql_url,
                dest_gate,
                &mut gate_name_cache,
            )
            .await;

            jump_data.push((char_id, timestamp, source_name, dest_name));
        }

        let mut s = state.write().await;
        for (char_id, timestamp, source_name, dest_name) in &jump_data {
            if let Some(id) = char_id {
                let profile = s.live.profiles.entry(*id).or_insert_with(|| ThreatProfile {
                    character_item_id: *id,
                    name: format!("Pilot #{id}"),
                    ..Default::default()
                });
                profile.systems_visited += 1;
                profile.threat_score = crate::threat_engine::compute_score(profile);

                s.live.push_event(
                    RawEvent {
                        event_type: "jump".into(),
                        timestamp_ms: *timestamp,
                        data: serde_json::json!({
                            "character_id": id,
                            "source_gate": source_name,
                            "dest_gate": dest_name,
                        }),
                    },
                    &None,
                );

                total += 1;
            }
        }

        let page_info = &json["data"]["events"]["pageInfo"];
        if !page_info["hasNextPage"].as_bool().unwrap_or(false) {
            break;
        }
        cursor = page_info["endCursor"].as_str().map(|s| s.to_string());
    }

    tracing::info!("Loaded {total} jump events");
    Ok(total)
}
