/// On-chain threat intelligence registry for EVE Frontier.
///
/// The backend service polls game events (killmails, bounties, gate jumps),
/// computes per-player threat scores, and publishes them here via `batch_update`.
/// Smart gates read scores to autonomously block high-threat players.
///
/// Scores are 0-10000 (basis points), displayed as 0-100.00: e.g. 5000 = 50.00.
#[allow(unused_const)]
module sentinel::threat_registry {
    use sentinel::config::AdminCap;
    use std::string::String;
    use sui::{clock::Clock, dynamic_field as df, event};

    // === Errors ===
    #[error(code = 0)]
    const EEntryNotFound: vector<u8> = b"No threat entry for this character";
    #[error(code = 1)]
    const EBatchLengthMismatch: vector<u8> = b"All input vectors must have the same length";
    #[error(code = 2)]
    const EBatchTooLarge: vector<u8> = b"Batch size exceeds maximum of 50";
    #[error(code = 3)]
    const EScoreOutOfRange: vector<u8> = b"Threat score must be 0-10000";

    const MAX_BATCH_SIZE: u64 = 50;
    const MAX_SCORE: u64 = 10000;

    // === Structs ===

    /// Shared registry tracking threat entries for all known characters.
    public struct ThreatRegistry has key {
        id: UID,
        entry_count: u64,
        last_updated_ms: u64,
    }

    /// Per-character threat data, stored as dynamic field on ThreatRegistry.
    public struct ThreatEntry has copy, drop, store {
        character_item_id: u64,
        threat_score: u64,
        kill_count: u64,
        death_count: u64,
        bounty_count: u64,
        last_kill_timestamp: u64,
        last_seen_system: String,
        updated_at: u64,
    }

    /// Dynamic field key for threat entries.
    public struct ThreatEntryKey has copy, drop, store { character_item_id: u64 }

    // === Events ===

    public struct ThreatUpdatedEvent has copy, drop {
        character_item_id: u64,
        old_score: u64,
        new_score: u64,
        kill_count: u64,
        death_count: u64,
    }

    public struct RegistryCreatedEvent has copy, drop {
        registry_id: ID,
    }

    // === Admin Functions ===

    /// Create the shared ThreatRegistry. Call once after deploy.
    public fun create_registry(_admin_cap: &AdminCap, clock: &Clock, ctx: &mut TxContext) {
        let registry = ThreatRegistry {
            id: object::new(ctx),
            entry_count: 0,
            last_updated_ms: clock.timestamp_ms(),
        };
        let registry_id = object::id(&registry);
        transfer::share_object(registry);

        event::emit(RegistryCreatedEvent { registry_id });
    }

    /// Batch update threat entries. Called by the backend service every ~30s.
    /// All vectors must be the same length (up to MAX_BATCH_SIZE).
    public fun batch_update(
        registry: &mut ThreatRegistry,
        _admin_cap: &AdminCap,
        character_ids: vector<u64>,
        scores: vector<u64>,
        kills: vector<u64>,
        deaths: vector<u64>,
        bounties: vector<u64>,
        timestamps: vector<u64>,
        systems: vector<String>,
        clock: &Clock,
    ) {
        let len = character_ids.length();
        assert!(len <= MAX_BATCH_SIZE, EBatchTooLarge);
        assert!(
            scores.length() == len &&
        kills.length() == len &&
        deaths.length() == len &&
        bounties.length() == len &&
        timestamps.length() == len &&
        systems.length() == len,
            EBatchLengthMismatch,
        );

        let now = clock.timestamp_ms();
        let mut i: u64 = 0;

        while (i < len) {
            let character_item_id = character_ids[i];
            let new_score = scores[i];
            assert!(new_score <= MAX_SCORE, EScoreOutOfRange);

            let key = ThreatEntryKey { character_item_id };
            let old_score = if (df::exists_(&registry.id, key)) {
                let existing: &ThreatEntry = df::borrow(&registry.id, key);
                let old = existing.threat_score;
                let _removed: ThreatEntry = df::remove(&mut registry.id, key);
                old
            } else {
                registry.entry_count = registry.entry_count + 1;
                0
            };

            let entry = ThreatEntry {
                character_item_id,
                threat_score: new_score,
                kill_count: kills[i],
                death_count: deaths[i],
                bounty_count: bounties[i],
                last_kill_timestamp: timestamps[i],
                last_seen_system: systems[i],
                updated_at: now,
            };
            df::add(&mut registry.id, key, entry);

            event::emit(ThreatUpdatedEvent {
                character_item_id,
                old_score,
                new_score,
                kill_count: kills[i],
                death_count: deaths[i],
            });

            i = i + 1;
        };

        registry.last_updated_ms = now;
    }

    // === View Functions ===

    public fun get_threat_score(registry: &ThreatRegistry, character_item_id: u64): u64 {
        let key = ThreatEntryKey { character_item_id };
        assert!(df::exists_(&registry.id, key), EEntryNotFound);
        let entry: &ThreatEntry = df::borrow(&registry.id, key);
        entry.threat_score
    }

    public fun get_entry(registry: &ThreatRegistry, character_item_id: u64): &ThreatEntry {
        let key = ThreatEntryKey { character_item_id };
        assert!(df::exists_(&registry.id, key), EEntryNotFound);
        df::borrow(&registry.id, key)
    }

    public fun has_entry(registry: &ThreatRegistry, character_item_id: u64): bool {
        df::exists_(&registry.id, ThreatEntryKey { character_item_id })
    }

    public fun entry_count(registry: &ThreatRegistry): u64 {
        registry.entry_count
    }

    public fun last_updated_ms(registry: &ThreatRegistry): u64 {
        registry.last_updated_ms
    }

    // === ThreatEntry accessors ===

    public fun entry_character_item_id(entry: &ThreatEntry): u64 { entry.character_item_id }

    public fun entry_threat_score(entry: &ThreatEntry): u64 { entry.threat_score }

    public fun entry_kill_count(entry: &ThreatEntry): u64 { entry.kill_count }

    public fun entry_death_count(entry: &ThreatEntry): u64 { entry.death_count }

    public fun entry_bounty_count(entry: &ThreatEntry): u64 { entry.bounty_count }

    public fun entry_last_kill_timestamp(entry: &ThreatEntry): u64 { entry.last_kill_timestamp }

    public fun entry_last_seen_system(entry: &ThreatEntry): String { entry.last_seen_system }

    public fun entry_updated_at(entry: &ThreatEntry): u64 { entry.updated_at }
}
