wit_bindgen::generate!({
    world: "demo-generator",
    path: "wit",
    generate_all,
});

use exports::wasmcloud::messaging::handler::Guest;
use serde::Serialize;
use wasmcloud::messaging::consumer;

/// Outgoing event published to sentinel.events for the threat-engine to process.
#[derive(Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum ThreatEvent {
    Kill(KillEvent),
    Bounty(BountyEvent),
}

#[derive(Serialize)]
struct KillEvent {
    victim_id: u64,
    attacker_ids: Vec<u64>,
    system_id: String,
    timestamp: u64,
    ship_type_id: u64,
}

#[derive(Serialize)]
struct BountyEvent {
    target_id: u64,
    poster_id: u64,
    amount_mist: u64,
    timestamp: u64,
}

struct Component;
export!(Component);

// Demo characters — same spread as the Tokio backend to keep tier distribution consistent.
const CHARACTERS: &[(u64, &str)] = &[
    (90042, "Zero Pragma"),
    (88401, "Vex Nightburn"),
    (83216, "Wraith Decimax"),
    (71035, "Lyra Ironveil"),
    (55102, "Kira Ashfall"),
    (77320, "Dread Solaris"),
    (15467, "Nyx Corvane"),
    (47688, "Rook Vantage"),
    (12055, "Mira Voidwalker"),
    (33781, "Talon Drift"),
    (28903, "Jace Holloway"),
    (41199, "Sable Ren"),
    (50710, "Ember Kael"),
    (62830, "Ash Meridian"),
    (19544, "Lux Tenebris"),
    (34201, "Hex Remnant"),
    (56789, "Flux Daemon"),
    (67234, "Echo Vanta"),
    (78901, "Null Vector"),
    (89012, "Shade Proxy"),
    (91234, "Drift Cipher"),
    (10234, "Blaze Axiom"),
    (11345, "Void Syntex"),
    (12456, "Wreck Auton"),
    (13567, "Ghost Sigma"),
];

const SYSTEMS: &[&str] = &[
    "J-1042", "K-9731", "X-4419", "N-8820", "Z-0091",
    "H-5534", "Q-2281", "R-0099", "W-7714",
];

const SHIP_TYPES: &[u64] = &[1, 2, 3, 4, 5, 6, 7, 8];

/// Small deterministic pseudo-random (LCG). We derive a seed from the message
/// body so each tick produces different events without needing WASI random.
fn lcg_next(state: &mut u64) -> u64 {
    *state = state.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
    *state
}

fn pick_idx(len: usize, rng: &mut u64) -> usize {
    (lcg_next(rng) as usize) % len
}

/// Approximate demo timestamp — fixed base offset by the current seed.
fn demo_timestamp(seed: u64) -> u64 {
    // ~2025-04-01T00:00:00 UTC in ms
    1_743_465_600_000u64.wrapping_add(seed % 3_600_000)
}

impl Guest for Component {
    fn handle_message(msg: wasmcloud::messaging::types::BrokerMessage) -> Result<(), String> {
        // Derive a seed from the message body so each tick differs.
        let seed: u64 = msg.body.iter().enumerate().fold(0u64, |acc, (i, &b)| {
            acc.wrapping_add((b as u64).wrapping_mul(i as u64 + 1))
        });
        let mut rng = seed.wrapping_add(0xDEAD_BEEF_1234_5678);

        // Emit 1-3 events per tick.
        let event_count = 1 + (lcg_next(&mut rng) % 3) as usize;

        for _ in 0..event_count {
            let roll = lcg_next(&mut rng) % 100;
            if roll < 70 {
                // Kill event (more common)
                let attacker_idx = pick_idx(CHARACTERS.len(), &mut rng);
                let mut victim_idx = pick_idx(CHARACTERS.len(), &mut rng);
                if victim_idx == attacker_idx {
                    victim_idx = (attacker_idx + 1) % CHARACTERS.len();
                }
                let attacker_id = CHARACTERS[attacker_idx].0;
                let victim_id = CHARACTERS[victim_idx].0;
                let system_idx = pick_idx(SYSTEMS.len(), &mut rng);
                let ship_idx = pick_idx(SHIP_TYPES.len(), &mut rng);
                let ts = demo_timestamp(lcg_next(&mut rng));

                publish_event(&ThreatEvent::Kill(KillEvent {
                    victim_id,
                    attacker_ids: vec![attacker_id],
                    system_id: SYSTEMS[system_idx].to_string(),
                    timestamp: ts,
                    ship_type_id: SHIP_TYPES[ship_idx],
                }))?;
            } else {
                // Bounty event
                let target_idx = pick_idx(CHARACTERS.len(), &mut rng);
                let mut poster_idx = pick_idx(CHARACTERS.len(), &mut rng);
                if poster_idx == target_idx {
                    poster_idx = (target_idx + 1) % CHARACTERS.len();
                }
                let target_id = CHARACTERS[target_idx].0;
                let poster_id = CHARACTERS[poster_idx].0;
                let amount = (lcg_next(&mut rng) % 10 + 1) * 100_000;
                let ts = demo_timestamp(lcg_next(&mut rng));

                publish_event(&ThreatEvent::Bounty(BountyEvent {
                    target_id,
                    poster_id,
                    amount_mist: amount,
                    timestamp: ts,
                }))?;
            }
        }

        Ok(())
    }
}

fn publish_event<T: Serialize>(event: &T) -> Result<(), String> {
    let body = serde_json::to_vec(event).map_err(|e| format!("serialize event: {e}"))?;
    consumer::publish(&wasmcloud::messaging::types::BrokerMessage {
        subject: "sentinel.events".to_string(),
        reply_to: None,
        body,
    })
    .map_err(|e| format!("publish to sentinel.events: {e}"))
}
