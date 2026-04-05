//! World REST API client for resolving solar system names and tribe info.
//! Caches results in Postgres to avoid re-fetching across restarts.

use std::collections::HashMap;

use sqlx::PgPool;

/// Cached solar system and tribe data, backed by Postgres.
pub struct WorldApiClient {
    http: reqwest::Client,
    base_url: String,
    system_cache: HashMap<String, String>,
    tribe_cache: HashMap<String, TribeInfo>,
}

#[derive(Clone, Debug, serde::Serialize)]
pub struct TribeInfo {
    pub name: String,
    pub name_short: String,
}

impl WorldApiClient {
    /// Create a new client, pre-loading the system and tribe caches from Postgres.
    pub async fn new(base_url: &str, pool: &PgPool) -> Self {
        let mut client = Self {
            http: reqwest::Client::new(),
            base_url: base_url.trim_end_matches('/').to_string(),
            system_cache: HashMap::new(),
            tribe_cache: HashMap::new(),
        };

        // Load caches from DB
        if let Ok(rows) =
            sqlx::query_as::<_, (String, String)>("SELECT system_id, name FROM solar_system_cache")
                .fetch_all(pool)
                .await
        {
            for (id, name) in rows {
                client.system_cache.insert(id, name);
            }
        }

        if let Ok(rows) = sqlx::query_as::<_, (String, String, String)>(
            "SELECT tribe_id, name, name_short FROM tribe_cache",
        )
        .fetch_all(pool)
        .await
        {
            for (id, name, name_short) in rows {
                client
                    .tribe_cache
                    .insert(id, TribeInfo { name, name_short });
            }
        }

        tracing::info!(
            systems = client.system_cache.len(),
            tribes = client.tribe_cache.len(),
            "World API client loaded {} systems, {} tribes from cache",
            client.system_cache.len(),
            client.tribe_cache.len()
        );

        client
    }

    /// Get a cached system name, or None if not yet fetched.
    #[allow(dead_code)]
    pub fn get_system_name_cached(&self, system_id: &str) -> Option<&str> {
        self.system_cache.get(system_id).map(|s| s.as_str())
    }

    /// Get a cached tribe info, or None if not yet fetched.
    #[allow(dead_code)]
    pub fn get_tribe_cached(&self, tribe_id: &str) -> Option<&TribeInfo> {
        self.tribe_cache.get(tribe_id)
    }

    /// Fetch and cache a solar system name from the World API.
    pub async fn fetch_system_name(&mut self, system_id: &str, pool: &PgPool) -> Option<String> {
        if let Some(name) = self.system_cache.get(system_id) {
            return Some(name.clone());
        }

        let url = format!("{}/v2/solarsystems/{}", self.base_url, system_id);
        match self.http.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    if let Some(name) = json.get("name").and_then(|v| v.as_str()) {
                        let name = name.to_string();
                        self.system_cache
                            .insert(system_id.to_string(), name.clone());

                        // Persist to DB
                        let _ = sqlx::query(
                            "INSERT INTO solar_system_cache (system_id, name) VALUES ($1, $2) \
                             ON CONFLICT (system_id) DO NOTHING",
                        )
                        .bind(system_id)
                        .bind(&name)
                        .execute(pool)
                        .await;

                        return Some(name);
                    }
                }
            }
            Ok(resp) => {
                tracing::debug!("World API system {system_id}: status {}", resp.status());
            }
            Err(e) => {
                tracing::debug!("World API system {system_id} fetch failed: {e}");
            }
        }

        // Cache miss as empty to avoid re-fetching
        self.system_cache
            .insert(system_id.to_string(), String::new());
        None
    }

    /// Fetch a type name from the World API by type_id (e.g. 92279 → "Mini Turret").
    /// Results are not persisted to DB — caller stores in DataStore.type_name_cache.
    pub async fn fetch_type_name(&self, type_id: u64) -> Option<String> {
        let url = format!("{}/v2/types/{}", self.base_url, type_id);
        match self.http.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    return json
                        .get("name")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());
                }
            }
            Ok(resp) => tracing::debug!("World API type {type_id}: status {}", resp.status()),
            Err(e) => tracing::debug!("World API type {type_id} fetch failed: {e}"),
        }
        None
    }

    /// Fetch and cache tribe info from the World API.
    pub async fn fetch_tribe(&mut self, tribe_id: &str, pool: &PgPool) -> Option<TribeInfo> {
        if tribe_id.is_empty() {
            return None;
        }

        if let Some(info) = self.tribe_cache.get(tribe_id) {
            return Some(info.clone());
        }

        let url = format!("{}/v2/tribes/{}", self.base_url, tribe_id);
        match self.http.get(&url).send().await {
            Ok(resp) if resp.status().is_success() => {
                if let Ok(json) = resp.json::<serde_json::Value>().await {
                    let name = json
                        .get("name")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();
                    let name_short = json
                        .get("nameShort")
                        .and_then(|v| v.as_str())
                        .unwrap_or("")
                        .to_string();

                    let info = TribeInfo {
                        name: name.clone(),
                        name_short: name_short.clone(),
                    };
                    self.tribe_cache.insert(tribe_id.to_string(), info.clone());

                    let _ = sqlx::query(
                        "INSERT INTO tribe_cache (tribe_id, name, name_short) VALUES ($1, $2, $3) \
                         ON CONFLICT (tribe_id) DO NOTHING",
                    )
                    .bind(tribe_id)
                    .bind(&name)
                    .bind(&name_short)
                    .execute(pool)
                    .await;

                    return Some(info);
                }
            }
            Ok(resp) => {
                tracing::debug!("World API tribe {tribe_id}: status {}", resp.status());
            }
            Err(e) => {
                tracing::debug!("World API tribe {tribe_id} fetch failed: {e}");
            }
        }

        None
    }
}

