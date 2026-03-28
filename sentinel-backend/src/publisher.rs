//! On-chain publisher — sends batch_update transactions to ThreatRegistry.

use std::sync::Arc;
use tokio::sync::RwLock;

use base64::Engine;
use sui_crypto::SuiSigner;
use sui_crypto::ed25519::Ed25519PrivateKey;
use sui_sdk_types::{Address, Digest, Identifier};
use sui_transaction_builder::{Function, ObjectInput, TransactionBuilder};

use crate::config::AppConfig;
use crate::types::AppState;

const CLOCK_ID: &str = "0x0000000000000000000000000000000000000000000000000000000000000006";

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

    let admin_cap_id = std::env::var("SENTINEL_ADMIN_CAP_ID").unwrap_or_default();
    if admin_cap_id.is_empty() {
        tracing::warn!("SENTINEL_ADMIN_CAP_ID not set — publisher disabled");
        return;
    }

    let (admin_key, sender) = match parse_sui_private_key(&config.admin_private_key) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!("Failed to parse ADMIN_PRIVATE_KEY: {e}");
            return;
        }
    };

    tracing::info!(
        "Publisher started — sender: {sender}, batch_update every {}ms",
        config.publish_interval_ms
    );

    let http = reqwest::Client::new();
    let rpc_url = "https://fullnode.testnet.sui.io:443";
    let mut consecutive_failures: u32 = 0;

    loop {
        // Back off on repeated failures (30s, 60s, 120s, max 5min)
        let wait = if consecutive_failures > 0 {
            let backoff = interval * 2u32.pow(consecutive_failures.min(4));
            tracing::info!(
                "Publisher backing off for {}s after {} failures",
                backoff.as_secs(),
                consecutive_failures
            );
            backoff
        } else {
            interval
        };
        tokio::time::sleep(wait).await;

        let dirty_profiles: Vec<_> = {
            let state = state.read().await;
            state
                .live
                .profiles
                .values()
                .filter(|p| p.dirty)
                .cloned()
                .collect()
        };

        if dirty_profiles.is_empty() {
            continue;
        }

        let mut batch_ok = true;
        for chunk in dirty_profiles.chunks(50) {
            match publish_batch(
                &config,
                &http,
                rpc_url,
                &admin_key,
                sender,
                &admin_cap_id,
                chunk,
            )
            .await
            {
                Ok(digest) => {
                    tracing::info!("Published {} threat scores — tx: {digest}", chunk.len());
                    let mut state = state.write().await;
                    for p in chunk {
                        if let Some(profile) = state.live.profiles.get_mut(&p.character_item_id) {
                            profile.dirty = false;
                        }
                    }
                }
                Err(e) => {
                    tracing::error!("Failed to publish batch: {e}");
                    batch_ok = false;
                    break; // Don't try more batches if one fails
                }
            }
        }

        if batch_ok {
            consecutive_failures = 0;
        } else {
            consecutive_failures += 1;
        }
    }
}

/// Parse a `suiprivkey1...` bech32 key. Returns (signing_key, sui_address).
fn parse_sui_private_key(
    key_str: &str,
) -> Result<(Ed25519PrivateKey, Address), Box<dyn std::error::Error>> {
    // Decode bech32: suiprivkey1... → [scheme_byte, 32_bytes_secret]
    let (_, data) = bech32::decode(key_str)?;

    if data.is_empty() {
        return Err("empty private key".into());
    }

    // First byte is scheme (0 = ed25519), rest is the 32-byte secret
    let scheme = data[0];
    if scheme != 0 {
        return Err(format!("unsupported key scheme: {scheme}, expected ed25519 (0)").into());
    }

    let secret_bytes: [u8; 32] = data[1..]
        .try_into()
        .map_err(|_| "private key must be 32 bytes")?;

    let key = Ed25519PrivateKey::new(secret_bytes);

    // Derive Sui address: blake2b_256(0x00 || pubkey_bytes)
    let pubkey = key.public_key();
    let mut hasher = blake2b_simd::Params::new().hash_length(32).to_state();
    hasher.update(&[0x00]); // ed25519 scheme flag
    hasher.update(pubkey.inner());
    let hash = hasher.finalize();
    let address = Address::new(hash.as_bytes().try_into()?);

    Ok((key, address))
}

async fn resolve_object(
    http: &reqwest::Client,
    rpc_url: &str,
    object_id: &str,
) -> Result<(u64, Digest), Box<dyn std::error::Error + Send + Sync>> {
    let resp: serde_json::Value = http
        .post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "sui_getObject",
            "params": [object_id, {"showContent": false}]
        }))
        .send()
        .await?
        .json()
        .await?;

    let data = &resp["result"]["data"];
    let version: u64 = data["version"]
        .as_str()
        .and_then(|v| v.parse().ok())
        .ok_or("missing version")?;
    let digest: Digest = data["digest"]
        .as_str()
        .ok_or("missing digest")?
        .parse()
        .map_err(|e| format!("bad digest: {e}"))?;

    Ok((version, digest))
}

