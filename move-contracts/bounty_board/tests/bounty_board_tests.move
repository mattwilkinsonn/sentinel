#[test_only]
module bounty_board::bounty_board_tests {
    use bounty_board::{bounty_board, config::{Self, ExtensionConfig, AdminCap}};
    use std::string::utf8;
    use sui::{clock, coin, sui::SUI, test_scenario as ts};
    use world::{
        access::AdminACL,
        character::{Self, Character},
        killmail,
        killmail_registry::KillmailRegistry,
        object_registry::ObjectRegistry,
        test_helpers::{Self, governor, admin, user_a, user_b, tenant}
    };

    // === Test Constants ===
    const CHARACTER_A_ITEM_ID: u32 = 1234;
    const CHARACTER_B_ITEM_ID: u32 = 5678;

    const VICTIM_GAME_ID: u64 = 9999;
    const SOLAR_SYSTEM_ID: u64 = 300001;
    const KILL_TIMESTAMP: u64 = 1640995200; // seconds
    const LOSS_TYPE_SHIP: u8 = 1;
    const KILLMAIL_ID: u64 = 1001;

    const REWARD_MIST: u64 = 1_000_000_000; // 1 SUI

    const MAX_DURATION_MS: u64 = 604800000; // 7 days
    const DEFAULT_DURATION_MS: u64 = 86400000; // 1 day
    const MIN_RECENCY_MS: u64 = 0;

    // === Helpers ===

    fun create_character(ts: &mut ts::Scenario, user: address, item_id: u32): ID {
        ts::next_tx(ts, admin());
        let admin_acl = ts::take_shared<AdminACL>(ts);
        let mut registry = ts::take_shared<ObjectRegistry>(ts);
        let character = character::create_character(
            &mut registry,
            &admin_acl,
            item_id,
            tenant(),
            100,
            user,
            utf8(b"test_char"),
            ts::ctx(ts),
        );
        let character_id = object::id(&character);
        character.share_character(&admin_acl, ts.ctx());
        ts::return_shared(registry);
        ts::return_shared(admin_acl);
        character_id
    }

    fun setup_bounty_board(ts: &mut ts::Scenario): (ID, ID) {
        ts::next_tx(ts, admin());
        config::init_for_testing(ts.ctx());

        ts::next_tx(ts, admin());
        let admin_cap = ts::take_from_sender<AdminCap>(ts);
        let mut extension_config = ts::take_shared<ExtensionConfig>(ts);

        bounty_board::set_board_config(
            &mut extension_config,
            &admin_cap,
            MAX_DURATION_MS,
            DEFAULT_DURATION_MS,
            MIN_RECENCY_MS,
        );
        bounty_board::create_board(&admin_cap, ts.ctx());

        let extension_config_id = object::id(&extension_config);
        ts::return_to_sender(ts, admin_cap);
        ts::return_shared(extension_config);

        ts::next_tx(ts, admin());
        let board = ts::take_shared<bounty_board::BountyBoard>(ts);
        let board_id = object::id(&board);
        ts::return_shared(board);

        (board_id, extension_config_id)
    }

    fun create_killmail(
        ts: &mut ts::Scenario,
        reporter_id: ID,
        killer_item_id: u64,
        victim_item_id: u64,
    ): ID {
        ts::next_tx(ts, admin());
        let mut registry = ts::take_shared<KillmailRegistry>(ts);
        let admin_acl = ts::take_shared<AdminACL>(ts);
        let reporter = ts::take_shared_by_id<Character>(ts, reporter_id);

        killmail::create_killmail(
            &mut registry,
            &admin_acl,
            KILLMAIL_ID,
            killer_item_id,
            victim_item_id,
            &reporter,
            KILL_TIMESTAMP,
            LOSS_TYPE_SHIP,
            SOLAR_SYSTEM_ID,
            ts::ctx(ts),
        );

        ts::return_shared(reporter);
        ts::return_shared(admin_acl);
        ts::return_shared(registry);

        ts::next_tx(ts, admin());
        let km = ts::take_shared<killmail::Killmail>(ts);
        let km_id = object::id(&km);
        ts::return_shared(km);
        km_id
    }

    // === Tests ===

    #[test]
    fun test_create_board() {
        let mut ts = ts::begin(governor());
        test_helpers::setup_world(&mut ts);

        let (board_id, _) = setup_bounty_board(&mut ts);

        ts::next_tx(&mut ts, admin());
        let board = ts::take_shared_by_id<bounty_board::BountyBoard>(&ts, board_id);
        assert!(bounty_board::bounty_count(&board) == 0);
        ts::return_shared(board);

        ts::end(ts);
    }

