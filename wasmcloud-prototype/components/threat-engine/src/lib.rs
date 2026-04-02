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

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(f: impl FnOnce(&mut ThreatProfile)) -> ThreatProfile {
        let mut p = ThreatProfile::default();
        f(&mut p);
        p
    }

    // ─── compute_score ──────────────────────────────────────────────────

    #[test]
    fn score_zero_for_blank_profile() {
        let p = ThreatProfile::default();
        // Only kill_factor contributes: log2(1) * 300 = 0
        assert_eq!(compute_score(&p), 0);
    }

    #[test]
    fn score_increases_with_kills() {
        let low = profile(|p| p.kill_count = 1);
        let high = profile(|p| p.kill_count = 20);
        assert!(compute_score(&high) > compute_score(&low));
    }

    #[test]
    fn score_increases_with_recent_kills() {
        let base = profile(|p| {
            p.kill_count = 5;
            p.recent_kills_24h = 0;
        });
        let hot = profile(|p| {
            p.kill_count = 5;
            p.recent_kills_24h = 5;
        });
        assert!(compute_score(&hot) > compute_score(&base));
    }

    #[test]
    fn score_capped_at_10000() {
        let maxed = profile(|p| {
            p.kill_count = 1000;
            p.recent_kills_24h = 100;
            p.bounty_count = 100;
            p.systems_visited = 100;
        });
        assert_eq!(compute_score(&maxed), 10000);
    }

    #[test]
    fn score_kd_factor_uses_deaths_floor_of_one() {
        // With 0 deaths, kd = kill_count / 1 (not division by zero)
        let p = profile(|p| p.kill_count = 10);
        let score = compute_score(&p);
        assert!(score > 0);
    }

    #[test]
    fn score_bounty_factor_capped_at_1000() {
        let a = profile(|p| p.bounty_count = 3);
        let b = profile(|p| p.bounty_count = 100);
        // Both should have bounty_factor = 999 and 1000 respectively
        let diff = compute_score(&b) - compute_score(&a);
        assert!(diff <= 1); // capped, so negligible difference
    }

    #[test]
    fn score_movement_factor_capped_at_500() {
        let a = profile(|p| p.systems_visited = 5);
        let b = profile(|p| p.systems_visited = 50);
        let diff = compute_score(&b) - compute_score(&a);
        assert_eq!(diff, 0); // both hit the cap
    }

    // ─── threat_tier ────────────────────────────────────────────────────

    #[test]
    fn tier_boundaries() {
        assert_eq!(threat_tier(0), "LOW");
        assert_eq!(threat_tier(2500), "LOW");
        assert_eq!(threat_tier(2501), "MODERATE");
        assert_eq!(threat_tier(5000), "MODERATE");
        assert_eq!(threat_tier(5001), "HIGH");
        assert_eq!(threat_tier(7500), "HIGH");
        assert_eq!(threat_tier(7501), "CRITICAL");
        assert_eq!(threat_tier(10000), "CRITICAL");
    }

    // ─── earned_titles ──────────────────────────────────────────────────

    #[test]
    fn no_titles_for_blank_profile() {
        let p = ThreatProfile::default();
        assert!(earned_titles(&p).is_empty());
    }

    #[test]
    fn serial_killer_requires_10_kills_and_low_deaths() {
        let yes = profile(|p| { p.kill_count = 10; p.death_count = 2; });
        let no = profile(|p| { p.kill_count = 10; p.death_count = 3; });
        assert!(earned_titles(&yes).contains(&"Serial Killer"));
        assert!(!earned_titles(&no).contains(&"Serial Killer"));
    }

    #[test]
    fn apex_predator_at_50_kills() {
        let p = profile(|p| p.kill_count = 50);
        assert!(earned_titles(&p).contains(&"Apex Predator"));
    }

    #[test]
    fn untouchable_requires_5_kills_zero_deaths() {
        let yes = profile(|p| { p.kill_count = 5; p.death_count = 0; });
        let no = profile(|p| { p.kill_count = 5; p.death_count = 1; });
        assert!(earned_titles(&yes).contains(&"Untouchable"));
        assert!(!earned_titles(&no).contains(&"Untouchable"));
    }

    #[test]
    fn rampage_at_5_recent_kills() {
        let p = profile(|p| p.recent_kills_24h = 5);
        assert!(earned_titles(&p).contains(&"Rampage"));
    }

    #[test]
    fn bounty_magnet_at_1_bounty() {
        let p = profile(|p| p.bounty_count = 1);
        assert!(earned_titles(&p).contains(&"Bounty Magnet"));
    }

    #[test]
    fn most_wanted_at_3_bounties() {
        let p = profile(|p| p.bounty_count = 3);
        let titles = earned_titles(&p);
        assert!(titles.contains(&"Most Wanted"));
        assert!(titles.contains(&"Bounty Magnet"));
    }

    #[test]
    fn ghost_requires_5_systems_zero_kills() {
        let yes = profile(|p| { p.systems_visited = 5; p.kill_count = 0; });
        let no = profile(|p| { p.systems_visited = 5; p.kill_count = 1; });
        assert!(earned_titles(&yes).contains(&"Ghost"));
        assert!(!earned_titles(&no).contains(&"Ghost"));
    }

    #[test]
    fn nomad_and_cartographer() {
        let nomad = profile(|p| p.systems_visited = 10);
        let carto = profile(|p| p.systems_visited = 20);
        assert!(earned_titles(&nomad).contains(&"Nomad"));
        assert!(!earned_titles(&nomad).contains(&"Cartographer"));
        assert!(earned_titles(&carto).contains(&"Cartographer"));
    }

    #[test]
    fn gate_threat_and_public_enemy() {
        let gate = profile(|p| p.threat_score = 7500);
        let public = profile(|p| p.threat_score = 9000);
        assert!(earned_titles(&gate).contains(&"Gate Threat"));
        assert!(!earned_titles(&gate).contains(&"Public Enemy"));
        assert!(earned_titles(&public).contains(&"Public Enemy"));
    }

    #[test]
    fn frequent_victim() {
        let yes = profile(|p| { p.death_count = 10; p.kill_count = 2; });
        let no = profile(|p| { p.death_count = 10; p.kill_count = 3; });
        assert!(earned_titles(&yes).contains(&"Frequent Victim"));
        assert!(!earned_titles(&no).contains(&"Frequent Victim"));
    }

    #[test]
    fn warrior_requires_5_kills_and_5_deaths() {
        let p = profile(|p| { p.kill_count = 5; p.death_count = 5; });
        assert!(earned_titles(&p).contains(&"Warrior"));
    }

    #[test]
    fn efficient_killer_requires_3_kd_and_5_kills() {
        let yes = profile(|p| { p.kill_count = 15; p.death_count = 5; }); // kd = 3.0
        let no = profile(|p| { p.kill_count = 14; p.death_count = 5; }); // kd = 2.8
        assert!(earned_titles(&yes).contains(&"Efficient Killer"));
        assert!(!earned_titles(&no).contains(&"Efficient Killer"));
    }

    // ─── Event deserialization ───────────────────────────────────────────

    #[test]
    fn deserialize_kill_event() {
        let json = r#"{"type":"kill","victim_id":100,"attacker_ids":[200,201],"system_id":"J-1042","timestamp":1000,"ship_type_id":42}"#;
        let event: ThreatEvent = serde_json::from_str(json).unwrap();
        match event {
            ThreatEvent::Kill(k) => {
                assert_eq!(k.victim_id, 100);
                assert_eq!(k.attacker_ids, vec![200, 201]);
                assert_eq!(k.system_id, "J-1042");
            }
            _ => panic!("expected Kill"),
        }
    }

    #[test]
    fn deserialize_bounty_event() {
        let json = r#"{"type":"bounty","target_id":100,"poster_id":200,"amount_mist":5000,"timestamp":1000}"#;
        let event: ThreatEvent = serde_json::from_str(json).unwrap();
        match event {
            ThreatEvent::Bounty(b) => {
                assert_eq!(b.target_id, 100);
                assert_eq!(b.poster_id, 200);
                assert_eq!(b.amount_mist, 5000);
            }
            _ => panic!("expected Bounty"),
        }
    }

    #[test]
    fn deserialize_gate_event() {
        let json = r#"{"type":"gate","pilot_id":100,"gate_object_id":"0xabc","permitted":true,"timestamp":1000}"#;
        let event: ThreatEvent = serde_json::from_str(json).unwrap();
        match event {
            ThreatEvent::Gate(g) => {
                assert_eq!(g.pilot_id, 100);
                assert!(g.permitted);
            }
            _ => panic!("expected Gate"),
        }
    }

    // ─── ScoreUpdate / ThreatAlert serialization ────────────────────────

    #[test]
    fn score_update_serializes() {
        let update = ScoreUpdate {
            pilot_id: 42,
            name: Some("TestPilot".into()),
            threat_score: 5000,
            tier: "MODERATE",
            titles: vec!["Bounty Magnet"],
        };
        let json = serde_json::to_value(&update).unwrap();
        assert_eq!(json["pilot_id"], 42);
        assert_eq!(json["tier"], "MODERATE");
        assert_eq!(json["titles"][0], "Bounty Magnet");
    }

    #[test]
    fn threat_alert_serializes() {
        let alert = ThreatAlert {
            pilot_id: 42,
            pilot_name: "TestPilot".into(),
            threat_score: 8000,
            tier: "CRITICAL",
            system: "J-1042".into(),
        };
        let json = serde_json::to_value(&alert).unwrap();
        assert_eq!(json["pilot_name"], "TestPilot");
        assert_eq!(json["system"], "J-1042");
    }
}
