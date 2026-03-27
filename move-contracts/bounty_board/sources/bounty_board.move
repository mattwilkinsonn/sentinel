/// On-chain Bounty Board for EVE Frontier.
///
/// Players post bounties on targets by escrowing reward items into an SSU's open
/// inventory. Hunters claim bounties by presenting a Killmail as proof-of-kill.
/// The contract validates the killmail, releases the reward to the hunter's owned
/// inventory, and marks the bounty as claimed.
///
/// Item flow:
///   Post:   poster owned inv → withdraw_by_owner → deposit_to_open_inventory (escrow)
///   Claim:  escrow → withdraw_from_open_inventory → deposit_to_owned (hunter)
///   Cancel: escrow → withdraw_from_open_inventory → deposit_to_owned (poster refund)
#[allow(unused_const)]
module bounty_board::bounty_board;

use bounty_board::config::{Self, AdminCap, ExtensionConfig};
use std::string::String;
use sui::clock::Clock;
use sui::dynamic_field as df;
use sui::event;
use world::{
    access::OwnerCap,
    character::Character,
    in_game_id,
    killmail::Killmail,
    storage_unit::StorageUnit,
};

// === Errors ===
#[error(code = 0)]
const EBountyNotFound: vector<u8> = b"Bounty not found";
#[error(code = 1)]
const ENotPoster: vector<u8> = b"Only the poster can cancel this bounty";
#[error(code = 2)]
const EBountyAlreadyClaimed: vector<u8> = b"Bounty has already been claimed";
#[error(code = 3)]
const EBountyExpired: vector<u8> = b"Bounty has expired";
#[error(code = 4)]
const EKillmailVictimMismatch: vector<u8> = b"Killmail victim does not match bounty target";
#[error(code = 5)]
const EKillmailKillerMismatch: vector<u8> = b"Only the killer can claim this bounty";
#[error(code = 6)]
const EKillmailTooOld: vector<u8> = b"Killmail is too old";
#[error(code = 7)]
const EBoardConfigMissing: vector<u8> = b"Board config not set";
#[error(code = 8)]
const EStorageUnitMismatch: vector<u8> = b"Storage unit does not match this board";
#[error(code = 9)]
const EBountyNotExpired: vector<u8> = b"Bounty has not expired yet";
#[error(code = 10)]
const EInvalidDuration: vector<u8> = b"Duration exceeds maximum allowed";
#[error(code = 11)]
const ESenderMismatch: vector<u8> = b"Transaction sender does not match character address";

// === Structs ===

/// Shared object that tracks all bounties for a specific SSU.
public struct BountyBoard has key {
    id: UID,
    next_bounty_id: u64,
    active_bounty_ids: vector<u64>,
    storage_unit_id: ID,
}

/// Individual bounty stored as a dynamic field on BountyBoard.
/// Target is stored as raw (item_id, tenant) since TenantItemId cannot be
/// constructed outside the world package.
public struct Bounty has store, drop {
    id: u64,
    target_item_id: u64,
    target_tenant: String,
    reward_type_id: u64,
    reward_quantity: u32,
    poster: address,
    poster_character_id: ID,
    created_at: u64,
    expires_at: u64,
    claimed: bool,
    claimed_by: Option<address>,
    claimed_killmail_id: Option<ID>,
}

/// Dynamic field key for bounties.
public struct BountyKey has copy, drop, store { id: u64 }

// === Config ===

/// Admin-configurable board parameters, stored on ExtensionConfig.
public struct BoardConfig has drop, store {
    max_bounty_duration_ms: u64,
    default_bounty_duration_ms: u64,
    min_killmail_recency_ms: u64,
}

public struct BoardConfigKey has copy, drop, store {}

// === Events ===

public struct BountyPostedEvent has copy, drop {
    bounty_id: u64,
    board_id: ID,
    target_item_id: u64,
    target_tenant: String,
    reward_type_id: u64,
    reward_quantity: u32,
    poster: address,
    expires_at: u64,
}

public struct BountyClaimedEvent has copy, drop {
    bounty_id: u64,
    board_id: ID,
    hunter: address,
    killmail_id: ID,
    reward_type_id: u64,
    reward_quantity: u32,
}

public struct BountyCancelledEvent has copy, drop {
    bounty_id: u64,
    board_id: ID,
    poster: address,
}

// === Admin Functions ===

/// Create a BountyBoard bound to a specific StorageUnit. Call once after deploy.
public fun create_board(
    _admin_cap: &AdminCap,
    storage_unit_id: ID,
    ctx: &mut TxContext,
) {
    let board = BountyBoard {
        id: object::new(ctx),
        next_bounty_id: 1,
        active_bounty_ids: vector::empty(),
        storage_unit_id,
    };
    transfer::share_object(board);
}

