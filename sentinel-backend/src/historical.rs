//! Historical data loading via GraphQL (indexed queries) and gRPC (checkpoint replay).
//!
//! GraphQL is used for initial cold-start loading (killmails, character events,
//! jump events, character names) since these require indexer-backed queries that
//! gRPC cannot serve. gRPC checkpoint replay fills any gap between restarts.

use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::transport::Channel;

use crate::config::AppConfig;
use crate::grpc::sui_rpc::ledger_service_client::LedgerServiceClient;
use crate::threat_engine;
use crate::types::{AppState, RawEvent, ThreatProfile};

/// Load all historical data: GraphQL for indexed queries, gRPC for checkpoint replay.
pub async fn load_all(
    config: AppConfig,
    state: Arc<RwLock<AppState>>,
    pool: sqlx::PgPool,
    world_client: std::sync::Arc<tokio::sync::RwLock<crate::world_api::WorldApiClient>>,
) {
    macro_rules! load {
        ($fn:expr, $name:expr) => {
            if let Err(e) = $fn.await {
                tracing::warn!("{} failed: {e}", $name);
            }
        };
    }

    // Phase 1: GraphQL historical loads (indexed queries — killmails, events, names)
    load!(
        load_historical_killmails(&config, &state),
        "Historical killmails"
    );
    load!(load_character_events(&config, &state), "Character events");
    load!(load_jump_events(&config, &state), "Jump events");
    load!(
        load_character_names_graphql(&config, &state, &pool),
        "Character names (GraphQL)"
    );
    load!(
        load_structure_types(&config, &state, &pool, &world_client),
        "Structure types"
    );

    // Phase 2: gRPC checkpoint replay (fills gap between saved cursor and latest)
    let channel = match crate::sui_client::connect(&config.sui_grpc_url).await {
        Ok(ch) => ch,
        Err(e) => {
            tracing::error!("Historical loader: gRPC connect failed: {e}");
            // Still finalize even if gRPC fails
            finalize(&state).await;
            tracing::info!("Historical data load complete (gRPC unavailable)");
            return;
        }
    };

    if let Err(e) = replay_checkpoints(&config, &state, channel.clone()).await {
        tracing::warn!("Historical checkpoint replay failed: {e}");
    }

    // Phase 3: Resolve any remaining names via gRPC batch fetch
    if let Err(e) = load_character_names_grpc(&config, &state, channel, &pool).await {
        tracing::warn!("Character name resolution (gRPC) failed: {e}");
    }

    // Phase 4: Recompute scores and sort events
    finalize(&state).await;

    tracing::info!("Historical data load complete");
}

// ---------------------------------------------------------------------------
// GraphQL-based historical loaders (indexed queries)
// ---------------------------------------------------------------------------

/// Extract a numeric item_id from nested {"item_id": "123", "tenant": "..."} or plain value.
fn extract_item_id(v: &serde_json::Value) -> Option<u64> {
    v.get("item_id")
        .and_then(|id| {
            id.as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .or_else(|| id.as_u64())
        })
        .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
        .or_else(|| v.as_u64())
}

