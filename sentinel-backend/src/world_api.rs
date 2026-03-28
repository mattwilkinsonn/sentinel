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
pub async fn metadata_resolver_loop(
    world_client: std::sync::Arc<tokio::sync::RwLock<WorldApiClient>>,
    state: std::sync::Arc<tokio::sync::RwLock<crate::types::AppState>>,
    pool: PgPool,
) {
    loop {
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;

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

        // Resolve system names
        for system_id in &pending_systems {
            let name = {
                let mut client = world_client.write().await;
                client.fetch_system_name(system_id, &pool).await
            };
            if let Some(name) = name {
                if !name.is_empty() {
                    let mut s = state.write().await;
                    // Update the shared cache so gRPC handlers can use it inline
                    s.live
                        .system_name_cache
                        .insert(system_id.clone(), name.clone());
                    for profile in s.live.profiles.values_mut() {
                        if profile.last_seen_system == *system_id
                            && profile.last_seen_system_name.is_empty()
                        {
                            profile.last_seen_system_name = name.clone();
                            profile.dirty = true;
                        }
                    }
                }
            }
        }

        // Resolve tribe names
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

        if !pending_systems.is_empty() || !pending_tribes.is_empty() {
            tracing::info!(
                "Metadata resolver: {} systems, {} tribes pending",
                pending_systems.len(),
                pending_tribes.len()
            );
        }
    }
}
