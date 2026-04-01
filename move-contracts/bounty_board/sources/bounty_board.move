/// On-chain Bounty Board for EVE Frontier.
///
/// Players post bounties on targets by locking SUI coins as the reward.
/// Hunters claim bounties by presenting a Killmail as proof-of-kill.
/// The contract validates the killmail and transfers the escrowed SUI
/// directly to the hunter's wallet — no SSU or in-game travel required.
///
/// Coin flow:
///   Post:   Coin<SUI> → Balance<SUI> escrowed inside Bounty
///   Claim:  escrowed balance → Coin<SUI> transferred to hunter's address
///   Cancel: escrowed balance → Coin<SUI> refunded to poster's address
#[allow(unused_const)]
module bounty_board::bounty_board {
    use bounty_board::config::{AdminCap, ExtensionConfig};
    use std::string::String;
    use sui::{
        balance::{Self, Balance},
        clock::Clock,
        coin::{Self, Coin},
        dynamic_field as df,
        event,
        sui::SUI
    };
    use world::{character::Character, in_game_id, killmail::Killmail};

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
    const EBountyNotExpired: vector<u8> = b"Bounty has not expired yet";
    #[error(code = 9)]
    const EInvalidDuration: vector<u8> = b"Duration exceeds maximum allowed";
    #[error(code = 10)]
    const ESenderMismatch: vector<u8> = b"Transaction sender does not match character address";
    #[error(code = 11)]
    const EBountyNotActive: vector<u8> = b"Cannot add to an expired or claimed bounty";
    #[error(code = 12)]
    const ECannotCancelStackedBounty: vector<u8> =
        b"Cannot cancel a bounty with multiple contributors";
    #[error(code = 13)]
    const EContributionNotFound: vector<u8> = b"No contribution found for this address";
    #[error(code = 14)]
    const EZeroReward: vector<u8> = b"Reward must be greater than zero";

    // === Structs ===

    /// Shared object that tracks all bounties.
    public struct BountyBoard has key {
        id: UID,
        next_bounty_id: u64,
        active_bounty_ids: vector<u64>,
    }

    public struct Contribution has copy, drop, store {
        contributor: address,
        contributor_character_id: ID,
        amount: u64,
    }

    /// Individual bounty stored as a dynamic field on BountyBoard.
    /// SUI reward is escrowed in `escrow` until claimed or cancelled.
    /// Does not have `drop` because Balance<SUI> is not droppable.
    public struct Bounty has store {
        id: u64,
        target_item_id: u64,
        target_tenant: String,
        /// Total escrowed SUI in MIST (1 SUI = 1_000_000_000 MIST).
        reward_mist: u64,
        poster: address,
        poster_character_id: ID,
        created_at: u64,
        expires_at: u64,
        claimed: bool,
        claimed_by: Option<address>,
        claimed_killmail_id: Option<ID>,
        contributors: vector<Contribution>,
        escrow: Balance<SUI>,
    }

    public struct BountyKey has copy, drop, store { id: u64 }