async fn get_gas_coin(
    http: &reqwest::Client,
    rpc_url: &str,
    address: &str,
) -> Result<(Address, u64, Digest), Box<dyn std::error::Error + Send + Sync>> {
    let resp: serde_json::Value = http
        .post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "suix_getCoins",
            "params": [address, "0x2::sui::SUI", null, 1]
        }))
        .send()
        .await?
        .json()
        .await?;

    let coin = resp["result"]["data"]
        .as_array()
        .and_then(|a| a.first())
        .ok_or("no gas coins — fund the publisher address with testnet SUI")?;

    Ok((
        coin["coinObjectId"].as_str().ok_or("no id")?.parse()?,
        coin["version"]
            .as_str()
            .and_then(|v| v.parse().ok())
            .ok_or("no version")?,
        coin["digest"]
            .as_str()
            .ok_or("no digest")?
            .parse()
            .map_err(|e| format!("bad digest: {e}"))?,
    ))
}

async fn publish_batch(
    config: &AppConfig,
    http: &reqwest::Client,
    rpc_url: &str,
    admin_key: &Ed25519PrivateKey,
    sender: Address,
    admin_cap_id: &str,
    profiles: &[crate::types::ThreatProfile],
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
    let package_id: Address = config.sentinel_package_id.parse()?;
    let registry_id: Address = config.threat_registry_id.parse()?;
    let cap_id: Address = admin_cap_id.parse()?;
    let clock_id: Address = CLOCK_ID.parse()?;

    // Resolve live object references
    let (reg_version, _) = resolve_object(http, rpc_url, &config.threat_registry_id).await?;
    let (cap_version, cap_digest) = resolve_object(http, rpc_url, admin_cap_id).await?;
    let (gas_id, gas_version, gas_digest) =
        get_gas_coin(http, rpc_url, &format!("{sender}")).await?;

    // Build vectors
    let char_ids: Vec<u64> = profiles.iter().map(|p| p.character_item_id).collect();
    let scores: Vec<u64> = profiles.iter().map(|p| p.threat_score).collect();
    let kills: Vec<u64> = profiles.iter().map(|p| p.kill_count).collect();
    let deaths: Vec<u64> = profiles.iter().map(|p| p.death_count).collect();
    let bounties: Vec<u64> = profiles.iter().map(|p| p.bounty_count).collect();
    let timestamps: Vec<u64> = profiles.iter().map(|p| p.last_kill_timestamp).collect();
    let systems: Vec<String> = profiles
        .iter()
        .map(|p| p.last_seen_system.clone())
        .collect();

    // Build PTB
    let mut tx = TransactionBuilder::new();

    let registry_arg = tx.object(ObjectInput::shared(registry_id, reg_version, true));
    let cap_arg = tx.object(ObjectInput::owned(cap_id, cap_version, cap_digest));
    let char_ids_arg = tx.pure(&char_ids);
    let scores_arg = tx.pure(&scores);
    let kills_arg = tx.pure(&kills);
    let deaths_arg = tx.pure(&deaths);
    let bounties_arg = tx.pure(&bounties);
    let timestamps_arg = tx.pure(&timestamps);
    let systems_arg = tx.pure(&systems);
    let clock_arg = tx.object(ObjectInput::shared(clock_id, 1, false));

    let func = Function::new(
        package_id,
        Identifier::new("threat_registry").map_err(|e| format!("bad module: {e}"))?,
        Identifier::new("batch_update").map_err(|e| format!("bad function: {e}"))?,
    );

    tx.move_call(
        func,
        vec![
            registry_arg,
            cap_arg,
            char_ids_arg,
            scores_arg,
            kills_arg,
            deaths_arg,
            bounties_arg,
            timestamps_arg,
            systems_arg,
            clock_arg,
        ],
    );

    tx.set_sender(sender);
    tx.set_gas_budget(100_000_000);
    tx.set_gas_price(1000);
    tx.add_gas_objects([ObjectInput::owned(gas_id, gas_version, gas_digest)]);

    let transaction = tx.try_build()?;
    let signature = admin_key.sign_transaction(&transaction)?;

    // Encode and submit
    let b64 = base64::engine::general_purpose::STANDARD;
    let tx_b64 = b64.encode(bcs::to_bytes(&transaction)?);
    let sig_b64 = b64.encode(bcs::to_bytes(&signature)?);

    let resp: serde_json::Value = http
        .post(rpc_url)
        .json(&serde_json::json!({
            "jsonrpc": "2.0", "id": 1,
            "method": "sui_executeTransactionBlock",
            "params": [tx_b64, [sig_b64], null, "WaitForLocalExecution"]
        }))
        .send()
        .await?
        .json()
        .await?;

    if let Some(error) = resp.get("error") {
        return Err(format!("RPC error: {error}").into());
    }

    Ok(resp["result"]["digest"]
        .as_str()
        .unwrap_or("unknown")
        .to_string())
}
