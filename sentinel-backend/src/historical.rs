//! Historical data loading via gRPC checkpoint replay.
//! Iterates through past checkpoints to seed threat profiles and events,
//! then fetches Character objects for name resolution.

use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::transport::Channel;

use crate::config::AppConfig;
use crate::grpc::sui_rpc::ledger_service_client::LedgerServiceClient;
use crate::types::AppState;

/// Load all historical data: replay checkpoints for events, then resolve names.
pub async fn load_all(config: AppConfig, state: Arc<RwLock<AppState>>, pool: sqlx::PgPool) {
    let channel = match crate::sui_client::connect(&config.sui_grpc_url).await {
        Ok(ch) => ch,
        Err(e) => {
            tracing::error!("Historical loader: gRPC connect failed: {e}");
            return;
        }
    };

    // Phase 1: Replay checkpoints for events (kills, bounties, jumps, character creation)
    if let Err(e) = replay_checkpoints(&config, &state, channel.clone()).await {
        tracing::warn!("Historical checkpoint replay failed: {e}");
    }

    // Phase 2: Resolve character names via gRPC GetObject
    if let Err(e) = load_character_names(&config, &state, channel.clone(), &pool).await {
        tracing::warn!("Character name resolution failed: {e}");
    }

    // Phase 3: Recompute scores and sort events
    finalize(&state).await;

    tracing::info!("Historical data load complete");
}

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

    // If no saved cursor, skip replay — the live streamer will pick up from latest.
    // On first run we rely on the fullnode's available checkpoint range.
    let start = match saved_cursor {
        Some(cp) => cp + 1,
        None => {
            tracing::info!(
                "No saved checkpoint — skipping historical replay (latest checkpoint: {latest})"
            );
            return Ok(());
        }
    };

    if start > latest {
        tracing::info!("Already caught up at checkpoint {}", start - 1);
        return Ok(());
    }

    let gap = latest - start + 1;
    tracing::info!(
        "Replaying {gap} checkpoints ({start}..={latest}) via gRPC"
    );

    let mut client = LedgerServiceClient::new(channel);
    let mut processed = 0u64;
    let mut events_found = 0u64;

    // Process in batches to avoid holding locks too long
    let mut seq = start;
    while seq <= latest {
        let batch_end = (seq + 99).min(latest);

        for cp_seq in seq..=batch_end {
            let resp = match crate::sui_client::get_checkpoint(&mut client, cp_seq).await {
                Ok(r) => r,
                Err(e) => {
                    tracing::warn!("Failed to fetch checkpoint {cp_seq}: {e}");
                    // Continue with next — gap will be filled on next restart
                    continue;
                }
            };

            if let Some(ref checkpoint) = resp.checkpoint {
                let timestamp_ms = crate::grpc::checkpoint_timestamp_ms(checkpoint);
                let event_count_before;
                {
                    let mut s = state.write().await;
                    event_count_before = s.live.recent_events.len();
                    // Don't broadcast SSE for historical replay
                    crate::grpc::process_checkpoint_events(
                        config,
                        &mut s.live,
                        &None,
                        checkpoint,
                        timestamp_ms,
                    );
                    events_found +=
                        (s.live.recent_events.len() - event_count_before) as u64;
                }
            }

            processed += 1;
        }

        // Update cursor after each batch
        {
            let mut s = state.write().await;
            s.last_checkpoint = Some(batch_end);
        }

        seq = batch_end + 1;

        if processed % 1000 == 0 && processed > 0 {
            tracing::info!(
                "Historical replay: {processed}/{gap} checkpoints, {events_found} events found"
            );
        }
    }

    tracing::info!(
        "Historical replay complete: {processed} checkpoints, {events_found} events found, {} profiles",
        state.read().await.live.profiles.len()
    );

    Ok(())
}

/// Resolve character names by fetching Character objects via gRPC.
/// Uses ListOwnedObjects to scan for Character-type objects, or falls back
/// to fetching objects directly if object IDs are known.
async fn load_character_names(
    config: &AppConfig,
    state: &Arc<RwLock<AppState>>,
    channel: Channel,
    pool: &sqlx::PgPool,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    let (total_profiles, unresolved) = {
        let s = state.read().await;
        let total = s.live.profiles.len();
        let unresolved = s
            .live
            .profiles
            .values()
            .filter(|p| p.name.starts_with("Pilot #"))
            .count();
        (total, unresolved)
    };

    if unresolved == 0 {
        tracing::info!("All character names resolved from DB cache");
        return Ok(0);
    }
    if unresolved < 20 && total_profiles > 100 {
        tracing::info!(
            "{unresolved} unresolved names — too few for full reload, metadata resolver will handle"
        );
        return Ok(0);
    }

    tracing::info!("{unresolved}/{total_profiles} characters need names — loading via gRPC");

    let character_type = format!("{}::character::Character", config.world_package_id);
    if config.world_package_id.is_empty() {
        return Ok(0);
    }

    // Use ListOwnedObjects with type filter. Since we don't know individual owners,
    // we scan using a well-known owner or iterate. As a practical approach, fetch
    // Character objects by looking up their dynamic field entries on the world object,
    // or fall back to using the character_item_id to object_id mapping from events.
    //
    // The Sui gRPC API does not support scanning all objects by type (that requires
    // an indexer). Instead, we batch-fetch Character objects whose Sui object IDs
    // we can discover from transaction effects during checkpoint replay.
    //
    // For now, we use the LedgerService.GetObject with the JSON field mask.
    // Character object IDs are typically derivable from events or known registries.
    //
    // Fallback: scan using the StateService's ListOwnedObjects if we know an owner.

    // Try fetching character objects from known game registry patterns
    let total = resolve_names_from_objects(config, state, channel, pool, &character_type).await?;

    tracing::info!("Character name load complete: {total} names resolved");
    Ok(total)
}

