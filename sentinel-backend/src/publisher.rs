//! On-chain publisher — sends batch_update transactions to ThreatRegistry.
//! All Sui communication uses gRPC (no JSON-RPC or GraphQL).

use std::sync::Arc;
use tokio::sync::RwLock;

use sui_crypto::SuiSigner;
use sui_crypto::ed25519::Ed25519PrivateKey;
use sui_sdk_types::{Address, Digest, Identifier};
use sui_transaction_builder::{Function, ObjectInput, TransactionBuilder};
use tonic::transport::Channel;

use crate::config::AppConfig;
use crate::grpc::sui_rpc;
use crate::types::AppState;

use sui_rpc::ledger_service_client::LedgerServiceClient;
use sui_rpc::state_service_client::StateServiceClient;
use sui_rpc::transaction_execution_service_client::TransactionExecutionServiceClient;

const CLOCK_ID: &str = "0x0000000000000000000000000000000000000000000000000000000000000006";

/// BCS-deserializable key for ThreatRegistry dynamic fields.
#[derive(serde::Deserialize)]
struct ThreatEntryKey {
    character_item_id: u64,
}

/// BCS-deserializable value for ThreatRegistry dynamic fields.
/// All fields from ThreatEntry must be present: bcs::from_bytes fails on trailing
/// bytes, so a partial struct would cause silent deserialization failure every cycle,
/// leaving onchain_scores empty and republishing all profiles on every tick.
#[derive(serde::Deserialize)]
struct ThreatEntryValue {
    #[allow(dead_code)]
    character_item_id: u64,
    threat_score: u64,
    #[allow(dead_code)]
    kill_count: u64,
    #[allow(dead_code)]
    death_count: u64,
    #[allow(dead_code)]
    bounty_count: u64,
    #[allow(dead_code)]
    last_kill_timestamp: u64,
    #[allow(dead_code)]
    last_seen_system: String,
    #[allow(dead_code)]
    updated_at: u64,
}