    // === Config ===

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
        reward_mist: u64,
        poster: address,
        expires_at: u64,
    }

    public struct BountyClaimedEvent has copy, drop {
        bounty_id: u64,
        board_id: ID,
        hunter: address,
        killmail_id: ID,
        reward_mist: u64,
    }

    public struct BountyCancelledEvent has copy, drop {
        bounty_id: u64,
        board_id: ID,
        poster: address,
    }

    public struct BountyStackedEvent has copy, drop {
        bounty_id: u64,
        board_id: ID,
        contributor: address,
        amount_added_mist: u64,
        new_total_mist: u64,
    }

    public struct ContributionWithdrawnEvent has copy, drop {
        bounty_id: u64,
        board_id: ID,
        contributor: address,
        amount_withdrawn_mist: u64,
        remaining_total_mist: u64,
    }

    // === Admin Functions ===

    /// Create a BountyBoard. Call once after deploy.
    public fun create_board(_admin_cap: &AdminCap, ctx: &mut TxContext) {
        let board = BountyBoard {
            id: object::new(ctx),
            next_bounty_id: 1,
            active_bounty_ids: vector::empty(),
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

    /// Post a bounty. The poster locks SUI coins as the reward.
    /// `payment` is the entire reward — split from the poster's coin in the PTB.
    public fun post_bounty(
        board: &mut BountyBoard,
        extension_config: &ExtensionConfig,
        character: &Character,
        payment: Coin<SUI>,
        target_item_id: u64,
        target_tenant: String,
        duration_ms: u64,
        clock: &Clock,
        ctx: &mut TxContext,
    ) {
        assert!(character.character_address() == ctx.sender(), ESenderMismatch);
        assert!(extension_config.has_rule<BoardConfigKey>(BoardConfigKey {}), EBoardConfigMissing);

        let reward_mist = payment.value();
        assert!(reward_mist > 0, EZeroReward);

        let board_cfg = extension_config.borrow_rule<
            BoardConfigKey,
            BoardConfig,
        >(BoardConfigKey {});
        let actual_duration = if (duration_ms == 0) {
            board_cfg.default_bounty_duration_ms
        } else {
            assert!(duration_ms <= board_cfg.max_bounty_duration_ms, EInvalidDuration);
            duration_ms
        };

        let now = clock.timestamp_ms();
        let expires_at = now + actual_duration;
        let bounty_id = board.next_bounty_id;
        board.next_bounty_id = bounty_id + 1;

        let bounty = Bounty {
            id: bounty_id,
            target_item_id,
            target_tenant,
            reward_mist,
            poster: ctx.sender(),
            poster_character_id: character.id(),
            created_at: now,
            expires_at,
            claimed: false,
            claimed_by: option::none(),
            claimed_killmail_id: option::none(),
            contributors: vector[
                Contribution {
                    contributor: ctx.sender(),
                    contributor_character_id: character.id(),
                    amount: reward_mist,
                },
            ],
            escrow: payment.into_balance(),
        };

        df::add(&mut board.id, BountyKey { id: bounty_id }, bounty);
        board.active_bounty_ids.push_back(bounty_id);

        event::emit(BountyPostedEvent {
            bounty_id,
            board_id: object::id(board),
            target_item_id,
            target_tenant,
            reward_mist,
            poster: ctx.sender(),
            expires_at,
        });
    }

    /// Claim a bounty by providing a Killmail proof-of-kill.
    /// Escrowed SUI is transferred directly to the hunter's wallet address.
    public fun claim_bounty(
        board: &mut BountyBoard,
        extension_config: &ExtensionConfig,
        hunter_character: &Character,
        killmail: &Killmail,
        bounty_id: u64,
        clock: &Clock,
        ctx: &mut TxContext,
    ) {
        assert!(df::exists_(&board.id, BountyKey { id: bounty_id }), EBountyNotFound);

        let hunter_addr = hunter_character.character_address();
        let killmail_id = killmail.id();
        let board_id = object::id(board);

        // Validate and extract reward — borrow is dropped at end of block.
        let reward_balance = {
            let bounty = df::borrow_mut<BountyKey, Bounty>(
                &mut board.id,
                BountyKey { id: bounty_id },
            );

            assert!(!bounty.claimed, EBountyAlreadyClaimed);
            let now = clock.timestamp_ms();
            assert!(now <= bounty.expires_at, EBountyExpired);

            let victim = killmail.victim_id();
            assert!(
                in_game_id::item_id(&victim) == bounty.target_item_id &&
                in_game_id::tenant(&victim) == bounty.target_tenant,
                EKillmailVictimMismatch,
            );
            assert!(killmail.killer_id() == hunter_character.key(), EKillmailKillerMismatch);

            if (extension_config.has_rule<BoardConfigKey>(BoardConfigKey {})) {
                let board_cfg = extension_config.borrow_rule<
                    BoardConfigKey,
                    BoardConfig,
                >(BoardConfigKey {});
                if (board_cfg.min_killmail_recency_ms > 0) {
                    let kill_ts_ms = killmail.kill_timestamp() * 1000;
                    assert!(now - kill_ts_ms <= board_cfg.min_killmail_recency_ms, EKillmailTooOld);
                };
            };

            bounty.claimed = true;
            bounty.claimed_by = option::some(hunter_addr);
            bounty.claimed_killmail_id = option::some(killmail_id);

            let reward_mist = bounty.reward_mist;
            balance::split(&mut bounty.escrow, reward_mist)
        }; // mutable borrow of board.id dropped here

        remove_from_active(board, bounty_id);

        let reward_mist = balance::value(&reward_balance);
        transfer::public_transfer(coin::from_balance(reward_balance, ctx), hunter_addr);

        event::emit(BountyClaimedEvent {
            bounty_id,
            board_id,
            hunter: hunter_addr,
            killmail_id,
            reward_mist,
        });
    }

    /// Cancel a bounty and refund SUI to the poster.
    /// Only the original poster can cancel, and only if not stacked by others.
    public fun cancel_bounty(board: &mut BountyBoard, bounty_id: u64, ctx: &mut TxContext) {
        assert!(df::exists_(&board.id, BountyKey { id: bounty_id }), EBountyNotFound);

        let board_id = object::id(board);

        // Validate — borrow dropped at end of block.
        let poster = {
            let bounty = df::borrow<BountyKey, Bounty>(&board.id, BountyKey { id: bounty_id });
            assert!(!bounty.claimed, EBountyAlreadyClaimed);
            assert!(bounty.contributors.length() == 1, ECannotCancelStackedBounty);
            assert!(ctx.sender() == bounty.poster, ENotPoster);
            bounty.poster
        };

        let Bounty { escrow, .. } = df::remove(&mut board.id, BountyKey { id: bounty_id });
        remove_from_active(board, bounty_id);

        transfer::public_transfer(coin::from_balance(escrow, ctx), poster);

        event::emit(BountyCancelledEvent { bounty_id, board_id, poster });
    }

    /// Add more SUI reward to an existing active bounty.
    public fun add_to_bounty(
        board: &mut BountyBoard,
        character: &Character,
        payment: Coin<SUI>,
        bounty_id: u64,
        clock: &Clock,
        ctx: &mut TxContext,
    ) {
        assert!(character.character_address() == ctx.sender(), ESenderMismatch);
        assert!(df::exists_(&board.id, BountyKey { id: bounty_id }), EBountyNotFound);

        let amount = payment.value();
        assert!(amount > 0, EZeroReward);

        let bounty = df::borrow_mut<BountyKey, Bounty>(&mut board.id, BountyKey { id: bounty_id });
        assert!(!bounty.claimed, EBountyNotActive);
        let now = clock.timestamp_ms();
        assert!(now <= bounty.expires_at, EBountyNotActive);

        balance::join(&mut bounty.escrow, payment.into_balance());
        bounty.reward_mist = bounty.reward_mist + amount;
        bounty
            .contributors
            .push_back(Contribution {
                contributor: ctx.sender(),
                contributor_character_id: character.id(),
                amount,
            });

        let new_total_mist = bounty.reward_mist;
        let board_id = object::id(board);

        event::emit(BountyStackedEvent {
            bounty_id,
            board_id,
            contributor: ctx.sender(),
            amount_added_mist: amount,
            new_total_mist,
        });
    }

    /// Withdraw your stake from a bounty.
    /// If you are the last contributor, the bounty is deleted entirely.
    #[allow(lint(self_transfer))]
    public fun withdraw_my_contribution(
        board: &mut BountyBoard,
        bounty_id: u64,
        ctx: &mut TxContext,
    ) {
        assert!(df::exists_(&board.id, BountyKey { id: bounty_id }), EBountyNotFound);

        let sender = ctx.sender();
        let board_id = object::id(board);

        let (amount, remaining_total_mist, contributors_empty, refund_balance) = {
            let bounty = df::borrow_mut<BountyKey, Bounty>(
                &mut board.id,
                BountyKey { id: bounty_id },
            );
            assert!(!bounty.claimed, EBountyAlreadyClaimed);

            let len = bounty.contributors.length();
            let mut found_idx = len;
            let mut i = 0;
            while (i < len) {
                if (bounty.contributors[i].contributor == sender) {
                    found_idx = i;
                    break
                };
                i = i + 1;
            };
            assert!(found_idx < len, EContributionNotFound);

            let contribution = bounty.contributors.swap_remove(found_idx);
            let amount = contribution.amount;
            bounty.reward_mist = bounty.reward_mist - amount;
            let remaining_total_mist = bounty.reward_mist;
            let contributors_empty = bounty.contributors.is_empty();
            let refund_balance = balance::split(&mut bounty.escrow, amount);

            (amount, remaining_total_mist, contributors_empty, refund_balance)
        }; // borrow dropped

        transfer::public_transfer(coin::from_balance(refund_balance, ctx), sender);

        if (contributors_empty) {
            let Bounty { escrow, .. } = df::remove(&mut board.id, BountyKey { id: bounty_id });
            balance::destroy_zero(escrow);
            remove_from_active(board, bounty_id);
        };

        event::emit(ContributionWithdrawnEvent {
            bounty_id,
            board_id,
            contributor: sender,
            amount_withdrawn_mist: amount,
            remaining_total_mist,
        });
    }

    // === View Functions ===

    public fun bounty_count(board: &BountyBoard): u64 { board.active_bounty_ids.length() }

    public fun active_bounty_ids(board: &BountyBoard): &vector<u64> { &board.active_bounty_ids }

    public fun get_bounty(board: &BountyBoard, bounty_id: u64): &Bounty {
        assert!(df::exists_(&board.id, BountyKey { id: bounty_id }), EBountyNotFound);
        df::borrow(&board.id, BountyKey { id: bounty_id })
    }

    public fun bounty_target_item_id(bounty: &Bounty): u64 { bounty.target_item_id }

    public fun bounty_target_tenant(bounty: &Bounty): String { bounty.target_tenant }

    public fun bounty_reward_mist(bounty: &Bounty): u64 { bounty.reward_mist }

    public fun bounty_poster(bounty: &Bounty): address { bounty.poster }

    public fun bounty_expires_at(bounty: &Bounty): u64 { bounty.expires_at }

    public fun bounty_is_claimed(bounty: &Bounty): bool { bounty.claimed }

    public fun bounty_contributors(bounty: &Bounty): &vector<Contribution> { &bounty.contributors }

    public fun contribution_contributor(c: &Contribution): address { c.contributor }

    public fun contribution_amount(c: &Contribution): u64 { c.amount }

    public fun is_active(bounty: &Bounty, clock: &Clock): bool {
        !bounty.claimed && clock.timestamp_ms() <= bounty.expires_at
    }

    // === Private Helpers ===

    fun remove_from_active(board: &mut BountyBoard, bounty_id: u64) {
        let (found, idx) = board.active_bounty_ids.index_of(&bounty_id);
        if (found) { board.active_bounty_ids.swap_remove(idx); };
    }
}