/// Set or update board configuration.
public fun set_board_config(
    extension_config: &mut ExtensionConfig,
    admin_cap: &AdminCap,
    max_bounty_duration_ms: u64,
    default_bounty_duration_ms: u64,
    min_killmail_recency_ms: u64,
) {
    extension_config.set_rule<BoardConfigKey, BoardConfig>(
        admin_cap,
        BoardConfigKey {},
        BoardConfig {
            max_bounty_duration_ms,
            default_bounty_duration_ms,
            min_killmail_recency_ms,
        },
    );
}

// === Player Functions ===

/// Post a bounty. The poster must have reward items in their owned inventory at this SSU.
/// Withdraws reward from poster's owned inventory and escrows in SSU open inventory.
public fun post_bounty<T: key>(
    board: &mut BountyBoard,
    extension_config: &ExtensionConfig,
    storage_unit: &mut StorageUnit,
    character: &Character,
    owner_cap: &OwnerCap<T>,
    target_item_id: u64,
    target_tenant: String,
    reward_type_id: u64,
    reward_quantity: u32,
    duration_ms: u64,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    // Validate
    assert!(character.character_address() == ctx.sender(), ESenderMismatch);
    assert!(object::id(storage_unit) == board.storage_unit_id, EStorageUnitMismatch);
    assert!(extension_config.has_rule<BoardConfigKey>(BoardConfigKey {}), EBoardConfigMissing);

    let board_cfg = extension_config.borrow_rule<BoardConfigKey, BoardConfig>(BoardConfigKey {});
    let actual_duration = if (duration_ms == 0) {
        board_cfg.default_bounty_duration_ms
    } else {
        assert!(duration_ms <= board_cfg.max_bounty_duration_ms, EInvalidDuration);
        duration_ms
    };

    let now = clock.timestamp_ms();
    let expires_at = now + actual_duration;

    // Withdraw reward from poster's owned inventory
    let item = storage_unit.withdraw_by_owner(
        character,
        owner_cap,
        reward_type_id,
        reward_quantity,
        ctx,
    );

    // Escrow into SSU open inventory
    storage_unit.deposit_to_open_inventory(
        character,
        item,
        config::x_auth(),
        ctx,
    );

    // Record bounty
    let bounty_id = board.next_bounty_id;
    board.next_bounty_id = bounty_id + 1;

    let bounty = Bounty {
        id: bounty_id,
        target_item_id,
        target_tenant,
        reward_type_id,
        reward_quantity,
        poster: ctx.sender(),
        poster_character_id: character.id(),
        created_at: now,
        expires_at,
        claimed: false,
        claimed_by: option::none(),
        claimed_killmail_id: option::none(),
    };

    df::add(&mut board.id, BountyKey { id: bounty_id }, bounty);
    board.active_bounty_ids.push_back(bounty_id);

    event::emit(BountyPostedEvent {
        bounty_id,
        board_id: object::id(board),
        target_item_id,
        target_tenant,
        reward_type_id,
        reward_quantity,
        poster: ctx.sender(),
        expires_at,
    });
}

/// Claim a bounty by providing a Killmail proof-of-kill.
/// The hunter (killer) receives the reward in their owned inventory at this SSU.
public fun claim_bounty(
    board: &mut BountyBoard,
    extension_config: &ExtensionConfig,
    storage_unit: &mut StorageUnit,
    hunter_character: &Character,
    killmail: &Killmail,
    bounty_id: u64,
    clock: &Clock,
    ctx: &mut TxContext,
) {
    assert!(object::id(storage_unit) == board.storage_unit_id, EStorageUnitMismatch);
    assert!(df::exists_(&board.id, BountyKey { id: bounty_id }), EBountyNotFound);

    let bounty = df::borrow_mut<BountyKey, Bounty>(&mut board.id, BountyKey { id: bounty_id });

    // Validate bounty state
    assert!(!bounty.claimed, EBountyAlreadyClaimed);
    let now = clock.timestamp_ms();
    assert!(now <= bounty.expires_at, EBountyExpired);

    // Validate killmail victim matches target (compare fields individually)
    let victim = killmail.victim_id();
    assert!(
        in_game_id::item_id(&victim) == bounty.target_item_id &&
        in_game_id::tenant(&victim) == bounty.target_tenant,
        EKillmailVictimMismatch,
    );

    // Validate hunter is the killer
    assert!(killmail.killer_id() == hunter_character.key(), EKillmailKillerMismatch);

    // Validate killmail recency (if configured)
    if (extension_config.has_rule<BoardConfigKey>(BoardConfigKey {})) {
        let board_cfg = extension_config.borrow_rule<BoardConfigKey, BoardConfig>(BoardConfigKey {});
        if (board_cfg.min_killmail_recency_ms > 0) {
            // kill_timestamp is in seconds, clock is in ms
            let kill_ts_ms = killmail.kill_timestamp() * 1000;
            assert!(now - kill_ts_ms <= board_cfg.min_killmail_recency_ms, EKillmailTooOld);
        };
    };

    // Mark claimed
    let reward_type_id = bounty.reward_type_id;
    let reward_quantity = bounty.reward_quantity;
    bounty.claimed = true;
    bounty.claimed_by = option::some(hunter_character.character_address());
    bounty.claimed_killmail_id = option::some(killmail.id());

    // Remove from active list
    remove_from_active(board, bounty_id);

    // Withdraw reward from escrow (open inventory)
    let reward_item = storage_unit.withdraw_from_open_inventory(
        hunter_character,
        config::x_auth(),
        reward_type_id,
        reward_quantity,
        ctx,
    );

    // Deposit reward to hunter's owned inventory
    storage_unit.deposit_to_owned(
        hunter_character,
        reward_item,
        config::x_auth(),
        ctx,
    );

    event::emit(BountyClaimedEvent {
        bounty_id,
        board_id: object::id(board),
        hunter: hunter_character.character_address(),
        killmail_id: killmail.id(),
        reward_type_id,
        reward_quantity,
    });
}