/// Background loop that periodically publishes threat score changes to the ThreatRegistry on-chain.
///
/// On startup, fetches the current on-chain scores once to seed `published_scores`.
/// Each subsequent cycle compares in-memory profiles against that local cache — no
/// per-cycle chain reads. The cache is updated after every successful publish batch.
///
/// Exits early if required config is missing. Uses exponential backoff on consecutive failures.
pub async fn publish_loop(
    config: AppConfig,
    state: Arc<RwLock<AppState>>,
    sse_tx: Option<tokio::sync::broadcast::Sender<String>>,
) {
    let interval = std::time::Duration::from_millis(config.publish_interval_ms);

    if config.sentinel_package_id.is_empty() || config.threat_registry_id.is_empty() {
        tracing::warn!("SENTINEL_PACKAGE_ID or THREAT_REGISTRY_ID not set — publisher disabled");
        return;
    }

    if config.publisher_private_key.is_empty() {
        tracing::warn!("SUI_PUBLISHER_KEY not set — publisher disabled");
        return;
    }

    let admin_cap_id = std::env::var("SENTINEL_ADMIN_CAP_ID").unwrap_or_default();
    if admin_cap_id.is_empty() {
        tracing::warn!("SENTINEL_ADMIN_CAP_ID not set — publisher disabled");
        return;
    }

    let (admin_key, sender) = match parse_sui_private_key(&config.publisher_private_key) {
        Ok(v) => v,
        Err(e) => {
            tracing::error!(error = %e, "Failed to parse SUI_PUBLISHER_KEY: {e}");
            return;
        }
    };

    // Fetch current on-chain scores once at startup to seed the local cache.
    // After this, we track published scores in-memory and only re-fetch on failure.
    let mut published_scores: std::collections::HashMap<u64, u64> = {
        match connect_grpc(&config.sui_grpc_url).await {
            Ok(ch) => match fetch_onchain_scores(ch, &config.threat_registry_id).await {
                Ok(scores) => {
                    tracing::info!(
                        sender = %sender,
                        onchain_scores = scores.len(),
                        batch_interval_ms = config.publish_interval_ms,
                        "Publisher started — sender: {sender}, {} scores on-chain, \
                             batch_update every {}ms",
                        scores.len(),
                        config.publish_interval_ms
                    );
                    scores
                }
                Err(e) => {
                    tracing::warn!(
                        error = %e,
                        "Publisher: initial on-chain fetch failed ({e}), starting empty"
                    );
                    std::collections::HashMap::new()
                }
            },
            Err(e) => {
                tracing::warn!(error = %e, "Publisher: initial gRPC connect failed ({e}), starting empty");
                std::collections::HashMap::new()
            }
        }
    };

    let mut consecutive_failures: u32 = 0;

    loop {
        // Back off on repeated failures (30s, 60s, 120s, max 5min)
        let wait = if consecutive_failures > 0 {
            let backoff = interval * 2u32.pow(consecutive_failures.min(4));
            tracing::info!(
                backoff_secs = backoff.as_secs(),
                consecutive_failures,
                "Publisher backing off for {}s after {} failures",
                backoff.as_secs(),
                consecutive_failures
            );
            backoff
        } else {
            interval
        };
        tokio::time::sleep(wait).await;

        // Connect gRPC channel (reconnects each cycle for resilience)
        let channel = match connect_grpc(&config.sui_grpc_url).await {
            Ok(ch) => ch,
            Err(e) => {
                tracing::warn!(error = %e, "Publisher: gRPC connect failed: {e}");
                consecutive_failures += 1;
                continue;
            }
        };

        // On repeated failures, re-sync from chain to avoid diverging from reality
        if consecutive_failures > 0 {
            match fetch_onchain_scores(channel.clone(), &config.threat_registry_id).await {
                Ok(scores) => {
                    tracing::info!(
                        onchain_scores = scores.len(),
                        "Publisher: re-synced {} on-chain scores after failures",
                        scores.len()
                    );
                    published_scores = scores;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Publisher: re-sync fetch failed: {e}");
                    consecutive_failures += 1;
                    continue;
                }
            }
        }

        // Compare in-memory profiles against local published_scores cache
        let publishable: Vec<_> = {
            let state = state.read().await;
            state
                .live
                .profiles
                .values()
                .filter(|p| {
                    if p.threat_score == 0 {
                        return false;
                    }
                    // New profile not on chain yet
                    if !published_scores.contains_key(&p.character_item_id) {
                        return true;
                    }
                    // Score changed by more than threshold
                    let last = published_scores[&p.character_item_id];
                    p.threat_score.abs_diff(last) > config.publish_score_threshold_bp
                })
                .cloned()
                .collect()
        };

        if publishable.is_empty() {
            tracing::debug!("Publisher: no score changes to publish");
            continue;
        }

        tracing::info!(
            profiles_pending = publishable.len(),
            "Publisher: {} profiles with score changes",
            publishable.len()
        );

        let mut batch_ok = true;
        let mut cap_override: Option<(u64, Digest)> = None;
        for chunk in publishable.chunks(20) {
            match publish_batch(
                &config,
                channel.clone(),
                &admin_key,
                sender,
                &admin_cap_id,
                cap_override,
                chunk,
            )
            .await
            {
                Ok((digest, new_cap)) => {
                    tracing::info!(scores_published = chunk.len(), tx_digest = %digest, "Published {} threat scores — tx: {digest}", chunk.len());
                    // Update local cache and emit feed events for published profiles
                    {
                        let now_ms = std::time::SystemTime::now()
                            .duration_since(std::time::UNIX_EPOCH)
                            .unwrap_or_default()
                            .as_millis() as u64;
                        let mut s = state.write().await;
                        let cap = config.max_recent_events;
                        for p in chunk {
                            let old_score = published_scores
                                .get(&p.character_item_id)
                                .copied()
                                .unwrap_or(0);
                            let delta = p.threat_score as i64 - old_score as i64;
                            published_scores.insert(p.character_item_id, p.threat_score);
                            s.live.push_event(
                                crate::types::RawEvent {
                                    event_type: "score_change".into(),
                                    timestamp_ms: now_ms,
                                    data: serde_json::json!({
                                        "character_id": p.character_item_id,
                                        "new_score": p.threat_score,
                                        "delta": delta,
                                    }),
                                },
                                &sse_tx,
                                cap,
                            );
                        }
                    }
                    cap_override = new_cap;
                }
                Err(e) => {
                    tracing::warn!(error = %e, "Failed to publish batch: {e}");
                    batch_ok = false;
                    break;
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

/// Connect a TLS gRPC channel to the Sui fullnode URL.
async fn connect_grpc(url: &str) -> Result<Channel, Box<dyn std::error::Error + Send + Sync>> {
    let channel = Channel::from_shared(url.to_string())?
        .tls_config(tonic::transport::ClientTlsConfig::new().with_webpki_roots())?
        .connect()
        .await?;
    Ok(channel)
}

/// Fetch all threat scores currently on-chain from the ThreatRegistry's dynamic fields via gRPC.
async fn fetch_onchain_scores(
    channel: Channel,
    registry_id: &str,
) -> Result<std::collections::HashMap<u64, u64>, Box<dyn std::error::Error + Send + Sync>> {
    let mut client = StateServiceClient::new(channel);
    let mut scores = std::collections::HashMap::new();
    let mut page_token: Option<Vec<u8>> = None;

    loop {
        let request = sui_rpc::ListDynamicFieldsRequest {
            parent: Some(registry_id.to_string()),
            page_size: Some(200),
            page_token: page_token.clone(),
            read_mask: Some(prost_types::FieldMask {
                paths: vec!["name".into(), "value".into()],
            }),
        };

        let response = client.list_dynamic_fields(request).await?.into_inner();

        for field in &response.dynamic_fields {
            // Deserialize the BCS-encoded key and value
            let char_id = field
                .name
                .as_ref()
                .and_then(|bcs| bcs.value.as_ref())
                .and_then(|bytes| bcs::from_bytes::<ThreatEntryKey>(bytes).ok())
                .map(|k| k.character_item_id);

            let score = field
                .value
                .as_ref()
                .and_then(|bcs| bcs.value.as_ref())
                .and_then(|bytes| bcs::from_bytes::<ThreatEntryValue>(bytes).ok())
                .map(|v| v.threat_score);

            if let (Some(id), Some(s)) = (char_id, score) {
                scores.insert(id, s);
            }
        }

        match response.next_page_token {
            Some(token) if !token.is_empty() => page_token = Some(token),
            _ => break,
        }
    }

    Ok(scores)
}

/// Parse a `suiprivkey1...` bech32 key. Returns (signing_key, sui_address).
fn parse_sui_private_key(
    key_str: &str,
) -> Result<(Ed25519PrivateKey, Address), Box<dyn std::error::Error>> {
    let (_, data) = bech32::decode(key_str)?;

    if data.is_empty() {
        return Err("empty private key".into());
    }

    let scheme = data[0];
    if scheme != 0 {
        return Err(format!("unsupported key scheme: {scheme}, expected ed25519 (0)").into());
    }

    let secret_bytes: [u8; 32] = data[1..]
        .try_into()
        .map_err(|_| "private key must be 32 bytes")?;

    let key = Ed25519PrivateKey::new(secret_bytes);

    let pubkey = key.public_key();
    let mut hasher = blake2b_simd::Params::new().hash_length(32).to_state();
    hasher.update(&[0x00]);
    hasher.update(pubkey.inner());
    let hash = hasher.finalize();
    let address = Address::new(hash.as_bytes().try_into()?);

    Ok((key, address))
}

/// Resolve an object's version and digest via gRPC GetObject.
async fn resolve_object(
    channel: Channel,
    object_id: &str,
) -> Result<(u64, Digest), Box<dyn std::error::Error + Send + Sync>> {
    let mut client = LedgerServiceClient::new(channel);

    let response = client
        .get_object(sui_rpc::GetObjectRequest {
            object_id: Some(object_id.to_string()),
            version: None,
            read_mask: Some(prost_types::FieldMask {
                paths: vec!["version".into(), "digest".into()],
            }),
        })
        .await?
        .into_inner();

    let obj = response.object.ok_or("GetObject returned no object")?;
    let version = obj.version.ok_or("object missing version")?;
    let digest: Digest = obj
        .digest
        .as_deref()
        .ok_or("object missing digest")?
        .parse()
        .map_err(|e| format!("bad digest: {e}"))?;

    Ok((version, digest))
}

/// Resolve the initial_shared_version for a shared object via gRPC GetObject.
async fn resolve_shared_initial_version(
    channel: Channel,
    object_id: &str,
) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
    let mut client = LedgerServiceClient::new(channel);

    let response = client
        .get_object(sui_rpc::GetObjectRequest {
            object_id: Some(object_id.to_string()),
            version: None,
            read_mask: Some(prost_types::FieldMask {
                paths: vec!["owner".into()],
            }),
        })
        .await?
        .into_inner();

    let obj = response.object.ok_or("GetObject returned no object")?;
    let owner = obj.owner.ok_or("object missing owner")?;

    // Must be a shared object
    if owner.kind() != sui_rpc::owner::OwnerKind::Shared {
        return Err("not a shared object".into());
    }

    owner
        .version
        .ok_or_else(|| "shared object missing initial_shared_version".into())
}

/// Minimum gas balance (in MIST) before we warn. 1 SUI = 1_000_000_000 MIST.
/// ~0.5 SUI ≈ 1 day of runway at ~50 publishes/day × ~5M MIST gas each.
const GAS_BALANCE_WARN_THRESHOLD: u64 = 500_000_000;

/// Get a SUI gas coin for the given address via gRPC ListOwnedObjects.
/// Logs a warning if the balance is below `GAS_BALANCE_WARN_THRESHOLD`.
async fn get_gas_coin(
    channel: Channel,
    address: &str,
) -> Result<(Address, u64, Digest), Box<dyn std::error::Error + Send + Sync>> {
    let mut client = StateServiceClient::new(channel);

    let response = client
        .list_owned_objects(sui_rpc::ListOwnedObjectsRequest {
            owner: Some(address.to_string()),
            page_size: Some(1),
            page_token: None,
            read_mask: Some(prost_types::FieldMask {
                paths: vec![
                    "object_id".into(),
                    "version".into(),
                    "digest".into(),
                    "contents".into(),
                ],
            }),
            object_type: Some("0x2::coin::Coin<0x2::sui::SUI>".to_string()),
        })
        .await?
        .into_inner();

    let coin = response
        .objects
        .first()
        .ok_or("no gas coins — fund the publisher address with testnet SUI")?;

    // Check balance from the coin's BCS contents and warn if low
    if let Some(ref contents) = coin.contents {
        if let Some(ref bcs_data) = contents.value {
            // SUI Coin BCS layout: id (32 bytes) + balance (u64 LE)
            if bcs_data.len() >= 40 {
                let balance = u64::from_le_bytes(bcs_data[32..40].try_into().unwrap_or_default());
                if balance < GAS_BALANCE_WARN_THRESHOLD {
                    tracing::warn!(
                        balance_mist = balance,
                        threshold_mist = GAS_BALANCE_WARN_THRESHOLD,
                        address = %address,
                        "LOW GAS BALANCE: {} MIST ({:.4} SUI) — fund publisher wallet {}",
                        balance,
                        balance as f64 / 1_000_000_000.0,
                        address,
                    );
                }
            }
        }
    }

    let id: Address = coin
        .object_id
        .as_deref()
        .ok_or("coin missing object_id")?
        .parse()?;
    let version = coin.version.ok_or("coin missing version")?;
    let digest: Digest = coin
        .digest
        .as_deref()
        .ok_or("coin missing digest")?
        .parse()
        .map_err(|e| format!("bad digest: {e}"))?;

    Ok((id, version, digest))
}

/// Build and execute a `threat_registry::batch_update` PTB for up to 20 profiles.
///
/// Returns the transaction digest and the updated AdminCap reference (version + digest),
/// which can be passed as `cap_override` to the next batch to avoid re-resolving it via
/// gRPC between chunks. Simulates the transaction first so failures surface a useful error
/// message rather than a raw execution abort.
async fn publish_batch(
    config: &AppConfig,
    channel: Channel,
    admin_key: &Ed25519PrivateKey,
    sender: Address,
    admin_cap_id: &str,
    cap_override: Option<(u64, Digest)>,
    profiles: &[crate::types::ThreatProfile],
) -> Result<(String, Option<(u64, Digest)>), Box<dyn std::error::Error + Send + Sync>> {
    let package_id: Address = config.sentinel_package_id.parse()?;
    let registry_id: Address = config.threat_registry_id.parse()?;
    let cap_id: Address = admin_cap_id.parse()?;
    let clock_id: Address = CLOCK_ID.parse()?;

    // Resolve live object references via gRPC
    let reg_initial_version =
        resolve_shared_initial_version(channel.clone(), &config.threat_registry_id).await?;
    let (cap_version, cap_digest) = match cap_override {
        Some((v, d)) => (v, d),
        None => resolve_object(channel.clone(), admin_cap_id).await?,
    };
    let (gas_id, gas_version, gas_digest) =
        get_gas_coin(channel.clone(), &format!("{sender}")).await?;

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

    let registry_arg = tx.object(ObjectInput::shared(registry_id, reg_initial_version, true));
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
    tx.set_gas_budget(500_000_000);
    tx.set_gas_price(1000);
    tx.add_gas_objects([ObjectInput::owned(gas_id, gas_version, gas_digest)]);

    let transaction = tx.try_build()?;
    let signature = admin_key.sign_transaction(&transaction)?;

    // BCS-encode the transaction for gRPC
    let tx_bcs = bcs::to_bytes(&transaction)?;

    // Build the gRPC signature
    let grpc_sig = match &signature {
        sui_sdk_types::UserSignature::Simple(sui_sdk_types::SimpleSignature::Ed25519 {
            signature: sig,
            public_key: pk,
        }) => sui_rpc::UserSignature {
            bcs: None,
            scheme: Some(sui_rpc::SignatureScheme::Ed25519.into()),
            signature: Some(sui_rpc::user_signature::Signature::Simple(
                sui_rpc::SimpleSignature {
                    scheme: Some(sui_rpc::SignatureScheme::Ed25519.into()),
                    signature: Some(AsRef::<[u8]>::as_ref(sig).to_vec()),
                    public_key: Some(AsRef::<[u8]>::as_ref(pk).to_vec()),
                },
            )),
        },
        _ => return Err("Unsupported signature type".into()),
    };

    // Build the gRPC Transaction message with BCS
    let grpc_tx = sui_rpc::Transaction {
        bcs: Some(sui_rpc::Bcs {
            name: Some("TransactionData".into()),
            value: Some(tx_bcs.clone()),
        }),
        digest: None,
        version: None,
        kind: None,
        sender: None,
        gas_payment: None,
        expiration: None,
    };

    tracing::debug!(
        tx_bytes = tx_bcs.len(),
        sender = %sender,
        "Publishing tx via gRPC: {} bytes, sender: {sender}",
        tx_bcs.len(),
    );

    let mut tx_client = TransactionExecutionServiceClient::new(channel.clone());

    // Simulate first to get better error messages
    let sim_response = tx_client
        .simulate_transaction(sui_rpc::SimulateTransactionRequest {
            transaction: Some(grpc_tx.clone()),
            read_mask: Some(prost_types::FieldMask {
                paths: vec!["effects.status".into()],
            }),
            checks: None,
            do_gas_selection: None,
        })
        .await?
        .into_inner();

    if let Some(ref executed) = sim_response.transaction {
        if let Some(ref effects) = executed.effects {
            if let Some(ref status) = effects.status {
                if status.success != Some(true) {
                    let err_msg = status
                        .error
                        .as_ref()
                        .and_then(|e| e.description.as_deref())
                        .unwrap_or("unknown error");
                    return Err(format!("Simulation failed: {err_msg}").into());
                }
            }
        }
    }

    // Execute for real
    let exec_response = tx_client
        .execute_transaction(sui_rpc::ExecuteTransactionRequest {
            transaction: Some(grpc_tx),
            signatures: vec![grpc_sig],
            read_mask: Some(prost_types::FieldMask {
                paths: vec![
                    "digest".into(),
                    "effects.status".into(),
                    "effects.changed_objects".into(),
                ],
            }),
        })
        .await?
        .into_inner();

    let digest = exec_response
        .transaction
        .as_ref()
        .and_then(|t| t.digest.as_deref())
        .unwrap_or("unknown");

    // Check execution status and extract updated object versions
    let mut new_cap: Option<(u64, Digest)> = None;
    if let Some(ref executed) = exec_response.transaction {
        if let Some(ref effects) = executed.effects {
            if let Some(ref status) = effects.status {
                if status.success != Some(true) {
                    let err_msg = status
                        .error
                        .as_ref()
                        .and_then(|e| e.description.as_deref())
                        .unwrap_or("unknown error");
                    return Err(format!("Execute failed (tx={digest}): {err_msg}").into());
                }
            }

            let cap_id_normalized = admin_cap_id.trim_start_matches("0x").to_lowercase();
            for obj in &effects.changed_objects {
                if let Some(ref oid) = obj.object_id {
                    if oid.trim_start_matches("0x").to_lowercase() == cap_id_normalized {
                        if let (Some(v), Some(d)) =
                            (obj.output_version, obj.output_digest.as_deref())
                        {
                            if let Ok(parsed) = d.parse::<Digest>() {
                                new_cap = Some((v, parsed));
                                tracing::debug!(version = v, "AdminCap updated: v={v}");
                            }
                        }
                    }
                }
            }

            if new_cap.is_none() {
                tracing::warn!(
                    changed_objects = effects.changed_objects.len(),
                    "Could not extract AdminCap from effects ({} changed_objects), will re-resolve next batch",
                    effects.changed_objects.len()
                );
            }
        }
    }

    Ok((digest.to_string(), new_cap))
}
