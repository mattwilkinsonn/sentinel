//! Historical killmail loading from Sui GraphQL.
//! Fetches past killmails on startup to seed threat profiles.

use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::AppConfig;
use crate::threat_engine;
use crate::types::{AppState, RawEvent, ThreatProfile};

/// Extract a numeric item_id from nested {"item_id": "123", "tenant": "..."} or plain value.
fn extract_item_id(v: &serde_json::Value) -> Option<u64> {
    // Nested: {"item_id": "123", "tenant": "stillness"}
    v.get("item_id")
        .and_then(|id| {
            id.as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .or_else(|| id.as_u64())
        })
        // Fallback: plain value
        .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
        .or_else(|| v.as_u64())
}

/// Extract item_id as string (for solar_system_id).
fn extract_item_id_str(v: &serde_json::Value) -> String {
    // Nested: {"item_id": "30016543", "tenant": "stillness"}
    if let Some(id) = v.get("item_id") {
        return id.as_str().unwrap_or("").to_string();
    }
    // Plain string or number
    v.as_str().map(|s| s.to_string()).unwrap_or_default()
}

/// Load historical killmails from Sui GraphQL and seed live profiles.
pub async fn load_historical_killmails(
    config: &AppConfig,
    state: &Arc<RwLock<AppState>>,
) -> Result<usize, Box<dyn std::error::Error>> {
    if config.world_package_id.is_empty() {
        tracing::warn!("WORLD_PACKAGE_ID not set, skipping historical killmail load");
        return Ok(0);
    }

    let killmail_type = format!("{}::killmail::Killmail", config.world_package_id);
    tracing::info!("Loading historical killmails (type: {killmail_type})");

    let http = reqwest::Client::new();
    let mut cursor: Option<String> = None;
    let mut total = 0;

    loop {
        let after_clause = cursor
            .as_ref()
            .map(|c| format!(r#", after: "{}""#, c))
            .unwrap_or_default();

        let query = format!(
            r#"{{
                objects(filter: {{ type: "{killmail_type}" }}, first: 50{after_clause}) {{
                    nodes {{
                        asMoveObject {{
                            contents {{
                                json
                            }}
                        }}
                    }}
                    pageInfo {{
                        hasNextPage
                        endCursor
                    }}
                }}
            }}"#
        );

        let resp = http
            .post(&config.sui_graphql_url)
            .json(&serde_json::json!({ "query": query }))
            .send()
            .await?;

        let json: serde_json::Value = resp.json().await?;

        let nodes = json["data"]["objects"]["nodes"]
            .as_array()
            .cloned()
            .unwrap_or_default();

        if nodes.is_empty() {
            break;
        }

        let mut s = state.write().await;
        for node in &nodes {
            let contents = &node["asMoveObject"]["contents"]["json"];
            if contents.is_null() {
                continue;
            }

            // Killmail fields are nested: {"item_id": "123", "tenant": "stillness"}
            let killer_id = extract_item_id(&contents["killer_id"]);
            let victim_id = extract_item_id(&contents["victim_id"]);
            let timestamp_secs = contents["kill_timestamp"]
                .as_str()
                .and_then(|s| s.parse::<u64>().ok())
                .or_else(|| contents["kill_timestamp"].as_u64())
                .unwrap_or(0);
            // Convert seconds to milliseconds
            let timestamp = if timestamp_secs < 10_000_000_000 {
                timestamp_secs * 1000
            } else {
                timestamp_secs
            };
            let system = extract_item_id_str(&contents["solar_system_id"]);

            if let Some(kid) = killer_id {
                let profile = s.live.profiles.entry(kid).or_insert_with(|| ThreatProfile {
                    character_item_id: kid,
                    name: format!("Pilot #{kid}"),
                    ..Default::default()
                });
                profile.kill_count += 1;
                profile.last_kill_timestamp = profile.last_kill_timestamp.max(timestamp);
                if !system.is_empty() {
                    profile.last_seen_system = system.clone();
                }
                profile.threat_score = threat_engine::compute_score(profile);
            }

            if let Some(vid) = victim_id {
                let profile = s.live.profiles.entry(vid).or_insert_with(|| ThreatProfile {
                    character_item_id: vid,
                    name: format!("Pilot #{vid}"),
                    ..Default::default()
                });
                profile.death_count += 1;
                if !system.is_empty() {
                    profile.last_seen_system = system.clone();
                }
                profile.threat_score = threat_engine::compute_score(profile);
            }

            // Add as a historical event with flattened data
            s.live.push_event(
                RawEvent {
                    event_type: "kill".into(),
                    timestamp_ms: timestamp,
                    data: serde_json::json!({
                        "killer_character_id": killer_id,
                        "target_item_id": victim_id,
                        "solar_system_id": system,
                    }),
                },
                &None, // Don't broadcast historical events via SSE
            );

            total += 1;
        }

        let page_info = &json["data"]["objects"]["pageInfo"];
        let has_next = page_info["hasNextPage"].as_bool().unwrap_or(false);

        if !has_next {
            break;
        }

        cursor = page_info["endCursor"].as_str().map(|s| s.to_string());

        tracing::info!("Loaded {total} historical killmails so far...");
    }

    tracing::info!(
        "Historical load complete: {total} killmails, {} profiles seeded",
        state.read().await.live.profiles.len()
    );

    Ok(total)
}
