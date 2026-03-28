//! Demo mode — seeds fake threat profiles and streams random events.

use rand::{Rng, SeedableRng, rngs::StdRng};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::threat_engine;
use crate::types::{AppState, RawEvent, ThreatProfile};

struct DemoCharacter {
    id: u64,
    name: &'static str,
    kills: u64,
    deaths: u64,
    bounties: u64,
    recent_24h: u64,
    systems: u64,
    system_name: &'static str,
}

const DEMO_CHARACTERS: &[DemoCharacter] = &[
    DemoCharacter {
        id: 88401,
        name: "Vex Nightburn",
        kills: 11,
        deaths: 3,
        bounties: 2,
        recent_24h: 4,
        systems: 6,
        system_name: "J-1042",
    },
    DemoCharacter {
        id: 55102,
        name: "Kira Ashfall",
        kills: 8,
        deaths: 5,
        bounties: 1,
        recent_24h: 3,
        systems: 8,
        system_name: "K-9731",
    },
    DemoCharacter {
        id: 77320,
        name: "Dread Solaris",
        kills: 7,
        deaths: 1,
        bounties: 1,
        recent_24h: 2,
        systems: 3,
        system_name: "X-4419",
    },
    DemoCharacter {
        id: 12055,
        name: "Mira Voidwalker",
        kills: 5,
        deaths: 6,
        bounties: 1,
        recent_24h: 1,
        systems: 12,
        system_name: "N-8820",
    },
    DemoCharacter {
        id: 33781,
        name: "Talon Drift",
        kills: 4,
        deaths: 7,
        bounties: 0,
        recent_24h: 1,
        systems: 5,
        system_name: "J-1042",
    },
    DemoCharacter {
        id: 90042,
        name: "Zero Pragma",
        kills: 14,
        deaths: 2,
        bounties: 3,
        recent_24h: 6,
        systems: 2,
        system_name: "Z-0091",
    },
    DemoCharacter {
        id: 41199,
        name: "Sable Ren",
        kills: 3,
        deaths: 3,
        bounties: 0,
        recent_24h: 0,
        systems: 7,
        system_name: "H-5534",
    },
    DemoCharacter {
        id: 62830,
        name: "Ash Meridian",
        kills: 1,
        deaths: 8,
        bounties: 0,
        recent_24h: 0,
        systems: 4,
        system_name: "K-9731",
    },
    DemoCharacter {
        id: 15467,
        name: "Nyx Corvane",
        kills: 6,
        deaths: 3,
        bounties: 1,
        recent_24h: 2,
        systems: 5,
        system_name: "X-4419",
    },
    DemoCharacter {
        id: 28903,
        name: "Jace Holloway",
        kills: 3,
        deaths: 4,
        bounties: 0,
        recent_24h: 1,
        systems: 9,
        system_name: "N-8820",
    },
    DemoCharacter {
        id: 50710,
        name: "Ember Kael",
        kills: 1,
        deaths: 0,
        bounties: 0,
        recent_24h: 0,
        systems: 2,
        system_name: "J-1042",
    },
    DemoCharacter {
        id: 83216,
        name: "Wraith Decimax",
        kills: 9,
        deaths: 2,
        bounties: 2,
        recent_24h: 3,
        systems: 4,
        system_name: "Z-0091",
    },
    DemoCharacter {
        id: 19544,
        name: "Lux Tenebris",
        kills: 0,
        deaths: 3,
        bounties: 0,
        recent_24h: 0,
        systems: 3,
        system_name: "H-5534",
    },
    DemoCharacter {
        id: 47688,
        name: "Rook Vantage",
        kills: 4,
        deaths: 5,
        bounties: 1,
        recent_24h: 1,
        systems: 6,
        system_name: "X-4419",
    },
    DemoCharacter {
        id: 71035,
        name: "Lyra Ironveil",
        kills: 7,
        deaths: 1,
        bounties: 1,
        recent_24h: 2,
        systems: 4,
        system_name: "K-9731",
    },
];

const SYSTEMS: &[&str] = &[
    "J-1042", "K-9731", "X-4419", "N-8820", "Z-0091", "H-5534", "Q-2281", "R-0099", "W-7714",
];

