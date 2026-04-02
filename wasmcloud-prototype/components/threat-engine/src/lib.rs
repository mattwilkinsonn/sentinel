wit_bindgen::generate!({
    world: "threat-engine",
    path: "wit",
    generate_all,
});

use exports::wasmcloud::messaging::handler::Guest;
use serde::{Deserialize, Serialize};
use wasmcloud::messaging::consumer;
use wasi::keyvalue::store;

/// Mirror of ThreatProfile stored in sentinel.profiles KV bucket.
/// Key format: "pilot:{item_id}"
#[derive(Debug, Default, Serialize, Deserialize)]
struct ThreatProfile {
    item_id: u64,
    name: Option<String>,
    threat_score: u64,
    kill_count: u64,
    death_count: u64,
    bounty_count: u64,
    last_kill_ts: u64,
    last_seen_system: String,
    tribe_id: Option<String>,
    tribe_name: Option<String>,
    recent_kills_24h: u64,
    recent_deaths_24h: u64,
    systems_visited: u64,
    dirty: bool,
}

/// Incoming event from sentinel.events JetStream.
/// Serialised by sui-bridge using sentinel:ingestion/types.
#[derive(Debug, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ThreatEvent {
    Kill(KillEvent),
    Bounty(BountyEvent),
    Gate(GateEvent),
}

#[derive(Debug, Deserialize)]
struct KillEvent {
    victim_id: u64,
    attacker_ids: Vec<u64>,
    system_id: String,
    timestamp: u64,
    #[allow(dead_code)]
    ship_type_id: u64,
}

#[derive(Debug, Deserialize)]
struct BountyEvent {
    target_id: u64,
    #[allow(dead_code)]
    poster_id: u64,
    #[allow(dead_code)]
    amount_mist: u64,
    #[allow(dead_code)]
    timestamp: u64,
}

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct GateEvent {
    pilot_id: u64,
    gate_object_id: String,
    permitted: bool,
    timestamp: u64,
}

/// Score update published to sentinel.scores for SSE fan-out.
#[derive(Serialize)]
struct ScoreUpdate {
    pilot_id: u64,
    name: Option<String>,
    threat_score: u64,
    tier: &'static str,
    titles: Vec<&'static str>,
}

/// Alert published to sentinel.alerts when a pilot first crosses CRITICAL.
#[derive(Serialize)]
struct ThreatAlert {
    pilot_id: u64,
    pilot_name: String,
    threat_score: u64,
    tier: &'static str,
    system: String,
}

struct Component;
export!(Component);

impl Guest for Component {
    fn handle_message(msg: wasmcloud::messaging::types::BrokerMessage) -> Result<(), String> {
        let event: ThreatEvent = serde_json::from_slice(&msg.body)
            .map_err(|e| format!("failed to deserialize event: {e}"))?;

        match event {
            ThreatEvent::Kill(e) => handle_kill(&e)?,
            ThreatEvent::Bounty(e) => handle_bounty(&e)?,
            ThreatEvent::Gate(_) => {} // gate events don't affect scores
        }

        Ok(())
    }
}

fn handle_kill(event: &KillEvent) -> Result<(), String> {
    let bucket = open_profiles_bucket()?;

    let mut victim = load_profile(&bucket, event.victim_id)?;
    victim.death_count += 1;
    victim.last_seen_system = event.system_id.clone();
    upsert_and_publish(&bucket, &mut victim, &event.system_id)?;

    for &attacker_id in &event.attacker_ids {
        let mut attacker = load_profile(&bucket, attacker_id)?;
        attacker.kill_count += 1;
        attacker.recent_kills_24h += 1;
        attacker.last_kill_ts = event.timestamp;
        attacker.last_seen_system = event.system_id.clone();
        upsert_and_publish(&bucket, &mut attacker, &event.system_id)?;
    }

    Ok(())
}

fn handle_bounty(event: &BountyEvent) -> Result<(), String> {
    let bucket = open_profiles_bucket()?;
    let mut profile = load_profile(&bucket, event.target_id)?;
    profile.bounty_count += 1;
    upsert_and_publish(&bucket, &mut profile, "")?;
    Ok(())
}

fn load_profile(bucket: &store::Bucket, pilot_id: u64) -> Result<ThreatProfile, String> {
    let key = format!("pilot:{pilot_id}");
    match bucket.get(&key).map_err(|e| format!("kv get: {e:?}"))? {
        Some(bytes) => serde_json::from_slice(&bytes).map_err(|e| format!("deserialize profile: {e}")),
        None => Ok(ThreatProfile { item_id: pilot_id, ..Default::default() }),
    }
}