    #[test]
    fun test_post_bounty() {
        let mut ts = ts::begin(governor());
        test_helpers::setup_world(&mut ts);

        let poster_char_id = create_character(&mut ts, user_a(), CHARACTER_A_ITEM_ID);
        let (board_id, _) = setup_bounty_board(&mut ts);

        let clock = clock::create_for_testing(ts.ctx());
        ts::next_tx(&mut ts, user_a());
        {
            let mut board = ts::take_shared_by_id<bounty_board::BountyBoard>(&ts, board_id);
            let extension_config = ts::take_shared<ExtensionConfig>(&ts);
            let character = ts::take_shared_by_id<Character>(&ts, poster_char_id);
            let payment = coin::mint_for_testing<SUI>(REWARD_MIST, ts::ctx(&mut ts));

            bounty_board::post_bounty(
                &mut board,
                &extension_config,
                &character,
                payment,
                VICTIM_GAME_ID,
                tenant(),
                0, // default duration
                &clock,
                ts::ctx(&mut ts),
            );

            assert!(bounty_board::bounty_count(&board) == 1);

            let bounty = bounty_board::get_bounty(&board, 1);
            assert!(bounty_board::bounty_reward_mist(bounty) == REWARD_MIST);
            assert!(!bounty_board::bounty_is_claimed(bounty));

            ts::return_shared(character);
            ts::return_shared(extension_config);
            ts::return_shared(board);
        };

        clock.destroy_for_testing();
        ts::end(ts);
    }

    #[test]
    fun test_claim_bounty() {
        let mut ts = ts::begin(governor());
        test_helpers::setup_world(&mut ts);

        let poster_char_id = create_character(&mut ts, user_a(), CHARACTER_A_ITEM_ID);
        let hunter_char_id = create_character(&mut ts, user_b(), CHARACTER_B_ITEM_ID);
        let (board_id, _) = setup_bounty_board(&mut ts);

        let killmail_id = create_killmail(
            &mut ts,
            poster_char_id,
            CHARACTER_B_ITEM_ID as u64,
            VICTIM_GAME_ID,
        );

        let mut clock = clock::create_for_testing(ts.ctx());
        clock.set_for_testing(KILL_TIMESTAMP * 1000);

        // Post bounty
        ts::next_tx(&mut ts, user_a());
        {
            let mut board = ts::take_shared_by_id<bounty_board::BountyBoard>(&ts, board_id);
            let extension_config = ts::take_shared<ExtensionConfig>(&ts);
            let character = ts::take_shared_by_id<Character>(&ts, poster_char_id);
            let payment = coin::mint_for_testing<SUI>(REWARD_MIST, ts::ctx(&mut ts));

            bounty_board::post_bounty(
                &mut board,
                &extension_config,
                &character,
                payment,
                VICTIM_GAME_ID,
                tenant(),
                0,
                &clock,
                ts::ctx(&mut ts),
            );

            ts::return_shared(character);
            ts::return_shared(extension_config);
            ts::return_shared(board);
        };

        // Claim bounty
        ts::next_tx(&mut ts, user_b());
        {
            let mut board = ts::take_shared_by_id<bounty_board::BountyBoard>(&ts, board_id);
            let extension_config = ts::take_shared<ExtensionConfig>(&ts);
            let hunter = ts::take_shared_by_id<Character>(&ts, hunter_char_id);
            let killmail = ts::take_shared_by_id<killmail::Killmail>(&ts, killmail_id);

            bounty_board::claim_bounty(
                &mut board,
                &extension_config,
                &hunter,
                &killmail,
                1,
                &clock,
                ts::ctx(&mut ts),
            );

            assert!(bounty_board::bounty_count(&board) == 0);
            let bounty = bounty_board::get_bounty(&board, 1);
            assert!(bounty_board::bounty_is_claimed(bounty));

            ts::return_shared(killmail);
            ts::return_shared(hunter);
            ts::return_shared(extension_config);
            ts::return_shared(board);
        };

        clock.destroy_for_testing();
        ts::end(ts);
    }

    #[test]
    fun test_cancel_bounty() {
        let mut ts = ts::begin(governor());
        test_helpers::setup_world(&mut ts);

        let poster_char_id = create_character(&mut ts, user_a(), CHARACTER_A_ITEM_ID);
        let (board_id, _) = setup_bounty_board(&mut ts);

        let clock = clock::create_for_testing(ts.ctx());
        ts::next_tx(&mut ts, user_a());
        {
            let mut board = ts::take_shared_by_id<bounty_board::BountyBoard>(&ts, board_id);
            let extension_config = ts::take_shared<ExtensionConfig>(&ts);
            let character = ts::take_shared_by_id<Character>(&ts, poster_char_id);
            let payment = coin::mint_for_testing<SUI>(REWARD_MIST, ts::ctx(&mut ts));

            bounty_board::post_bounty(
                &mut board,
                &extension_config,
                &character,
                payment,
                VICTIM_GAME_ID,
                tenant(),
                0,
                &clock,
                ts::ctx(&mut ts),
            );

            ts::return_shared(character);
            ts::return_shared(extension_config);
            ts::return_shared(board);
        };

        ts::next_tx(&mut ts, user_a());
        {
            let mut board = ts::take_shared_by_id<bounty_board::BountyBoard>(&ts, board_id);
            bounty_board::cancel_bounty(&mut board, 1, ts::ctx(&mut ts));
            assert!(bounty_board::bounty_count(&board) == 0);
            ts::return_shared(board);
        };

        clock.destroy_for_testing();
        ts::end(ts);
    }