pub async fn seed_demo_data(state: Arc<RwLock<AppState>>) {
    tracing::info!(
        "DEMO MODE — seeding {} threat profiles",
        DEMO_CHARACTERS.len()
    );

    let now = chrono::Utc::now().timestamp_millis() as u64;
    let mut s = state.write().await;

    for c in DEMO_CHARACTERS {
        s.demo.name_cache.insert(c.id, c.name.to_string());
        let mut profile = ThreatProfile {
            character_item_id: c.id,
            name: c.name.to_string(),
            kill_count: c.kills,
            death_count: c.deaths,
            bounty_count: c.bounties,
            recent_kills_24h: c.recent_24h,
            systems_visited: c.systems,
            last_seen_system: c.system_name.to_string(),
            last_kill_timestamp: now - 3_600_000 + (c.id % 3600) * 1000,
            ..Default::default()
        };
        profile.threat_score = threat_engine::compute_score(&profile).min(9500);
        s.demo.profiles.insert(c.id, profile);
    }

    // Seed 10 recent events spread across the last 5 minutes
    let mut rng = StdRng::from_os_rng();
    for _ in 0..10 {
        let age_ms = rng.random_range(10_000..300_000u64);
        let ts = now - age_ms;
        let char_count = DEMO_CHARACTERS.len();

        let event_roll: u32 = rng.random_range(0..100);
        let (event_type, data) = if event_roll < 40 {
            let killer = &DEMO_CHARACTERS[rng.random_range(0..char_count)];
            let mut victim_idx = rng.random_range(0..char_count);
            while DEMO_CHARACTERS[victim_idx].id == killer.id {
                victim_idx = rng.random_range(0..char_count);
            }
            let victim = &DEMO_CHARACTERS[victim_idx];
            (
                "kill",
                serde_json::json!({
                    "killer_character_id": killer.id,
                    "target_item_id": victim.id,
                    "solar_system_id": SYSTEMS[rng.random_range(0..SYSTEMS.len())],
                }),
            )
        } else if event_roll < 60 {
            let c = &DEMO_CHARACTERS[rng.random_range(0..char_count)];
            (
                "jump",
                serde_json::json!({
                    "character_id": c.id,
                    "solar_system_id": SYSTEMS[rng.random_range(0..SYSTEMS.len())],
                }),
            )
        } else if event_roll < 75 {
            let poster = &DEMO_CHARACTERS[rng.random_range(0..char_count)];
            let target = &DEMO_CHARACTERS[rng.random_range(0..char_count)];
            (
                "bounty_posted",
                serde_json::json!({
                    "poster_id": poster.id,
                    "target_item_id": target.id,
                }),
            )
        } else if event_roll < 85 {
            let contributor = &DEMO_CHARACTERS[rng.random_range(0..char_count)];
            let target = &DEMO_CHARACTERS[rng.random_range(0..char_count)];
            (
                "bounty_stacked",
                serde_json::json!({
                    "contributor_id": contributor.id,
                    "target_item_id": target.id,
                    "reward_quantity_added": rng.random_range(1..15u32),
                }),
            )
        } else if event_roll < 92 {
            let hunter = &DEMO_CHARACTERS[rng.random_range(0..char_count)];
            let target = &DEMO_CHARACTERS[rng.random_range(0..char_count)];
            (
                "bounty_claimed",
                serde_json::json!({
                    "hunter_id": hunter.id,
                    "target_item_id": target.id,
                    "reward_quantity": rng.random_range(5..40u32),
                }),
            )
        } else {
            let c = &DEMO_CHARACTERS[rng.random_range(0..char_count)];
            (
                "gate_blocked",
                serde_json::json!({
                    "character_id": c.id,
                    "gate_system": SYSTEMS[rng.random_range(0..SYSTEMS.len())],
                }),
            )
        };

        let sse = s.sse_tx.clone();
        s.demo.push_event(
            RawEvent {
                event_type: event_type.to_string(),
                timestamp_ms: ts,
                data,
            },
            &sse,
        );
    }

    // Sort events newest first
    s.demo
        .recent_events
        .make_contiguous()
        .sort_by(|a, b| b.timestamp_ms.cmp(&a.timestamp_ms));

    tracing::info!(
        "Demo data seeded: {} profiles, {} events",
        s.demo.profiles.len(),
        s.demo.recent_events.len()
    );
}

