/// Sentinel smart gate extension.
///
/// Gate owners authorize this extension on their gates. Players call
/// `request_passage` to get a JumpPermit — but only if their threat score
/// is below the configured threshold. High-threat players are blocked.
///
/// The player's dApp constructs the TX, passing ThreatRegistry as an argument.
#[allow(unused_const)]
module sentinel::smart_gate;

use sentinel::config::{Self, AdminCap, ExtensionConfig};
use sentinel::threat_registry::ThreatRegistry;
use sui::clock::Clock;
use world::character::Character;
use world::gate::Gate;

// === Errors ===
#[error(code = 0)]
const EThreatTooHigh: vector<u8> = b"Character threat score exceeds gate threshold";
#[error(code = 1)]
const EThresholdNotSet: vector<u8> = b"Gate threshold not configured";

/// Default permit duration: 60 seconds.
const DEFAULT_PERMIT_DURATION_MS: u64 = 60_000;

// === Config ===

/// Threshold config stored as dynamic field on ExtensionConfig.
public struct GateThreshold has drop, store {
    max_threat_score: u64,
}

public struct GateThresholdKey has copy, drop, store {}

// === Admin Functions ===

/// Set the maximum threat score allowed through sentinel-controlled gates.
public fun set_gate_threshold(
    config: &mut ExtensionConfig,
    admin_cap: &AdminCap,
    max_threat_score: u64,
) {
    config.set_rule(
        admin_cap,
        GateThresholdKey {},
        GateThreshold { max_threat_score },
    );
}

// === Player Functions ===

/// Request passage through a sentinel-controlled gate.
/// Reads the player's threat score from the registry and compares against
/// the configured threshold. Issues a JumpPermit if below threshold.
public fun request_passage(
    source_gate: &Gate,
    destination_gate: &Gate,
    character: &Character,
    registry: &ThreatRegistry,
    extension_config: &ExtensionConfig,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    // Load threshold
    assert!(
        extension_config.has_rule<GateThresholdKey>(GateThresholdKey {}),
        EThresholdNotSet,
    );
    let threshold = extension_config.borrow_rule<GateThresholdKey, GateThreshold>(
        GateThresholdKey {},
    );

    // Check threat score (unknown characters pass freely — score 0)
    let character_key = character.key();
    let character_item_id = world::in_game_id::item_id(&character_key);
    let score = if (registry.has_entry(character_item_id)) {
        registry.get_threat_score(character_item_id)
    } else {
        0
    };

    assert!(score <= threshold.max_threat_score, EThreatTooHigh);

    // Issue jump permit
    let expires_at = clock.timestamp_ms() + DEFAULT_PERMIT_DURATION_MS;
    world::gate::issue_jump_permit(
        source_gate,
        destination_gate,
        character,
        config::sentinel_auth(),
        expires_at,
        ctx,
    );
}

// === View Functions ===

public fun get_gate_threshold(config: &ExtensionConfig): u64 {
    assert!(
        config.has_rule<GateThresholdKey>(GateThresholdKey {}),
        EThresholdNotSet,
    );
    let threshold = config.borrow_rule<GateThresholdKey, GateThreshold>(GateThresholdKey {});
    threshold.max_threat_score
}
