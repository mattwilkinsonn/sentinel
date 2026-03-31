//! Shared Sui gRPC client utilities.
//! Provides connection helpers and reusable query functions.

use tonic::transport::Channel;

use crate::grpc::sui_rpc;
use sui_rpc::ledger_service_client::LedgerServiceClient;
use sui_rpc::state_service_client::StateServiceClient;

/// Connect a gRPC channel to the Sui fullnode.
pub async fn connect(url: &str) -> Result<Channel, Box<dyn std::error::Error + Send + Sync>> {
    let channel = Channel::from_shared(url.to_string())?
        .tls_config(tonic::transport::ClientTlsConfig::new().with_webpki_roots())?
        .connect()
        .await?;
    Ok(channel)
}

/// Get the latest checkpoint height from the Sui fullnode.
pub async fn get_latest_checkpoint(
    channel: Channel,
) -> Result<u64, Box<dyn std::error::Error + Send + Sync>> {
    let mut client = LedgerServiceClient::new(channel);
    let resp = client
        .get_service_info(sui_rpc::GetServiceInfoRequest {})
        .await?
        .into_inner();
    resp.checkpoint_height
        .ok_or_else(|| "GetServiceInfo missing checkpoint_height".into())
}

/// Fetch a single checkpoint by sequence number.
pub async fn get_checkpoint(
    client: &mut LedgerServiceClient<Channel>,
    seq: u64,
) -> Result<sui_rpc::GetCheckpointResponse, Box<dyn std::error::Error + Send + Sync>> {
    let resp = client
        .get_checkpoint(sui_rpc::GetCheckpointRequest {
            checkpoint_id: Some(sui_rpc::get_checkpoint_request::CheckpointId::SequenceNumber(seq)),
            read_mask: Some(prost_types::FieldMask {
                paths: vec!["summary".into(), "transactions".into()],
            }),
        })
        .await?
        .into_inner();
    Ok(resp)
}

/// Fetch an object by ID with JSON rendering.
pub async fn get_object_json(
    channel: Channel,
    object_id: &str,
) -> Result<serde_json::Value, Box<dyn std::error::Error + Send + Sync>> {
    let mut client = LedgerServiceClient::new(channel);
    let resp = client
        .get_object(sui_rpc::GetObjectRequest {
            object_id: Some(object_id.to_string()),
            version: None,
            read_mask: Some(prost_types::FieldMask {
                paths: vec!["json".into(), "object_type".into()],
            }),
        })
        .await?
        .into_inner();

    let obj = resp.object.ok_or("GetObject returned no object")?;
    let json = obj
        .json
        .map(|v| crate::grpc::proto_value_to_json(&v))
        .unwrap_or(serde_json::Value::Null);
    Ok(json)
}

/// Batch-fetch objects by ID with JSON rendering.
pub async fn batch_get_objects_json(
    channel: Channel,
    object_ids: &[String],
) -> Result<Vec<(String, serde_json::Value)>, Box<dyn std::error::Error + Send + Sync>> {
    if object_ids.is_empty() {
        return Ok(Vec::new());
    }

    let mut client = LedgerServiceClient::new(channel);
    let requests: Vec<sui_rpc::GetObjectRequest> = object_ids
        .iter()
        .map(|id| sui_rpc::GetObjectRequest {
            object_id: Some(id.clone()),
            version: None,
            read_mask: None, // batch uses top-level read_mask
        })
        .collect();

    let resp = client
        .batch_get_objects(sui_rpc::BatchGetObjectsRequest {
            requests,
            read_mask: Some(prost_types::FieldMask {
                paths: vec!["json".into(), "object_id".into(), "object_type".into()],
            }),
        })
        .await?
        .into_inner();

    let mut results = Vec::new();
    for result in resp.objects {
        if let Some(sui_rpc::get_object_result::Result::Object(obj)) = result.result {
            let id = obj.object_id.clone().unwrap_or_default();
            let json = obj
                .json
                .map(|v| crate::grpc::proto_value_to_json(&v))
                .unwrap_or(serde_json::Value::Null);
            results.push((id, json));
        }
    }
    Ok(results)
}

/// List dynamic fields with BCS name/value data.
pub async fn list_dynamic_fields(
    channel: Channel,
    parent_id: &str,
    page_size: u32,
) -> Result<Vec<sui_rpc::DynamicField>, Box<dyn std::error::Error + Send + Sync>> {
    let mut client = StateServiceClient::new(channel);
    let mut all_fields = Vec::new();
    let mut page_token: Option<Vec<u8>> = None;

    loop {
        let resp = client
            .list_dynamic_fields(sui_rpc::ListDynamicFieldsRequest {
                parent: Some(parent_id.to_string()),
                page_size: Some(page_size),
                page_token: page_token.clone(),
                read_mask: Some(prost_types::FieldMask {
                    paths: vec!["name".into(), "value".into()],
                }),
            })
            .await?
            .into_inner();

        all_fields.extend(resp.dynamic_fields);

        match resp.next_page_token {
            Some(token) if !token.is_empty() => page_token = Some(token),
            _ => break,
        }
    }

    Ok(all_fields)
}