/// Background loop that resolves pending system names and tribe affiliations.
/// Character names are resolved via the World REST API (no Sui queries needed).
pub async fn metadata_resolver_loop(
    world_client: std::sync::Arc<tokio::sync::RwLock<WorldApiClient>>,
    state: std::sync::Arc<tokio::sync::RwLock<crate::types::AppState>>,
    pool: PgPool,
    grpc_url: String,
    world_package_id: String,
) {
    let mut failed_names: std::collections::HashSet<u64> = std::collections::HashSet::new();

    loop {
        tokio::time::sleep(std::time::Duration::from_secs(3600)).await;

        // Collect pending system IDs and tribe IDs
        let (pending_systems, pending_tribes): (Vec<String>, Vec<String>) = {
            let s = state.read().await;
            let systems: Vec<String> = s
                .live
                .profiles
                .values()
                .filter(|p| !p.last_seen_system.is_empty() && p.last_seen_system_name.is_empty())
                .map(|p| p.last_seen_system.clone())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();

            let tribes: Vec<String> = s
                .live
                .profiles
                .values()
                .filter(|p| !p.tribe_id.is_empty() && p.tribe_name.is_empty())
                .map(|p| p.tribe_id.clone())
                .collect::<std::collections::HashSet<_>>()
                .into_iter()
                .collect();

            (systems, tribes)
        };

        // Resolve system names via World REST API
        for system_id in &pending_systems {
            // If the stored value is already a human-readable name (non-numeric, e.g. "J-1042"
            // stored incorrectly where a numeric ID should be), skip the API call and use it
            // directly as the display name.
            let name = if system_id.parse::<u64>().is_err() {
                tracing::debug!(
                    "system_id {system_id:?} is non-numeric, using as display name directly"
                );
                Some(system_id.clone())
            } else {
                let mut client = world_client.write().await;
                client.fetch_system_name(system_id, &pool).await
            };
            // fetch_system_name returns Some("") for known-bad IDs (cached 400/error).
            // Fall back to the raw system ID so the profile stops being queued.
            if let Some(name) = name {
                let display = if name.is_empty() {
                    system_id.clone()
                } else {
                    name
                };
                let mut s = state.write().await;
                s.live
                    .system_name_cache
                    .insert(system_id.clone(), display.clone());
                for profile in s.live.profiles.values_mut() {
                    if profile.last_seen_system == *system_id
                        && profile.last_seen_system_name.is_empty()
                    {
                        profile.last_seen_system_name = display.clone();
                        profile.dirty = true;
                    }
                }
            }
        }

        // Resolve tribe names via World REST API
        for tribe_id in &pending_tribes {
            let info = {
                let mut client = world_client.write().await;
                client.fetch_tribe(tribe_id, &pool).await
            };
            if let Some(info) = info {
                let mut s = state.write().await;
                for profile in s.live.profiles.values_mut() {
                    if profile.tribe_id == *tribe_id && profile.tribe_name.is_empty() {
                        profile.tribe_name = info.name.clone();
                        profile.dirty = true;
                    }
                }
            }
        }

        // First check DB cache for names resolved by historical loader
        {
            let mut s = state.write().await;
            let unresolved: Vec<u64> = s
                .live
                .profiles
                .values()
                .filter(|p| p.name.is_none())
                .map(|p| p.character_item_id)
                .collect();
            for id in &unresolved {
                if let Some(name) = s.live.name_cache.get(id) {
                    if !name.is_empty() {
                        let name = name.clone();
                        if let Some(p) = s.live.profiles.get_mut(id) {
                            p.name = Some(name);
                            p.dirty = true;
                        }
                    }
                }
            }
        }

        // Resolve remaining unresolved character names via gRPC.
        // We attempt to discover Character objects via the world registry,
        // then fetch their JSON contents to extract metadata.name.
        let pending_names: Vec<u64> = {
            let s = state.read().await;
            s.live
                .profiles
                .values()
                .filter(|p| p.name.is_none() && !failed_names.contains(&p.character_item_id))
                .map(|p| p.character_item_id)
                .take(50)
                .collect()
        };

        if !pending_names.is_empty() && !world_package_id.is_empty() {
            // Try to resolve names via gRPC by looking up known character object IDs
            // from the object_id_cache (populated during checkpoint replay)
            let object_ids: Vec<(u64, String)> = {
                let s = state.read().await;
                pending_names
                    .iter()
                    .filter_map(|id| s.live.object_id_cache.get(id).map(|oid| (*id, oid.clone())))
                    .collect()
            };

            if !object_ids.is_empty() {
                if let Ok(channel) = crate::sui_client::connect(&grpc_url).await {
                    let oids: Vec<String> = object_ids.iter().map(|(_, oid)| oid.clone()).collect();
                    match crate::sui_client::batch_get_objects_json(channel, &oids).await {
                        Ok(results) => {
                            let mut resolved = 0;
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
                                    let _ = crate::db::upsert_character_name(&pool, id, name).await;
                                    resolved += 1;
                                }
                            }
                            if resolved > 0 {
                                tracing::info!(
                                    "Metadata resolver: resolved {resolved} character names via gRPC"
                                );
                            }
                        }
                        Err(e) => {
                            tracing::debug!("gRPC batch name resolution failed: {e}");
                        }
                    }
                }
            }

            // Mark as failed only those we actually attempted (had an object ID for).
            // Characters without object IDs are skipped here — they'll get an object ID
            // once seen in the live stream and can be retried then.
            let attempted: std::collections::HashSet<u64> =
                object_ids.iter().map(|(id, _)| *id).collect();
            {
                let s = state.read().await;
                for id in &pending_names {
                    if attempted.contains(id)
                        && !s.live.name_cache.get(id).is_some_and(|n| !n.is_empty())
                    {
                        failed_names.insert(*id);
                    }
                }
            }
        }

        if !pending_systems.is_empty() || !pending_tribes.is_empty() {
            tracing::debug!(
                "Metadata resolver: {} systems, {} tribes pending",
                pending_systems.len(),
                pending_tribes.len(),
            );
        }
        // Names without a cached object ID can't be resolved yet — log at trace only
        // to avoid noise. They'll be resolved when the live stream provides an object ID.
        if !pending_names.is_empty() {
            let with_oid = {
                let s = state.read().await;
                pending_names
                    .iter()
                    .filter(|id| s.live.object_id_cache.contains_key(*id))
                    .count()
            };
            if with_oid > 0 {
                tracing::debug!("{with_oid} names pending (have object IDs, retrying)");
            } else {
                tracing::trace!(
                    "{} names pending (no object IDs yet, waiting for live stream)",
                    pending_names.len()
                );
            }
        }
    }
}