/// Spawn a background loop that generates random events every 3-8 seconds.
pub async fn demo_event_loop(state: Arc<RwLock<AppState>>) {
    tracing::info!("Demo event stream started");
    let mut rng = StdRng::from_os_rng();

    loop {
        // Random delay 5-12 seconds
        let delay_ms = rng.random_range(5_000..12_000u64);
        tokio::time::sleep(std::time::Duration::from_millis(delay_ms)).await;

        let now = chrono::Utc::now().timestamp_millis() as u64;
        let char_count = DEMO_CHARACTERS.len();

        // Pick a random event type (weighted)
        let event_roll: u32 = rng.random_range(0..120);
        let (event_type, data) = if event_roll < 35 {
            // Kill
            let killer_idx = rng.random_range(0..char_count);
            let mut victim_idx = rng.random_range(0..char_count);
            while victim_idx == killer_idx {
                victim_idx = rng.random_range(0..char_count);
            }
            let killer = &DEMO_CHARACTERS[killer_idx];
            let victim = &DEMO_CHARACTERS[victim_idx];
            let system = SYSTEMS[rng.random_range(0..SYSTEMS.len())];

            (
                "kill",
                serde_json::json!({
                    "killer_character_id": killer.id,
                    "target_item_id": victim.id,
                    "solar_system_id": system,
                }),
            )
        } else if event_roll < 55 {
            // Jump
            let char_idx = rng.random_range(0..char_count);
            let c = &DEMO_CHARACTERS[char_idx];
            let system = SYSTEMS[rng.random_range(0..SYSTEMS.len())];

            (
                "jump",
                serde_json::json!({
                    "character_id": c.id,
                    "solar_system_id": system,
                }),
            )
        } else if event_roll < 68 {
            // Bounty posted (with poster)
            let poster_idx = rng.random_range(0..char_count);
            let mut target_idx = rng.random_range(0..char_count);
            while target_idx == poster_idx {
                target_idx = rng.random_range(0..char_count);
            }
            let poster = &DEMO_CHARACTERS[poster_idx];
            let target = &DEMO_CHARACTERS[target_idx];

            (
                "bounty_posted",
                serde_json::json!({
                    "poster_id": poster.id,
                    "target_item_id": target.id,
                }),
            )
        } else if event_roll < 78 {
            // Bounty stacked
            let contributor_idx = rng.random_range(0..char_count);
            let target_idx = rng.random_range(0..char_count);
            let contributor = &DEMO_CHARACTERS[contributor_idx];
            let target = &DEMO_CHARACTERS[target_idx];

            (
                "bounty_stacked",
                serde_json::json!({
                    "contributor_id": contributor.id,
                    "target_item_id": target.id,
                    "reward_quantity_added": rng.random_range(1..20u32),
                }),
            )
        } else if event_roll < 86 {
            // Bounty removed (with poster)
            let poster_idx = rng.random_range(0..char_count);
            let target_idx = rng.random_range(0..char_count);
            let poster = &DEMO_CHARACTERS[poster_idx];
            let target = &DEMO_CHARACTERS[target_idx];

            (
                "bounty_removed",
                serde_json::json!({
                    "poster_id": poster.id,
                    "target_item_id": target.id,
                }),
            )
        } else if event_roll < 93 {
            // Bounty claimed
            let hunter_idx = rng.random_range(0..char_count);
            let target_idx = rng.random_range(0..char_count);
            let hunter = &DEMO_CHARACTERS[hunter_idx];
            let target = &DEMO_CHARACTERS[target_idx];

            (
                "bounty_claimed",
                serde_json::json!({
                    "hunter_id": hunter.id,
                    "target_item_id": target.id,
                    "reward_quantity": rng.random_range(5..50u32),
                }),
            )
        } else if event_roll < 100 {
            // Score change (synthetic — happens after kills but we emit it separately for the feed)
            let char_idx = rng.random_range(0..char_count);
            let c = &DEMO_CHARACTERS[char_idx];
            let new_score = {
                let s = state.read().await;
                s.demo
                    .profiles
                    .get(&c.id)
                    .map(|p| p.threat_score)
                    .unwrap_or(0)
            };

            (
                "score_change",
                serde_json::json!({
                    "character_id": c.id,
                    "new_score": new_score,
                    "old_score": new_score.saturating_sub(rng.random_range(50..300)),
                }),
            )
        } else if event_roll < 110 {
            // Gate blocked
            let char_idx = rng.random_range(0..char_count);
            let c = &DEMO_CHARACTERS[char_idx];
            let system = SYSTEMS[rng.random_range(0..SYSTEMS.len())];
            let threat_score = {
                let s = state.read().await;
                s.demo
                    .profiles
                    .get(&c.id)
                    .map(|p| p.threat_score)
                    .unwrap_or(0)
            };

            (
                "gate_blocked",
                serde_json::json!({
                    "character_id": c.id,
                    "gate_system": system,
                    "threat_score": threat_score,
                }),
            )
        } else {
            // New character detected
            let new_id: u64 = rng.random_range(100_000..999_999);
            let names = [
                "Ghost Sigma",
                "Wreck Auton",
                "Null Vector",
                "Shade Proxy",
                "Flux Daemon",
                "Echo Vanta",
                "Drift Cipher",
                "Blaze Axiom",
                "Void Syntex",
                "Hex Remnant",
            ];
            let name = names[rng.random_range(0..names.len())];

            // Add to state
            {
                let mut s = state.write().await;
                s.demo.name_cache.insert(new_id, name.to_string());
                let profile = ThreatProfile {
                    character_item_id: new_id,
                    name: name.to_string(),
                    last_seen_system: SYSTEMS[rng.random_range(0..SYSTEMS.len())].to_string(),
                    systems_visited: 1,
                    ..Default::default()
                };
                s.demo.profiles.insert(new_id, profile);
            }

            (
                "new_character",
                serde_json::json!({
                    "character_id": new_id,
                }),
            )
        };

        // Apply event to state
        let mut s = state.write().await;

        match event_type {
            "kill" => {
                let killer_id = data["killer_character_id"].as_u64().unwrap();
                let victim_id = data["target_item_id"].as_u64().unwrap();

                if let Some(p) = s.demo.profiles.get_mut(&killer_id) {
                    p.kill_count += 1;
                    p.recent_kills_24h += 1;
                    p.last_kill_timestamp = now;
                    if let Some(sys) = data["solar_system_id"].as_str() {
                        p.last_seen_system = sys.to_string();
                    }
                    p.threat_score = threat_engine::compute_score(p).min(9500);
                }
                if let Some(p) = s.demo.profiles.get_mut(&victim_id) {
                    p.death_count += 1;
                    p.threat_score = threat_engine::compute_score(p).min(9500);
                }
            }
            "jump" => {
                let char_id = data["character_id"].as_u64().unwrap();
                if let Some(p) = s.demo.profiles.get_mut(&char_id) {
                    if let Some(sys) = data["solar_system_id"].as_str() {
                        if p.last_seen_system != sys {
                            p.systems_visited += 1;
                            p.last_seen_system = sys.to_string();
                        }
                    }
                    p.threat_score = threat_engine::compute_score(p).min(9500);
                }
            }
            "bounty_posted" => {
                let target_id = data["target_item_id"].as_u64().unwrap();
                if let Some(p) = s.demo.profiles.get_mut(&target_id) {
                    p.bounty_count += 1;
                    p.threat_score = threat_engine::compute_score(p).min(9500);
                }
            }
            "bounty_removed" => {
                let target_id = data["target_item_id"].as_u64().unwrap();
                if let Some(p) = s.demo.profiles.get_mut(&target_id) {
                    p.bounty_count = p.bounty_count.saturating_sub(1);
                    p.threat_score = threat_engine::compute_score(p).min(9500);
                }
            }
            _ => {}
        }

        let sse = s.sse_tx.clone();
        s.demo.push_event(
            RawEvent {
                event_type: event_type.to_string(),
                timestamp_ms: now,
                data,
            },
            &sse,
        );
    }
}
