//! On-chain publisher — sends batch_update transactions to ThreatRegistry via gRPC.

use std::sync::Arc;
use tokio::sync::RwLock;
use tonic::transport::Channel;

use crate::config::AppConfig;
use crate::grpc::sui_rpc;
use crate::types::AppState;

use sui_rpc::transaction_execution_service_client::TransactionExecutionServiceClient;

/// Periodically publish dirty threat profiles to the on-chain ThreatRegistry.
pub async fn publish_loop(config: AppConfig, state: Arc<RwLock<AppState>>) {
    let interval = std::time::Duration::from_millis(config.publish_interval_ms);

    if config.sentinel_package_id.is_empty() || config.threat_registry_id.is_empty() {
        tracing::warn!("SENTINEL_PACKAGE_ID or THREAT_REGISTRY_ID not set — publisher disabled");
        return;
    }

    if config.admin_private_key.is_empty() {
        tracing::warn!("ADMIN_PRIVATE_KEY not set — publisher disabled");
        return;
    }

    tracing::info!(
        "Publisher started — batch_update every {}ms",
        config.publish_interval_ms
    );

    loop {
        tokio::time::sleep(interval).await;

        let dirty_profiles = {
            let mut state = state.write().await;
            let dirty: Vec<_> = state
                .live
                .profiles
                .values()
                .filter(|p| p.dirty)
                .cloned()
                .collect();

            // Mark clean
            for p in state.live.profiles.values_mut() {
                p.dirty = false;
            }

            dirty
        };

        if dirty_profiles.is_empty() {
            continue;
        }

        // Batch in groups of 50 (contract limit)
        for chunk in dirty_profiles.chunks(50) {
            match publish_batch_grpc(&config, chunk).await {
                Ok(digest) => {
                    tracing::info!("Published {} threat scores — tx: {digest}", chunk.len());
                }
                Err(e) => {
                    tracing::error!("Failed to publish batch: {e}");
                    // Re-mark as dirty on failure
                    let mut state = state.write().await;
                    for p in chunk {
                        if let Some(profile) = state.live.profiles.get_mut(&p.character_item_id) {
                            profile.dirty = true;
                        }
                    }
                }
            }
        }
    }
}

/// Build and submit a batch_update transaction via gRPC ExecuteTransaction.
///
/// The full flow:
/// 1. Build a ProgrammableTransaction with a MoveCall to batch_update
/// 2. Sign with the admin key
/// 3. Submit via TransactionExecutionService.ExecuteTransaction
async fn publish_batch_grpc(
    config: &AppConfig,
    profiles: &[crate::types::ThreatProfile],
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let channel = Channel::from_shared(config.sui_grpc_url.clone())?
        .tls_config(tonic::transport::ClientTlsConfig::new().with_webpki_roots())?
        .connect()
        .await?;

    let mut _client = TransactionExecutionServiceClient::new(channel);

    // TODO: Build ProgrammableTransaction using sui-transaction-builder
    // For now, log what we would publish
    let char_ids: Vec<u64> = profiles.iter().map(|p| p.character_item_id).collect();
    let scores: Vec<u64> = profiles.iter().map(|p| p.threat_score).collect();

    tracing::info!(
        "Would publish batch_update for {} profiles: ids={:?} scores={:?}",
        profiles.len(),
        &char_ids[..char_ids.len().min(5)],
        &scores[..scores.len().min(5)],
    );

    // Placeholder — transaction building requires resolving object references
    // via LedgerService.GetObject first. Full implementation comes after deploy.
    Ok("pending-implementation".to_string())
}
