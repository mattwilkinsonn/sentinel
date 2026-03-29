//! Character name resolution via gRPC LedgerService.
//!
//! Fetches Character objects from Sui and extracts the display name
//! from their metadata. Results are cached permanently in AppState.

use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::transport::Channel;

use crate::config::AppConfig;
use crate::types::AppState;

/// Resolve names for any character_item_ids that aren't cached yet.
/// Called periodically or after new characters are discovered.
pub async fn resolve_pending_names(config: &AppConfig, state: &Arc<RwLock<AppState>>) {
    // Collect IDs that need resolution and have a known Sui object address
    let pending: Vec<(u64, String)> = {
        let s = state.read().await;
        s.live
            .profiles
            .values()
            .filter(|p| p.name.is_none())
            .filter_map(|p| {
                s.live
                    .object_id_cache
                    .get(&p.character_item_id)
                    .map(|oid| (p.character_item_id, oid.clone()))
            })
            .collect()
    };

    if pending.is_empty() {
        return;
    }

    tracing::info!("Resolving names for {} characters", pending.len());

    let channel = match connect_grpc(config).await {
        Ok(ch) => ch,
        Err(e) => {
            tracing::warn!("Failed to connect for name resolution: {e}");
            return;
        }
    };

    let oids: Vec<String> = pending.iter().map(|(_, oid)| oid.clone()).collect();
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
                }
            }
        }
        Err(e) => {
            tracing::warn!("gRPC batch name resolution failed: {e}");
        }
    }
}

/// Open a TLS gRPC channel to the Sui fullnode for name resolution.
async fn connect_grpc(
    config: &AppConfig,
) -> Result<Channel, Box<dyn std::error::Error + Send + Sync>> {
    let channel = Channel::from_shared(config.sui_grpc_url.clone())?
        .tls_config(tonic::transport::ClientTlsConfig::new().with_webpki_roots())?
        .connect()
        .await?;
    Ok(channel)
}

/// Spawn a background task that periodically resolves pending names.
pub async fn name_resolver_loop(config: AppConfig, state: Arc<RwLock<AppState>>) {
    let interval = std::time::Duration::from_secs(15);

    loop {
        tokio::time::sleep(interval).await;
        resolve_pending_names(&config, &state).await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{DataStore, ThreatProfile};

    fn state_with(store: DataStore) -> Arc<RwLock<AppState>> {
        let mut app = AppState::default();
        app.live = store;
        Arc::new(RwLock::new(app))
    }

    #[tokio::test]
    async fn pending_empty_when_all_names_resolved() {
        let mut store = DataStore::default();
        store.profiles.insert(
            1,
            ThreatProfile {
                character_item_id: 1,
                name: Some("Alice".to_string()),
                ..Default::default()
            },
        );
        store.object_id_cache.insert(1, "0xaaa".to_string());

        let state = state_with(store);
        let s = state.read().await;
        let pending: Vec<(u64, String)> = s
            .live
            .profiles
            .values()
            .filter(|p| p.name.is_none())
            .filter_map(|p| {
                s.live
                    .object_id_cache
                    .get(&p.character_item_id)
                    .map(|oid| (p.character_item_id, oid.clone()))
            })
            .collect();

        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn pending_includes_only_unresolved_with_object_ids() {
        let mut store = DataStore::default();
        // Unresolved with object ID — should be pending
        store.profiles.insert(
            1,
            ThreatProfile {
                character_item_id: 1,
                name: None,
                ..Default::default()
            },
        );
        store.object_id_cache.insert(1, "0xaaa".to_string());

        // Unresolved without object ID — should NOT be pending
        store.profiles.insert(
            2,
            ThreatProfile {
                character_item_id: 2,
                name: None,
                ..Default::default()
            },
        );

        // Resolved with object ID — should NOT be pending
        store.profiles.insert(
            3,
            ThreatProfile {
                character_item_id: 3,
                name: Some("Bob".to_string()),
                ..Default::default()
            },
        );
        store.object_id_cache.insert(3, "0xccc".to_string());

        let state = state_with(store);
        let s = state.read().await;
        let pending: Vec<(u64, String)> = s
            .live
            .profiles
            .values()
            .filter(|p| p.name.is_none())
            .filter_map(|p| {
                s.live
                    .object_id_cache
                    .get(&p.character_item_id)
                    .map(|oid| (p.character_item_id, oid.clone()))
            })
            .collect();

        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].0, 1);
        assert_eq!(pending[0].1, "0xaaa");
    }

    #[tokio::test]
    async fn pending_empty_when_no_profiles() {
        let store = DataStore::default();
        let state = state_with(store);
        let s = state.read().await;
        let pending: Vec<(u64, String)> = s
            .live
            .profiles
            .values()
            .filter(|p| p.name.is_none())
            .filter_map(|p| {
                s.live
                    .object_id_cache
                    .get(&p.character_item_id)
                    .map(|oid| (p.character_item_id, oid.clone()))
            })
            .collect();

        assert!(pending.is_empty());
    }

    #[tokio::test]
    async fn multiple_unresolved_with_object_ids_all_collected() {
        let mut store = DataStore::default();
        for i in 1..=5 {
            store.profiles.insert(
                i,
                ThreatProfile {
                    character_item_id: i,
                    name: None,
                    ..Default::default()
                },
            );
            store.object_id_cache.insert(i, format!("0x{:03x}", i));
        }

        let state = state_with(store);
        let s = state.read().await;
        let pending: Vec<(u64, String)> = s
            .live
            .profiles
            .values()
            .filter(|p| p.name.is_none())
            .filter_map(|p| {
                s.live
                    .object_id_cache
                    .get(&p.character_item_id)
                    .map(|oid| (p.character_item_id, oid.clone()))
            })
            .collect();

        assert_eq!(pending.len(), 5);
    }
}