/// Recompute score, mark dirty, save to KV, publish score update.
/// If the pilot just crossed into CRITICAL, also publish an alert.
fn upsert_and_publish(
    bucket: &store::Bucket,
    profile: &mut ThreatProfile,
    system: &str,
) -> Result<(), String> {
    let prev_tier = threat_tier(profile.threat_score);
    profile.threat_score = compute_score(profile);
    profile.dirty = true;

    let bytes = serde_json::to_vec(profile).map_err(|e| format!("serialize profile: {e}"))?;
    bucket
        .set(&format!("pilot:{}", profile.item_id), &bytes)
        .map_err(|e| format!("kv set: {e:?}"))?;

    let tier = threat_tier(profile.threat_score);

    publish_json("sentinel.scores", &ScoreUpdate {
        pilot_id: profile.item_id,
        name: profile.name.clone(),
        threat_score: profile.threat_score,
        tier,
        titles: earned_titles(profile),
    })?;

    if tier == "CRITICAL" && prev_tier != "CRITICAL" {
        publish_json("sentinel.alerts", &ThreatAlert {
            pilot_id: profile.item_id,
            pilot_name: profile.name.clone().unwrap_or_else(|| profile.item_id.to_string()),
            threat_score: profile.threat_score,
            tier,
            system: system.to_string(),
        })?;
    }

    Ok(())
}

fn open_profiles_bucket() -> Result<store::Bucket, String> {
    store::open("sentinel.profiles").map_err(|e| format!("open kv bucket: {e:?}"))
}

fn publish_json<T: Serialize>(subject: &str, payload: &T) -> Result<(), String> {
    let body = serde_json::to_vec(payload).map_err(|e| format!("serialize: {e}"))?;
    consumer::publish(&wasmcloud::messaging::types::BrokerMessage {
        subject: subject.to_string(),
        reply_to: None,
        body,
    })
    .map_err(|e| format!("publish to {subject}: {e}"))
}

// ─── Scoring algorithm ──────────────────────────────────────────────────────
// Ported directly from sentinel-backend/src/threat_engine.rs.
// Keep in sync with the original until the ECS backend is retired.

fn compute_score(p: &ThreatProfile) -> u64 {
    let deaths = p.death_count.max(1);
    let kd = p.kill_count as f64 / deaths as f64;
    let kd_factor = (kd * 150.0).min(1500.0) as u64;

    let recent_kd_factor = if p.recent_kills_24h > 0 {
        let recent_kd = p.recent_kills_24h as f64 / p.recent_deaths_24h.max(1) as f64;
        (recent_kd * 300.0).min(3000.0) as u64
    } else {
        0
    };

    let recency_factor = (p.recent_kills_24h * 500).min(3000);
    let kill_factor = ((p.kill_count as f64 + 1.0).log2() * 300.0).min(1000.0) as u64;
    let bounty_factor = (p.bounty_count * 333).min(1000);
    let movement_factor = (p.systems_visited * 100).min(500);

    (kd_factor + recent_kd_factor + recency_factor + kill_factor + bounty_factor + movement_factor)
        .min(10000)
}

fn threat_tier(score: u64) -> &'static str {
    match score {
        0..=2500 => "LOW",
        2501..=5000 => "MODERATE",
        5001..=7500 => "HIGH",
        _ => "CRITICAL",
    }
}

fn earned_titles(p: &ThreatProfile) -> Vec<&'static str> {
    let mut titles = Vec::new();
    if p.kill_count >= 10 && p.death_count <= 2 { titles.push("Serial Killer"); }
    if p.kill_count >= 50 { titles.push("Apex Predator"); }
    if p.kill_count >= 5 && p.death_count == 0 { titles.push("Untouchable"); }
    if p.recent_kills_24h >= 5 { titles.push("Rampage"); }
    if p.bounty_count >= 3 { titles.push("Most Wanted"); }
    if p.bounty_count >= 1 { titles.push("Bounty Magnet"); }
    if p.systems_visited >= 10 { titles.push("Nomad"); }
    if p.systems_visited >= 20 { titles.push("Cartographer"); }
    if p.systems_visited >= 5 && p.kill_count == 0 { titles.push("Ghost"); }
    if p.threat_score >= 7500 { titles.push("Gate Threat"); }
    if p.threat_score >= 9000 { titles.push("Public Enemy"); }
    if p.death_count >= 10 && p.kill_count <= 2 { titles.push("Frequent Victim"); }
    if p.death_count >= 5 && p.kill_count >= 5 { titles.push("Warrior"); }
    let kd = if p.death_count > 0 { p.kill_count as f64 / p.death_count as f64 } else { p.kill_count as f64 };
    if kd >= 3.0 && p.kill_count >= 5 { titles.push("Efficient Killer"); }
    titles
}
