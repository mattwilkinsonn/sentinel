//! Character name resolution via gRPC LedgerService.
//!
//! Fetches Character objects from Sui and extracts the display name
//! from their metadata. Results are cached permanently in AppState.

use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::transport::Channel;

use crate::config::AppConfig;
use crate::grpc::sui_rpc;
use crate::types::AppState;

use sui_rpc::ledger_service_client::LedgerServiceClient;

/// Resolve names for any character_item_ids that aren't cached yet.
/// Called periodically or after new characters are discovered.
pub async fn resolve_pending_names(config: &AppConfig, state: &Arc<RwLock<AppState>>) {
    // Collect IDs that need resolution
    let pending: Vec<u64> = {
        let s = state.read().await;
        s.live.profiles
            .keys()
            .filter(|id| !s.live.name_cache.contains_key(id))
            .copied()
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

    let mut client = LedgerServiceClient::new(channel);

    for character_item_id in pending {
        match resolve_one(&mut client, character_item_id).await {
            Ok(name) => {
                let mut s = state.write().await;
                s.live.name_cache.insert(character_item_id, name.clone());
                if let Some(profile) = s.live.profiles.get_mut(&character_item_id) {
                    profile.name = name;
                }
            }
            Err(e) => {
                tracing::debug!(
                    "Could not resolve name for character {character_item_id}: {e}"
                );
                // Cache a fallback so we don't retry every cycle
                let mut s = state.write().await;
                let fallback = format!("Pilot #{character_item_id}");
                s.live.name_cache.insert(character_item_id, fallback.clone());
                if let Some(profile) = s.live.profiles.get_mut(&character_item_id) {
                    profile.name = fallback;
                }
            }
        }
    }
}


async fn connect_grpc(
    config: &AppConfig,
) -> Result<Channel, Box<dyn std::error::Error + Send + Sync>> {
    let channel = Channel::from_shared(config.sui_grpc_url.clone())?
        .tls_config(tonic::transport::ClientTlsConfig::new().with_webpki_roots())?
        .connect()
        .await?;
    Ok(channel)
}

async fn resolve_one(
    _client: &mut LedgerServiceClient<Channel>,
    character_item_id: u64,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    // We need the Sui object ID for this character, not the item_id.
    // The item_id is a game-level identifier stored inside the Character object.
    //
    // For now, we can't directly look up by item_id via gRPC — we'd need
    // to search dynamic fields or use an indexer. As a pragmatic solution,
    // if events include the Sui object ID for characters, we'd use that.
    //
    // For the hackathon demo, we'll use item_id as the display and resolve
    // names when we have the actual Sui object IDs from event data.
    //
    // TODO: When processing events, extract the Character Sui object ID
    // and store it alongside item_id, then use GetObject here.

    Err(format!("Object ID not available for item_id {character_item_id}").into())
}

/// Spawn a background task that periodically resolves pending names.
pub async fn name_resolver_loop(config: AppConfig, state: Arc<RwLock<AppState>>) {
    let interval = std::time::Duration::from_secs(15);

    loop {
        tokio::time::sleep(interval).await;
        resolve_pending_names(&config, &state).await;
    }
}
