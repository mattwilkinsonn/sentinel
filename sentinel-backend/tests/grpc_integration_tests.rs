//! Integration tests against the real Sui testnet gRPC endpoint.
//! These tests make actual network calls and may be slow or flaky.
//!
//! Run with: cargo test --test grpc_integration_tests -- --ignored
//! (they are #[ignore] by default so they don't run in CI)

use sentinel_backend::grpc::sui_rpc;
use sentinel_backend::sui_client;

const TESTNET_GRPC: &str = "https://fullnode.testnet.sui.io:443";

/// Helper to connect to the real testnet.
async fn testnet_channel() -> tonic::transport::Channel {
    sui_client::connect(TESTNET_GRPC).await.unwrap()
}

#[tokio::test]
#[ignore]
async fn real_get_service_info() {
    let channel = testnet_channel().await;
    let height = sui_client::get_latest_checkpoint(channel).await.unwrap();

    // Testnet should have at least some checkpoints
    assert!(
        height > 1000,
        "expected checkpoint height > 1000, got {height}"
    );
    println!("Testnet latest checkpoint: {height}");
}

#[tokio::test]
#[ignore]
async fn real_get_checkpoint() {
    let channel = testnet_channel().await;

    // Get latest height, then fetch a recent checkpoint (old ones get pruned)
    let height = sui_client::get_latest_checkpoint(channel.clone())
        .await
        .unwrap();
    let target = height.saturating_sub(10);

    let mut client = sui_rpc::ledger_service_client::LedgerServiceClient::new(channel);
    let resp = sui_client::get_checkpoint(&mut client, target)
        .await
        .unwrap();

    let cp = resp.checkpoint.expect("checkpoint should be present");
    // read_mask only requests summary + transactions, so sequence_number may be None
    println!(
        "Checkpoint {target}: {} transactions, summary: {:?}",
        cp.transactions.len(),
        cp.summary
    );
}

#[tokio::test]
#[ignore]
async fn real_get_object_clock() {
    let channel = testnet_channel().await;

    // The Clock object (0x6) exists on every Sui network
    let clock_id = "0x0000000000000000000000000000000000000000000000000000000000000006";
    let json = sui_client::get_object_json(channel, clock_id)
        .await
        .unwrap();

    // Clock object should have a timestamp_ms field
    println!("Clock object JSON: {json}");
    assert!(
        json.get("timestamp_ms").is_some() || !json.is_null(),
        "Clock object should have data"
    );
}

#[tokio::test]
#[ignore]
async fn real_get_object_missing() {
    let channel = testnet_channel().await;

    // A made-up object ID that shouldn't exist
    let fake_id = "0xdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeefdeadbeef";
    let result = sui_client::get_object_json(channel, fake_id).await;

    // Should either return null JSON or error
    match result {
        Ok(json) => assert!(json.is_null(), "fake object should be null"),
        Err(_) => {} // error is also acceptable
    }
}

#[tokio::test]
#[ignore]
async fn real_list_dynamic_fields_on_system_object() {
    let channel = testnet_channel().await;

    // The Sui system state object (0x5) has dynamic fields
    let system_id = "0x0000000000000000000000000000000000000000000000000000000000000005";
    let fields = sui_client::list_dynamic_fields(channel, system_id, 5)
        .await
        .unwrap();

    println!("System object has {} dynamic fields (page)", fields.len());
    // System state should have at least some dynamic fields (validator set, etc.)
    assert!(
        !fields.is_empty(),
        "system state object should have dynamic fields"
    );
}

#[tokio::test]
#[ignore]
async fn real_checkpoint_replay_processes_recent_events() {
    let channel = testnet_channel().await;
    let latest = sui_client::get_latest_checkpoint(channel.clone())
        .await
        .unwrap();

    // Fetch the 5 most recent checkpoints and count transactions
    let mut client = sui_rpc::ledger_service_client::LedgerServiceClient::new(channel);
    let mut total_txs = 0u64;

    let start = latest.saturating_sub(4);
    for seq in start..=latest {
        let resp = sui_client::get_checkpoint(&mut client, seq).await.unwrap();
        if let Some(cp) = resp.checkpoint {
            let tx_count = cp.transactions.len();
            total_txs += tx_count as u64;
            println!("Checkpoint {seq}: {tx_count} transactions");
        }
    }

    println!("Total transactions in 5 checkpoints: {total_txs}");
    // Recent checkpoints should have at least some transactions
    assert!(total_txs > 0, "recent checkpoints should have transactions");
}