    #[test]
    #[expected_failure(abort_code = bounty_board::EBountyAlreadyClaimed)]
    fun test_double_claim_fails() {
        let mut ts = ts::begin(governor());
        test_helpers::setup_world(&mut ts);

        let poster_char_id = create_character(&mut ts, user_a(), CHARACTER_A_ITEM_ID);
        let hunter_char_id = create_character(&mut ts, user_b(), CHARACTER_B_ITEM_ID);
        let (board_id, _) = setup_bounty_board(&mut ts);

        let killmail_id = create_killmail(
            &mut ts,
            poster_char_id,
            CHARACTER_B_ITEM_ID as u64,
            VICTIM_GAME_ID,
        );

        let mut clock = clock::create_for_testing(ts.ctx());
        clock.set_for_testing(KILL_TIMESTAMP * 1000);

        ts::next_tx(&mut ts, user_a());
        {
            let mut board = ts::take_shared_by_id<bounty_board::BountyBoard>(&ts, board_id);
            let extension_config = ts::take_shared<ExtensionConfig>(&ts);
            let character = ts::take_shared_by_id<Character>(&ts, poster_char_id);
            let payment = coin::mint_for_testing<SUI>(REWARD_MIST, ts::ctx(&mut ts));
            bounty_board::post_bounty(
                &mut board,
                &extension_config,
                &character,
                payment,
                VICTIM_GAME_ID,
                tenant(),
                0,
                &clock,
                ts::ctx(&mut ts),
            );
            ts::return_shared(character);
            ts::return_shared(extension_config);
            ts::return_shared(board);
        };

        // First claim
        ts::next_tx(&mut ts, user_b());
        {
            let mut board = ts::take_shared_by_id<bounty_board::BountyBoard>(&ts, board_id);
            let extension_config = ts::take_shared<ExtensionConfig>(&ts);
            let hunter = ts::take_shared_by_id<Character>(&ts, hunter_char_id);
            let killmail = ts::take_shared_by_id<killmail::Killmail>(&ts, killmail_id);
            bounty_board::claim_bounty(
                &mut board,
                &extension_config,
                &hunter,
                &killmail,
                1,
                &clock,
                ts::ctx(&mut ts),
            );
            ts::return_shared(killmail);
            ts::return_shared(hunter);
            ts::return_shared(extension_config);
            ts::return_shared(board);
        };

        // Second claim — should abort
        ts::next_tx(&mut ts, user_b());
        {
            let mut board = ts::take_shared_by_id<bounty_board::BountyBoard>(&ts, board_id);
            let extension_config = ts::take_shared<ExtensionConfig>(&ts);
            let hunter = ts::take_shared_by_id<Character>(&ts, hunter_char_id);
            let killmail = ts::take_shared_by_id<killmail::Killmail>(&ts, killmail_id);
            bounty_board::claim_bounty(
                &mut board,
                &extension_config,
                &hunter,
                &killmail,
                1,
                &clock,
                ts::ctx(&mut ts),
            );
            abort 999
        }
    }

    #[test]
    #[expected_failure(abort_code = bounty_board::ENotPoster)]
    fun test_cancel_by_non_poster_fails() {
        let mut ts = ts::begin(governor());
        test_helpers::setup_world(&mut ts);

        let poster_char_id = create_character(&mut ts, user_a(), CHARACTER_A_ITEM_ID);
        let (board_id, _) = setup_bounty_board(&mut ts);

        let clock = clock::create_for_testing(ts.ctx());
        ts::next_tx(&mut ts, user_a());
        {
            let mut board = ts::take_shared_by_id<bounty_board::BountyBoard>(&ts, board_id);
            let extension_config = ts::take_shared<ExtensionConfig>(&ts);
            let character = ts::take_shared_by_id<Character>(&ts, poster_char_id);
            let payment = coin::mint_for_testing<SUI>(REWARD_MIST, ts::ctx(&mut ts));
            bounty_board::post_bounty(
                &mut board,
                &extension_config,
                &character,
                payment,
                VICTIM_GAME_ID,
                tenant(),
                0,
                &clock,
                ts::ctx(&mut ts),
            );
            ts::return_shared(character);
            ts::return_shared(extension_config);
            ts::return_shared(board);
        };

        // Cancel as user_b — should abort
        ts::next_tx(&mut ts, user_b());
        {
            let mut board = ts::take_shared_by_id<bounty_board::BountyBoard>(&ts, board_id);
            bounty_board::cancel_bounty(&mut board, 1, ts::ctx(&mut ts));
            abort 999
        }
    }
}
