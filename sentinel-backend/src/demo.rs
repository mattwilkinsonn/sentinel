//! Demo mode — seeds fake threat profiles and streams random events.

use rand::{Rng, SeedableRng, rngs::StdRng};
use std::sync::Arc;
use tokio::sync::RwLock;

use crate::config::AppConfig;
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

// Designed for a spread across all tiers: CRITICAL > HIGH > MODERATE > LOW
// Score formula: recency(600*r, max 3500) + kills(log2*600, max 2000) + kd(kd*400, max 1500) + bounties(500*b, max 1500) + movement(100*s, max 500)
const DEMO_CHARACTERS: &[DemoCharacter] = &[
    // CRITICAL tier (7500+) — the apex threats
    DemoCharacter {
        id: 90042,
        name: "Zero Pragma",
        kills: 87,
        deaths: 3,
        bounties: 3,
        recent_24h: 8,
        systems: 5,
        system_name: "Z-0091",
    },
    DemoCharacter {
        id: 88401,
        name: "Vex Nightburn",
        kills: 64,
        deaths: 2,
        bounties: 2,
        recent_24h: 6,
        systems: 4,
        system_name: "J-1042",
    },
    // HIGH tier (5000-7500)
    DemoCharacter {
        id: 83216,
        name: "Wraith Decimax",
        kills: 42,
        deaths: 5,
        bounties: 2,
        recent_24h: 5,
        systems: 3,
        system_name: "Z-0091",
    },
    DemoCharacter {
        id: 71035,
        name: "Lyra Ironveil",
        kills: 31,
        deaths: 3,
        bounties: 1,
        recent_24h: 4,
        systems: 6,
        system_name: "K-9731",
    },
    DemoCharacter {
        id: 55102,
        name: "Kira Ashfall",
        kills: 25,
        deaths: 8,
        bounties: 1,
        recent_24h: 3,
        systems: 8,
        system_name: "K-9731",
    },
    // MODERATE tier (2500-5000)
    DemoCharacter {
        id: 77320,
        name: "Dread Solaris",
        kills: 18,
        deaths: 4,
        bounties: 1,
        recent_24h: 2,
        systems: 3,
        system_name: "X-4419",
    },
    DemoCharacter {
        id: 15467,
        name: "Nyx Corvane",
        kills: 12,
        deaths: 6,
        bounties: 0,
        recent_24h: 2,
        systems: 5,
        system_name: "X-4419",
    },
    DemoCharacter {
        id: 47688,
        name: "Rook Vantage",
        kills: 9,
        deaths: 5,
        bounties: 1,
        recent_24h: 1,
        systems: 6,
        system_name: "X-4419",
    },
    DemoCharacter {
        id: 12055,
        name: "Mira Voidwalker",
        kills: 7,
        deaths: 9,
        bounties: 0,
        recent_24h: 1,
        systems: 12,
        system_name: "N-8820",
    },
    // LOW tier (0-2500) — explorers, victims, newcomers
    DemoCharacter {
        id: 33781,
        name: "Talon Drift",
        kills: 4,
        deaths: 11,
        bounties: 0,
        recent_24h: 1,
        systems: 5,
        system_name: "J-1042",
    },
    DemoCharacter {
        id: 28903,
        name: "Jace Holloway",
        kills: 3,
        deaths: 7,
        bounties: 0,
        recent_24h: 0,
        systems: 9,
        system_name: "N-8820",
    },
    DemoCharacter {
        id: 41199,
        name: "Sable Ren",
        kills: 2,
        deaths: 4,
        bounties: 0,
        recent_24h: 0,
        systems: 7,
        system_name: "H-5534",
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
        id: 62830,
        name: "Ash Meridian",
        kills: 1,
        deaths: 12,
        bounties: 0,
        recent_24h: 1,
        systems: 4,
        system_name: "K-9731",
    },
    DemoCharacter {
        id: 19544,
        name: "Lux Tenebris",
        kills: 2,
        deaths: 5,
        bounties: 0,
        recent_24h: 1,
        systems: 3,
        system_name: "H-5534",
    },
    // Additional pilots
    DemoCharacter {
        id: 34201,
        name: "Hex Remnant",
        kills: 15,
        deaths: 4,
        bounties: 1,
        recent_24h: 2,
        systems: 7,
        system_name: "Q-2281",
    },
    DemoCharacter {
        id: 56789,
        name: "Flux Daemon",
        kills: 22,
        deaths: 6,
        bounties: 0,
        recent_24h: 3,
        systems: 4,
        system_name: "R-0099",
    },
    DemoCharacter {
        id: 67234,
        name: "Echo Vanta",
        kills: 6,
        deaths: 2,
        bounties: 1,
        recent_24h: 1,
        systems: 10,
        system_name: "W-7714",
    },
    DemoCharacter {
        id: 78901,
        name: "Null Vector",
        kills: 35,
        deaths: 7,
        bounties: 2,
        recent_24h: 4,
        systems: 5,
        system_name: "Q-2281",
    },
    DemoCharacter {
        id: 89012,
        name: "Shade Proxy",
        kills: 11,
        deaths: 9,
        bounties: 0,
        recent_24h: 2,
        systems: 8,
        system_name: "R-0099",
    },
    DemoCharacter {
        id: 91234,
        name: "Drift Cipher",
        kills: 8,
        deaths: 3,
        bounties: 1,
        recent_24h: 1,
        systems: 6,
        system_name: "W-7714",
    },
    DemoCharacter {
        id: 10234,
        name: "Blaze Axiom",
        kills: 19,
        deaths: 5,
        bounties: 0,
        recent_24h: 3,
        systems: 3,
        system_name: "Z-0091",
    },
    DemoCharacter {
        id: 11345,
        name: "Void Syntex",
        kills: 3,
        deaths: 11,
        bounties: 0,
        recent_24h: 0,
        systems: 5,
        system_name: "N-8820",
    },
    DemoCharacter {
        id: 12456,
        name: "Wreck Auton",
        kills: 28,
        deaths: 4,
        bounties: 2,
        recent_24h: 5,
        systems: 4,
        system_name: "J-1042",
    },
    DemoCharacter {
        id: 13567,
        name: "Ghost Sigma",
        kills: 0,
        deaths: 2,
        bounties: 0,
        recent_24h: 0,
        systems: 15,
        system_name: "H-5534",
    },
];