/// Extract item_id as string (for solar_system_id).
fn extract_item_id_str(v: &serde_json::Value) -> String {
    if let Some(id) = v.get("item_id") {
        return id.as_str().unwrap_or("").to_string();
    }
    v.as_str().map(|s| s.to_string()).unwrap_or_default()
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

    let existing_profiles: std::collections::HashSet<u64> = {
        let s = state.read().await;
        s.live.profiles.keys().copied().collect()
    };

    // If profiles are already seeded from the DB, skip the full GraphQL scan.
    // New killmails since the last saved checkpoint are caught by the gRPC replay.
    if !existing_profiles.is_empty() {
        tracing::info!(
            profiles = existing_profiles.len(),
            "Skipping historical killmail load — profiles already seeded from DB"
        );
        return Ok(existing_profiles.len());
    }

    let has_existing_kills = {
        let s = state.read().await;
        s.live.recent_events.iter().any(|e| e.event_type == "kill")
    };
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

            let killer_id = extract_item_id(&contents["killer_id"]);
            let victim_id = extract_item_id(&contents["victim_id"]);
            let reported_by_id = extract_item_id(&contents["reported_by_character_id"]);
            let loss_is_structure = contents["loss_type"]["@variant"]
                .as_str()
                .map(|v| v == "STRUCTURE")
                .unwrap_or(false);
            let is_ship_kill = !loss_is_structure;
            let killed_by_structure = is_ship_kill
                && killer_id
                    .map(|k| k >= STRUCTURE_ITEM_ID_MIN)
                    .unwrap_or(false);
            let credited_killer = if killed_by_structure {
                reported_by_id.or(killer_id)
            } else {
                killer_id
            };
            let timestamp_secs = contents["kill_timestamp"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .or_else(|| contents["kill_timestamp"].as_u64())
                .unwrap_or(0);
            let timestamp = if timestamp_secs < 10_000_000_000 {
                timestamp_secs * 1000
            } else {
                timestamp_secs
            };
            let system = extract_item_id_str(&contents["solar_system_id"]);

            let killmail_id = contents["id"]
                .as_str()
                .or_else(|| contents["key"]["item_id"].as_str())
                .unwrap_or("")
                .to_string();
            if killmail_id.is_empty() || !seen_ids.insert(killmail_id) {
                continue;
            }

            let now_ms = chrono::Utc::now().timestamp_millis() as u64;
            let is_recent = timestamp > now_ms.saturating_sub(86_400_000);

            if let Some(kid) = credited_killer {
                let is_new = !existing_profiles.contains(&kid);
                let profile = s.live.profiles.entry(kid).or_insert_with(|| ThreatProfile {
                    character_item_id: kid,
                    name: None,
                    ..Default::default()
                });
                if is_new {
                    profile.kill_count += 1;
                    if is_recent {
                        profile.recent_kills_24h += 1;
                    }
                }
                profile.last_kill_timestamp = profile.last_kill_timestamp.max(timestamp);
                if !system.is_empty() {
                    profile.last_seen_system = system.clone();
                }
                profile.threat_score = threat_engine::compute_score(profile);
            }

            if is_ship_kill {
                if let Some(vid) = victim_id {
                    let is_new = !existing_profiles.contains(&vid);
                    let profile = s.live.profiles.entry(vid).or_insert_with(|| ThreatProfile {
                        character_item_id: vid,
                        name: None,
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
            }

            if has_existing_kills {
                total += 1;
                continue;
            }
            let evt_type = if is_ship_kill {
                "kill"
            } else {
                "structure_destroyed"
            };
            let mut evt_data = serde_json::json!({
                "killer_character_id": credited_killer,
                "target_item_id": victim_id,
                "solar_system_id": system,
            });
            if killed_by_structure {
                let structure_name = s.live.resolve_structure_name(killer_id.unwrap());
                evt_data["killed_by_structure"] = serde_json::json!(true);
                evt_data["structure_name"] = serde_json::json!(structure_name);
            }
            s.live.push_event(
                RawEvent {
                    event_type: evt_type.into(),
                    timestamp_ms: timestamp,
                    data: evt_data,
                },
                &None,
            );

            total += 1;
        }

        let page_info = &json["data"]["objects"]["pageInfo"];
        if !page_info["hasNextPage"].as_bool().unwrap_or(false) {
            break;
        }
        cursor = page_info["endCursor"].as_str().map(|s| s.to_string());

        if total % 100 == 0 && total > 0 {
            tracing::info!("Loaded {total} historical killmails so far...");
        }
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

    let existing_profiles = {
        let s = state.read().await;
        s.live.profiles.len()
    };
    if existing_profiles > 0 {
        tracing::info!(
            profiles = existing_profiles,
            "Skipping character events — profiles already seeded from DB"
        );
        return Ok(existing_profiles);
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
                s.live.profiles.entry(id).or_insert_with(|| ThreatProfile {
                    character_item_id: id,
                    name: None,
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

    let (has_existing, existing_profiles) = {
        let s = state.read().await;
        let has = s.live.recent_events.iter().any(|e| e.event_type == "jump");
        let profiles: std::collections::HashSet<u64> = s.live.profiles.keys().copied().collect();
        (has, profiles)
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

            let source_name = resolve_gate_name_graphql(
                &http,
                &config.sui_graphql_url,
                source_gate,
                &mut gate_name_cache,
            )
            .await;
            let dest_name = resolve_gate_name_graphql(
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
                let is_new = !existing_profiles.contains(id);
                let profile = s.live.profiles.entry(*id).or_insert_with(|| ThreatProfile {
                    character_item_id: *id,
                    name: None,
                    ..Default::default()
                });
                if is_new {
                    profile.systems_visited += 1;
                }
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

/// Load character names from Sui GraphQL (indexed object scan).
pub async fn load_character_names_graphql(
    config: &AppConfig,
    state: &Arc<RwLock<AppState>>,
    pool: &sqlx::PgPool,
) -> Result<usize, Box<dyn std::error::Error>> {
    let (total_profiles, unresolved) = {
        let s = state.read().await;
        let total = s.live.profiles.len();
        // Use name_cache presence (not p.name) so that permanently-unresolvable
        // characters (marked with "" in the cache after a prior scan) are not retried.
        let unresolved = s
            .live
            .profiles
            .keys()
            .filter(|id| !s.live.name_cache.contains_key(id))
            .count();
        (total, unresolved)
    };

    if unresolved == 0 {
        tracing::info!("All character names resolved from DB cache");
        return Ok(0);
    }

    tracing::info!(
        unresolved,
        total_profiles,
        "Loading character names from GraphQL"
    );

    let character_type = format!("{}::character::Character", config.world_package_id);

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
                        address
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

            let item_id =
                extract_item_id(&contents["key"]).or_else(|| extract_item_id(&contents["item_id"]));
            let address = node["address"].as_str();
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
                // Cache the Sui object address for future gRPC lookups
                if let Some(addr) = address {
                    s.live.object_id_cache.insert(id, addr.to_string());
                }
                if !name.is_empty() {
                    s.live.name_cache.insert(id, name.clone());
                    let _ = crate::db::upsert_character_name(pool, id, &name).await;
                }
                if let Some(profile) = s.live.profiles.get_mut(&id) {
                    if !name.is_empty() && profile.name.is_none() {
                        profile.name = Some(name);
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

    tracing::info!("Character name load (GraphQL) complete: {total} names resolved");

    // Mark any characters that were scanned but still have no name as permanently
    // unresolvable (deleted accounts, test accounts with no name in the API).
    // Storing "" in name_cache + DB means subsequent startups skip them.
    {
        let mut s = state.write().await;
        let unresolvable: Vec<u64> = s
            .live
            .profiles
            .keys()
            .filter(|id| !s.live.name_cache.contains_key(id))
            .copied()
            .collect();
        let count = unresolvable.len();
        for id in &unresolvable {
            s.live.name_cache.insert(*id, String::new());
        }
        drop(s);
        for id in &unresolvable {
            let _ = crate::db::upsert_character_name(pool, *id, "").await;
        }
        if count > 0 {
            tracing::info!(
                "Marked {count} characters as permanently unresolvable — will skip on next startup"
            );
        }
    }

    Ok(total)
}

/// Resolve a gate's display name via GraphQL (used during historical jump event loading).
async fn resolve_gate_name_graphql(
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
            let name = contents["metadata"]["name"]
                .as_str()
                .filter(|s| !s.is_empty());
            let item_id = contents["key"]["item_id"].as_str();
            match (name, item_id) {
                (Some(n), _) => n.to_string(),
                (None, Some(id)) => format!("Gate #{id}"),
                _ => {
                    tracing::debug!(gate_id, "Could not resolve gate {gate_id}");
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

// ---------------------------------------------------------------------------
// gRPC-based checkpoint replay (fills gaps between restarts)
// ---------------------------------------------------------------------------

/// Replay checkpoints from saved cursor to latest, processing all relevant events.
async fn replay_checkpoints(
    config: &AppConfig,
    state: &Arc<RwLock<AppState>>,
    channel: Channel,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    if config.world_package_id.is_empty() {
        tracing::warn!("WORLD_PACKAGE_ID not set, skipping historical checkpoint replay");
        return Ok(());
    }

    let saved_cursor = state.read().await.last_checkpoint;
    let latest = crate::sui_client::get_latest_checkpoint(channel.clone()).await?;

    let start = match saved_cursor {
        Some(cp) => cp + 1,
        None => {
            tracing::info!(
                latest_checkpoint = latest,
                "No saved checkpoint — skipping gRPC replay (latest checkpoint: {latest})"
            );
            return Ok(());
        }
    };

    if start > latest {
        tracing::info!(
            checkpoint = start - 1,
            "Already caught up at checkpoint {}",
            start - 1
        );
        return Ok(());
    }

    let gap = latest - start + 1;
    tracing::info!(
        gap,
        start,
        latest,
        "Replaying {gap} checkpoints ({start}..={latest}) via gRPC"
    );

    let mut client = LedgerServiceClient::new(channel.clone());
    let mut processed = 0u64;
    let mut events_found = 0u64;

    let mut seq = start;
    while seq <= latest {
        let batch_end = (seq + 99).min(latest);

        for cp_seq in seq..=batch_end {
            let resp = match crate::sui_client::get_checkpoint(&mut client, cp_seq).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!(checkpoint = cp_seq, error = %e, "Failed to fetch checkpoint {cp_seq}: {e}");
                    continue;
                }
            };

            if let Some(ref checkpoint) = resp.checkpoint {
                let timestamp_ms = crate::grpc::checkpoint_timestamp_ms(checkpoint);
                let event_count_before;
                {
                    let mut s = state.write().await;
                    event_count_before = s.live.recent_events.len();
                    crate::grpc::process_checkpoint_events(
                        config,
                        &mut s.live,
                        &None,
                        checkpoint,
                        timestamp_ms,
                    );
                    events_found += (s.live.recent_events.len() - event_count_before) as u64;
                }
            }

            processed += 1;
        }

        {
            let mut s = state.write().await;
            s.last_checkpoint = Some(batch_end);
        }

        seq = batch_end + 1;

        if processed % 1000 == 0 && processed > 0 {
            tracing::info!(
                processed,
                gap,
                events_found,
                "Historical replay: {processed}/{gap} checkpoints, {events_found} events found"
            );
        }
    }

    let profile_count = state.read().await.live.profiles.len();
    tracing::info!(
        processed,
        events_found,
        profiles = profile_count,
        "Historical replay complete: {processed} checkpoints, {events_found} events found, {profile_count} profiles"
    );

    // Post-replay: resolve gate names for any jump events from replay
    resolve_replay_gate_names(state, channel).await;

    Ok(())
}

/// After checkpoint replay, resolve any gate IDs in recent jump events that
/// aren't cached yet. Updates both the cache and the event data in place.
async fn resolve_replay_gate_names(state: &Arc<RwLock<AppState>>, channel: Channel) {
    // Collect uncached gate IDs from recent jump events
    let uncached: Vec<String> = {
        let s = state.read().await;
        let mut ids: Vec<String> = s
            .live
            .recent_events
            .iter()
            .filter(|e| e.event_type == "jump")
            .flat_map(|e| {
                let mut gate_ids = Vec::new();
                if let Some(id) = e.data["source_gate_id"].as_str() {
                    if !id.is_empty() && !s.live.gate_name_cache.contains_key(id) {
                        gate_ids.push(id.to_string());
                    }
                }
                if let Some(id) = e.data["dest_gate_id"].as_str() {
                    if !id.is_empty() && !s.live.gate_name_cache.contains_key(id) {
                        gate_ids.push(id.to_string());
                    }
                }
                gate_ids
            })
            .collect();
        ids.sort();
        ids.dedup();
        ids
    };

    if uncached.is_empty() {
        return;
    }

    tracing::info!(
        count = uncached.len(),
        "Resolving {} gate names from replay jump events",
        uncached.len()
    );

    // Resolve each gate name via gRPC
    {
        let mut s = state.write().await;
        for gate_id in &uncached {
            resolve_gate_name(channel.clone(), gate_id, &mut s.live.gate_name_cache).await;
        }
    }

    // Patch jump events with resolved names
    {
        let mut s = state.write().await;
        let cache = s.live.gate_name_cache.clone();
        for event in s.live.recent_events.iter_mut() {
            if event.event_type != "jump" {
                continue;
            }
            if let Some(id) = event.data["source_gate_id"].as_str() {
                if let Some(name) = cache.get(id) {
                    event.data["source_gate"] = serde_json::Value::String(name.clone());
                }
            }
            if let Some(id) = event.data["dest_gate_id"].as_str() {
                if let Some(name) = cache.get(id) {
                    event.data["dest_gate"] = serde_json::Value::String(name.clone());
                }
            }
        }
    }

    tracing::info!("Gate name resolution complete");
}

// ---------------------------------------------------------------------------
// gRPC-based character name resolution (for names not found via GraphQL)
// ---------------------------------------------------------------------------

/// Resolve character names via gRPC BatchGetObjects (using cached object IDs).
async fn load_character_names_grpc(
    config: &AppConfig,
    state: &Arc<RwLock<AppState>>,
    channel: Channel,
    pool: &sqlx::PgPool,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    if config.world_package_id.is_empty() {
        return Ok(0);
    }

    let unresolved: Vec<u64> = {
        let s = state.read().await;
        s.live
            .profiles
            .keys()
            .filter(|id| !s.live.name_cache.contains_key(id))
            .copied()
            .collect()
    };

    if unresolved.is_empty() {
        return Ok(0);
    }

    tracing::info!(
        count = unresolved.len(),
        "{} characters still need names — trying gRPC batch fetch",
        unresolved.len()
    );

    let object_ids: Vec<(u64, String)> = {
        let s = state.read().await;
        unresolved
            .iter()
            .filter_map(|id| s.live.object_id_cache.get(id).map(|oid| (*id, oid.clone())))
            .collect()
    };

    let mut total = 0;

    if !object_ids.is_empty() {
        let oids: Vec<String> = object_ids.iter().map(|(_, oid)| oid.clone()).collect();
        match crate::sui_client::batch_get_objects_json(channel, &oids).await {
            Ok(results) => {
                let mut s = state.write().await;
                for (_oid, json) in &results {
                    let item_id = json["key"]["item_id"]
                        .as_str()
                        .and_then(|v| v.parse::<u64>().ok());
                    let name: Option<&str> =
                        json["metadata"]["name"].as_str().filter(|n| !n.is_empty());
                    if let (Some(id), Some(name)) = (item_id, name) {
                        s.live.name_cache.insert(id, name.to_string());
                        if let Some(p) = s.live.profiles.get_mut(&id) {
                            p.name = Some(name.to_string());
                            p.dirty = true;
                        }
                        let _ = crate::db::upsert_character_name(pool, id, name).await;
                        total += 1;
                    }
                }
            }
            Err(e) => {
                tracing::warn!(error = %e, "gRPC batch name resolution failed: {e}");
            }
        }
    }

    let remaining = unresolved.len() - total;
    if remaining > 0 {
        tracing::info!(
            count = remaining,
            "{remaining} names deferred to metadata resolver (no cached object IDs)"
        );
    }

    Ok(total)
}

// ---------------------------------------------------------------------------
// Structure type resolution
// ---------------------------------------------------------------------------

/// Known deployable structure types in the world package.
const STRUCTURE_TYPES: &[&str] = &["gate::Gate", "turret::Turret", "assembly::Assembly"];

/// Minimum item_id that identifies a structure (vs a character).
/// Characters are ~2.1B; structures are ~1T.
pub const STRUCTURE_ITEM_ID_MIN: u64 = 1_000_000_000_000;

/// Scan Gate, Turret, and Assembly objects via GraphQL to build the
/// item_id → type_id cache, then resolve type names from the World API.
pub async fn load_structure_types(
    config: &AppConfig,
    state: &Arc<RwLock<AppState>>,
    pool: &sqlx::PgPool,
    world_client: &std::sync::Arc<tokio::sync::RwLock<crate::world_api::WorldApiClient>>,
) -> Result<usize, Box<dyn std::error::Error>> {
    if config.world_package_id.is_empty() {
        return Ok(0);
    }

    let http = reqwest::Client::new();
    let mut total = 0;
    let mut seen_type_ids: std::collections::HashSet<u64> = std::collections::HashSet::new();

    for struct_type in STRUCTURE_TYPES {
        let type_filter = format!("{}::{}", config.world_package_id, struct_type);
        let mut cursor: Option<String> = None;

        loop {
            let after_clause = cursor
                .as_ref()
                .map(|c| format!(r#", after: "{}""#, c))
                .unwrap_or_default();

            let query = format!(
                r#"{{
                    objects(filter: {{ type: "{type_filter}" }}, first: 50{after_clause}) {{
                        nodes {{
                            asMoveObject {{
                                contents {{ json }}
                            }}
                        }}
                        pageInfo {{ hasNextPage endCursor }}
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
                let item_id = extract_item_id(&contents["key"]);
                let type_id = contents["type_id"]
                    .as_u64()
                    .or_else(|| contents["type_id"].as_str().and_then(|v| v.parse().ok()));

                if let (Some(iid), Some(tid)) = (item_id, type_id) {
                    s.live.structure_type_cache.insert(iid, tid);
                    seen_type_ids.insert(tid);
                    let _ = crate::db::upsert_structure_type(pool, iid, tid).await;
                    total += 1;
                }
            }

            let page_info = &json["data"]["objects"]["pageInfo"];
            if !page_info["hasNextPage"].as_bool().unwrap_or(false) {
                break;
            }
            cursor = page_info["endCursor"].as_str().map(|s| s.to_string());
        }
    }

    // Resolve type names from World API for any newly seen type_ids
    for type_id in seen_type_ids {
        let already_cached = state
            .read()
            .await
            .live
            .type_name_cache
            .contains_key(&type_id);
        if !already_cached {
            let name = world_client.read().await.fetch_type_name(type_id).await;
            if let Some(name) = name {
                state
                    .write()
                    .await
                    .live
                    .type_name_cache
                    .insert(type_id, name);
            }
        }
    }

    let type_name_count = state.read().await.live.type_name_cache.len();
    tracing::info!(
        structures = total,
        type_names = type_name_count,
        "Structure type cache loaded: {total} structures, {type_name_count} type names"
    );
    Ok(total)
}

// ---------------------------------------------------------------------------
// Shared utilities
// ---------------------------------------------------------------------------

/// Resolve a gate's display name from its Sui object ID via gRPC.
pub async fn resolve_gate_name(
    channel: Channel,
    gate_id: &str,
    cache: &mut std::collections::HashMap<String, String>,
) -> String {
    if let Some(name) = cache.get(gate_id) {
        return name.clone();
    }

    let name = match crate::sui_client::get_object_json(channel, gate_id).await {
        Ok(json) => {
            let name = json["metadata"]["name"].as_str().filter(|s| !s.is_empty());
            let item_id = json["key"]["item_id"].as_str();
            match (name, item_id) {
                (Some(n), _) => n.to_string(),
                (None, Some(id)) => format!("Gate #{id}"),
                _ => {
                    tracing::debug!(gate_id, "Could not resolve gate {gate_id}");
                    format!("Gate {}", &gate_id[..8.min(gate_id.len())])
                }
            }
        }
        Err(e) => {
            tracing::debug!(gate_id, error = %e, "Failed to query gate {gate_id} via gRPC: {e}");
            format!("Gate {}", &gate_id[..8.min(gate_id.len())])
        }
    };

    cache.insert(gate_id.to_string(), name.clone());
    name
}

/// Final pass: sort events, recompute 24h kills and threat scores.
async fn finalize(state: &Arc<RwLock<AppState>>) {
    let mut s = state.write().await;

    s.live
        .recent_events
        .make_contiguous()
        .sort_by(|a, b| b.timestamp_ms.cmp(&a.timestamp_ms));
    s.live
        .new_pilot_events
        .make_contiguous()
        .sort_by(|a, b| b.timestamp_ms.cmp(&a.timestamp_ms));

    let now_ms = chrono::Utc::now().timestamp_millis() as u64;
    let day_ago = now_ms.saturating_sub(86_400_000);

    let mut kill_counts_24h: std::collections::HashMap<u64, u64> = std::collections::HashMap::new();
    for e in &s.live.recent_events {
        if e.event_type == "kill" && e.timestamp_ms >= day_ago {
            if let Some(kid) = e.data.get("killer_character_id").and_then(|v| v.as_u64()) {
                *kill_counts_24h.entry(kid).or_default() += 1;
            }
        }
    }

    for p in s.live.profiles.values_mut() {
        p.recent_kills_24h = kill_counts_24h
            .get(&p.character_item_id)
            .copied()
            .unwrap_or(0);
        p.threat_score = crate::threat_engine::compute_score(p);
    }

    let unresolved = s
        .live
        .profiles
        .values()
        .filter(|p| p.name.is_none())
        .count();
    if unresolved > 0 {
        tracing::info!(
            unresolved_count = unresolved,
            "{unresolved} characters could not be resolved — will retry via metadata resolver"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{AppState, DataStore, ThreatProfile};

    fn make_state_with_unresolved(count: usize, total: usize) -> AppState {
        let mut state = AppState::default();
        for i in 0..total {
            let resolved = i >= count;
            let name = if resolved {
                Some(format!("Player {i}"))
            } else {
                None
            };
            state.live.profiles.insert(
                i as u64,
                ThreatProfile {
                    character_item_id: i as u64,
                    name: name.clone(),
                    ..Default::default()
                },
            );
            if let Some(n) = name {
                state.live.name_cache.insert(i as u64, n);
            }
        }
        state
    }

    // --- GraphQL node processing (object_id_cache population) ---

    #[test]
    fn graphql_node_populates_object_id_cache() {
        // Simulates what load_character_names_graphql does for each node
        let mut store = DataStore::default();
        store.profiles.insert(
            42,
            ThreatProfile {
                character_item_id: 42,
                name: None,
                ..Default::default()
            },
        );

        let node = serde_json::json!({
            "address": "0xabc123",
            "asMoveObject": {
                "contents": {
                    "json": {
                        "key": { "item_id": "42" },
                        "metadata": { "name": "Vex Nightburn" },
                        "tribe_id": "7"
                    }
                }
            }
        });

        let contents = &node["asMoveObject"]["contents"]["json"];
        let item_id =
            extract_item_id(&contents["key"]).or_else(|| extract_item_id(&contents["item_id"]));
        let address = node["address"].as_str();
        let name = contents["metadata"]["name"]
            .as_str()
            .unwrap_or("")
            .to_string();

        if let Some(id) = item_id {
            if let Some(addr) = address {
                store.object_id_cache.insert(id, addr.to_string());
            }
            if !name.is_empty() {
                store.name_cache.insert(id, name.clone());
            }
            if let Some(profile) = store.profiles.get_mut(&id) {
                if !name.is_empty() && profile.name.is_none() {
                    profile.name = Some(name);
                    profile.dirty = true;
                }
            }
        }

        assert_eq!(
            store.object_id_cache.get(&42),
            Some(&"0xabc123".to_string())
        );
        assert_eq!(
            store.name_cache.get(&42),
            Some(&"Vex Nightburn".to_string())
        );
        assert_eq!(
            store.profiles.get(&42).unwrap().name,
            Some("Vex Nightburn".to_string())
        );
        assert!(store.profiles.get(&42).unwrap().dirty);
    }

    #[test]
    fn graphql_node_without_address_skips_object_id_cache() {
        let mut store = DataStore::default();

        let node = serde_json::json!({
            "asMoveObject": {
                "contents": {
                    "json": {
                        "key": { "item_id": "99" },
                        "metadata": { "name": "Ghost" }
                    }
                }
            }
        });

        let contents = &node["asMoveObject"]["contents"]["json"];
        let item_id = extract_item_id(&contents["key"]);
        let address = node["address"].as_str();

        if let Some(id) = item_id {
            if let Some(addr) = address {
                store.object_id_cache.insert(id, addr.to_string());
            }
            let name = contents["metadata"]["name"]
                .as_str()
                .unwrap_or("")
                .to_string();
            if !name.is_empty() {
                store.name_cache.insert(id, name);
            }
        }

        assert!(!store.object_id_cache.contains_key(&99));
        assert_eq!(store.name_cache.get(&99), Some(&"Ghost".to_string()));
    }

    #[test]
    fn graphql_node_skips_null_contents() {
        let node = serde_json::json!({
            "address": "0xdef456",
            "asMoveObject": { "contents": { "json": null } }
        });

        let contents = &node["asMoveObject"]["contents"]["json"];
        assert!(contents.is_null());
    }

    #[test]
    fn graphql_node_empty_name_not_cached() {
        let mut store = DataStore::default();
        store.profiles.insert(
            10,
            ThreatProfile {
                character_item_id: 10,
                name: None,
                ..Default::default()
            },
        );

        let node = serde_json::json!({
            "address": "0xfff",
            "asMoveObject": {
                "contents": {
                    "json": {
                        "key": { "item_id": "10" },
                        "metadata": { "name": "" }
                    }
                }
            }
        });

        let contents = &node["asMoveObject"]["contents"]["json"];
        let item_id = extract_item_id(&contents["key"]);
        let address = node["address"].as_str();
        let name = contents["metadata"]["name"]
            .as_str()
            .unwrap_or("")
            .to_string();

        if let Some(id) = item_id {
            if let Some(addr) = address {
                store.object_id_cache.insert(id, addr.to_string());
            }
            if !name.is_empty() {
                store.name_cache.insert(id, name.clone());
            }
        }

        // Object ID still cached even with empty name
        assert_eq!(store.object_id_cache.get(&10), Some(&"0xfff".to_string()));
        // But name not cached
        assert!(!store.name_cache.contains_key(&10));
        // Profile name unchanged (still None)
        assert!(store.profiles.get(&10).unwrap().name.is_none());
    }

    // --- finalize: unresolved count ---

    #[tokio::test]
    async fn finalize_counts_unresolved_profiles() {
        let state = Arc::new(RwLock::new(make_state_with_unresolved(3, 10)));
        finalize(&state).await;

        let s = state.read().await;
        let unresolved = s
            .live
            .profiles
            .values()
            .filter(|p| p.name.is_none())
            .count();
        assert_eq!(unresolved, 3);
    }

    #[tokio::test]
    async fn finalize_zero_unresolved_when_all_named() {
        let state = Arc::new(RwLock::new(make_state_with_unresolved(0, 5)));
        finalize(&state).await;

        let s = state.read().await;
        let unresolved = s
            .live
            .profiles
            .values()
            .filter(|p| p.name.is_none())
            .count();
        assert_eq!(unresolved, 0);
    }

    // --- gRPC name resolution: object_id_cache filtering ---

    #[test]
    fn grpc_resolver_filters_by_object_id_cache() {
        let mut store = DataStore::default();
        // Character with cached object ID
        store.profiles.insert(
            1,
            ThreatProfile {
                character_item_id: 1,
                name: None,
                ..Default::default()
            },
        );
        store.object_id_cache.insert(1, "0xaaa".to_string());

        // Character without cached object ID
        store.profiles.insert(
            2,
            ThreatProfile {
                character_item_id: 2,
                name: None,
                ..Default::default()
            },
        );

        // Character already resolved (should be skipped)
        store.profiles.insert(
            3,
            ThreatProfile {
                character_item_id: 3,
                name: Some("ResolvedPlayer".to_string()),
                ..Default::default()
            },
        );
        store.object_id_cache.insert(3, "0xccc".to_string());

        // Simulate the filtering from load_character_names_grpc
        let unresolved: Vec<u64> = store
            .profiles
            .values()
            .filter(|p| p.name.is_none())
            .map(|p| p.character_item_id)
            .collect();

        let with_oids: Vec<(u64, String)> = unresolved
            .iter()
            .filter_map(|id| store.object_id_cache.get(id).map(|oid| (*id, oid.clone())))
            .collect();

        // Only character 1 has both "Pilot #" name and cached object ID
        assert_eq!(with_oids.len(), 1);
        assert_eq!(with_oids[0].0, 1);
        assert_eq!(with_oids[0].1, "0xaaa");

        // Character 2 is unresolved but has no object ID — deferred
        assert_eq!(unresolved.len(), 2);
        let remaining = unresolved.len() - with_oids.len();
        assert_eq!(remaining, 1);
    }

    // --- extract_item_id ---

    #[test]
    fn extract_item_id_nested_string() {
        let v = serde_json::json!({"item_id": "42"});
        assert_eq!(extract_item_id(&v), Some(42));
    }

    #[test]
    fn extract_item_id_nested_number() {
        let v = serde_json::json!({"item_id": 42});
        assert_eq!(extract_item_id(&v), Some(42));
    }

    #[test]
    fn extract_item_id_bare_string() {
        let v = serde_json::json!("42");
        assert_eq!(extract_item_id(&v), Some(42));
    }

    #[test]
    fn extract_item_id_bare_number() {
        let v = serde_json::json!(42);
        assert_eq!(extract_item_id(&v), Some(42));
    }

    #[test]
    fn extract_item_id_null() {
        let v = serde_json::json!(null);
        assert_eq!(extract_item_id(&v), None);
    }
}