/// Cancel a bounty and refund the reward to the poster.
/// Only the original poster can cancel. Bounty must not be claimed.
public fun cancel_bounty(
    board: &mut BountyBoard,
    storage_unit: &mut StorageUnit,
    poster_character: &Character,
    bounty_id: u64,
    ctx: &mut TxContext,
) {
    assert!(object::id(storage_unit) == board.storage_unit_id, EStorageUnitMismatch);
    assert!(df::exists_(&board.id, BountyKey { id: bounty_id }), EBountyNotFound);

    let bounty = df::borrow<BountyKey, Bounty>(&board.id, BountyKey { id: bounty_id });

    assert!(!bounty.claimed, EBountyAlreadyClaimed);
    assert!(ctx.sender() == bounty.poster, ENotPoster);

    let reward_type_id = bounty.reward_type_id;
    let reward_quantity = bounty.reward_quantity;

    // Remove bounty
    let _bounty: Bounty = df::remove(&mut board.id, BountyKey { id: bounty_id });
    remove_from_active(board, bounty_id);

    // Withdraw from escrow
    let reward_item = storage_unit.withdraw_from_open_inventory(
        poster_character,
        config::x_auth(),
        reward_type_id,
        reward_quantity,
        ctx,
    );

    // Refund to poster's owned inventory
    storage_unit.deposit_to_owned(
        poster_character,
        reward_item,
        config::x_auth(),
        ctx,
    );

    event::emit(BountyCancelledEvent {
        bounty_id,
        board_id: object::id(board),
        poster: ctx.sender(),
    });
}

// === View Functions ===

public fun bounty_count(board: &BountyBoard): u64 {
    board.active_bounty_ids.length()
}

public fun active_bounty_ids(board: &BountyBoard): &vector<u64> {
    &board.active_bounty_ids
}

public fun storage_unit_id(board: &BountyBoard): ID {
    board.storage_unit_id
}

public fun get_bounty(board: &BountyBoard, bounty_id: u64): &Bounty {
    assert!(df::exists_(&board.id, BountyKey { id: bounty_id }), EBountyNotFound);
    df::borrow(&board.id, BountyKey { id: bounty_id })
}

public fun bounty_target_item_id(bounty: &Bounty): u64 { bounty.target_item_id }
public fun bounty_target_tenant(bounty: &Bounty): String { bounty.target_tenant }
public fun bounty_reward_type_id(bounty: &Bounty): u64 { bounty.reward_type_id }
public fun bounty_reward_quantity(bounty: &Bounty): u32 { bounty.reward_quantity }
public fun bounty_poster(bounty: &Bounty): address { bounty.poster }
public fun bounty_expires_at(bounty: &Bounty): u64 { bounty.expires_at }
public fun bounty_is_claimed(bounty: &Bounty): bool { bounty.claimed }

public fun is_active(bounty: &Bounty, clock: &Clock): bool {
    !bounty.claimed && clock.timestamp_ms() <= bounty.expires_at
}

// === Private Helpers ===

fun remove_from_active(board: &mut BountyBoard, bounty_id: u64) {
    let (found, idx) = board.active_bounty_ids.index_of(&bounty_id);
    if (found) {
        board.active_bounty_ids.swap_remove(idx);
    };
}