const SYSTEMS: &[&str] = &[
    "J-1042", "K-9731", "X-4419", "N-8820", "Z-0091", "H-5534", "Q-2281", "R-0099", "W-7714",
];

/// Populate the demo DataStore with pre-defined pilot profiles covering all threat tiers.
/// Called once at startup so the dashboard is immediately usable before historical data loads.
pub async fn seed_demo_data(config: &AppConfig, state: Arc<RwLock<AppState>>) {
    tracing::info!(
        profile_count = DEMO_CHARACTERS.len(),
        "DEMO MODE — seeding {} threat profiles",
        DEMO_CHARACTERS.len()
    );

    let now = chrono::Utc::now().timestamp_millis() as u64;
    let mut s = state.write().await;

    for c in DEMO_CHARACTERS {
        s.demo.name_cache.insert(c.id, c.name.to_string());
        let mut profile = ThreatProfile {
            character_item_id: c.id,
            name: Some(c.name.to_string()),
            kill_count: c.kills,
            death_count: c.deaths,
            bounty_count: c.bounties,
            recent_kills_24h: c.recent_24h,
            systems_visited: c.systems,
            last_seen_system: c.system_name.to_string(),
            last_kill_timestamp: now - 3_600_000 + (c.id % 3600) * 1000,
            ..Default::default()
        };
        profile.threat_score = threat_engine::compute_score(&profile);
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
            config.max_recent_events,
        );
    }

    // Seed new pilot events
    for c in DEMO_CHARACTERS.iter().take(5) {
        let age_ms = rng.random_range(60_000..600_000u64);
        let sse = s.sse_tx.clone();
        s.demo.push_event(
            RawEvent {
                event_type: "new_character".to_string(),
                timestamp_ms: now - age_ms,
                data: serde_json::json!({ "character_id": c.id }),
            },
            &sse,
            config.max_recent_events,
        );
    }

    // Sort events newest first
    s.demo
        .recent_events
        .make_contiguous()
        .sort_by(|a, b| b.timestamp_ms.cmp(&a.timestamp_ms));

    tracing::info!(
        profiles = s.demo.profiles.len(),
        events = s.demo.recent_events.len(),
        "Demo data seeded: {} profiles, {} events",
        s.demo.profiles.len(),
        s.demo.recent_events.len()
    );
}

/// Spawn a background loop that generates random events every 3-8 seconds.
pub async fn demo_event_loop(config: AppConfig, state: Arc<RwLock<AppState>>) {
    tracing::info!("Demo event stream started");
    let mut rng = StdRng::from_os_rng();
    let mut last_event_type = "";

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
            // Smart Gate Jump
            let char_idx = rng.random_range(0..char_count);
            let c = &DEMO_CHARACTERS[char_idx];
            let gate_names = [
                "ALPHA-1",
                "DELTA-7",
                "OMEGA-3",
                "SIGMA-9",
                "NEXUS-2",
                "VORTEX-4",
                "IRON GATE",
                "WARP GATE",
            ];
            let src = gate_names[rng.random_range(0..gate_names.len())];
            let mut dst_idx = rng.random_range(0..gate_names.len());
            while gate_names[dst_idx] == src {
                dst_idx = rng.random_range(0..gate_names.len());
            }
            let dst = gate_names[dst_idx];

            (
                "jump",
                serde_json::json!({
                    "character_id": c.id,
                    "source_gate": src,
                    "dest_gate": dst,
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
                    name: Some(name.to_string()),
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

        // Skip consecutive duplicate event types
        if event_type == last_event_type {
            continue;
        }
        last_event_type = event_type;

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
                    p.threat_score = threat_engine::compute_score(p);
                }
                if let Some(p) = s.demo.profiles.get_mut(&victim_id) {
                    p.death_count += 1;
                    p.threat_score = threat_engine::compute_score(p);
                }
            }
            "jump" => {
                let char_id = data["character_id"].as_u64().unwrap();
                if let Some(p) = s.demo.profiles.get_mut(&char_id) {
                    p.systems_visited += 1;
                    p.threat_score = threat_engine::compute_score(p);
                }
            }
            "bounty_posted" => {
                let target_id = data["target_item_id"].as_u64().unwrap();
                if let Some(p) = s.demo.profiles.get_mut(&target_id) {
                    p.bounty_count += 1;
                    p.threat_score = threat_engine::compute_score(p);
                }
            }
            "bounty_removed" => {
                let target_id = data["target_item_id"].as_u64().unwrap();
                if let Some(p) = s.demo.profiles.get_mut(&target_id) {
                    p.bounty_count = p.bounty_count.saturating_sub(1);
                    p.threat_score = threat_engine::compute_score(p);
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
            config.max_recent_events,
        );
    }
}