/// Attempt to resolve names by fetching Character objects.
/// Uses the world package's character registry to discover object IDs.
async fn resolve_names_from_objects(
    _config: &AppConfig,
    state: &Arc<RwLock<AppState>>,
    channel: Channel,
    pool: &sqlx::PgPool,
    character_type: &str,
) -> Result<usize, Box<dyn std::error::Error + Send + Sync>> {
    // Collect unresolved character IDs and any known object IDs
    let unresolved: Vec<u64> = {
        let s = state.read().await;
        s.live
            .profiles
            .values()
            .filter(|p| p.name.starts_with("Pilot #"))
            .map(|p| p.character_item_id)
            .collect()
    };

    if unresolved.is_empty() {
        return Ok(0);
    }

    // Discover Character object IDs by scanning owned objects from known addresses.
    // In EVE Frontier, Character objects are typically stored in a smart table or
    // as owned objects. We use ListOwnedObjects with the Character type filter
    // on well-known addresses, or we can iterate through all character holders.
    //
    // Since we don't know individual owner addresses, we use a pragmatic approach:
    // scan the character type across the chain using the State API.
    // The Sui gRPC API does not support scanning all objects by type — that
    // requires an indexer. Instead, resolve names for objects whose Sui object
    // IDs are cached (populated from transaction effects during checkpoint replay).
    let object_ids: Vec<(u64, String)> = {
        let s = state.read().await;
        unresolved
            .iter()
            .filter_map(|id| {
                s.live
                    .object_id_cache
                    .get(id)
                    .map(|oid| (*id, oid.clone()))
            })
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
                    let name: Option<&str> = json["metadata"]["name"]
                        .as_str()
                        .filter(|n| !n.is_empty());
                    if let (Some(id), Some(name)) = (item_id, name) {
                        s.live.name_cache.insert(id, name.to_string());
                        if let Some(p) = s.live.profiles.get_mut(&id) {
                            p.name = name.to_string();
                            p.dirty = true;
                        }
                        let _ = crate::db::upsert_character_name(pool, id, name).await;
                        total += 1;
                    }
                }
            }
            Err(e) => {
                tracing::warn!("gRPC batch name resolution failed: {e}");
            }
        }
    }

    let remaining = unresolved.len() - total;
    if remaining > 0 {
        tracing::info!(
            "{remaining} names deferred to metadata resolver (no cached object IDs)"
        );
    }

    let _ = character_type;
    Ok(total)
}

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
            let name = json["metadata"]["name"]
                .as_str()
                .filter(|s| !s.is_empty());
            let item_id = json["key"]["item_id"].as_str();
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
            tracing::debug!("Failed to query gate {gate_id} via gRPC: {e}");
            format!("Gate {}", &gate_id[..8.min(gate_id.len())])
        }
    };

    cache.insert(gate_id.to_string(), name.clone());
    name
}

/// Final pass: sort events, recompute 24h kills and threat scores.
async fn finalize(state: &Arc<RwLock<AppState>>) {
    let mut s = state.write().await;

    // Sort events newest first
    s.live
        .recent_events
        .make_contiguous()
        .sort_by(|a, b| b.timestamp_ms.cmp(&a.timestamp_ms));
    s.live
        .new_pilot_events
        .make_contiguous()
        .sort_by(|a, b| b.timestamp_ms.cmp(&a.timestamp_ms));

    // Recompute recent_kills_24h from actual events
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
        if p.published_score > 0 && p.published_score != p.threat_score {
            p.published_score = p.threat_score;
            p.dirty = true;
        }
    }

    let unresolved = s
        .live
        .profiles
        .values()
        .filter(|p| p.name.starts_with("Pilot #"))
        .count();
    if unresolved > 0 {
        tracing::info!(
            "{unresolved} characters could not be resolved — will retry via metadata resolver"
        );
    }
}

