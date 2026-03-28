#[test_only]
module bounty_board::bounty_board_tests {
    use bounty_board::{bounty_board, config::{Self, ExtensionConfig, AdminCap}};
    use std::string::utf8;
    use sui::{clock, test_scenario as ts};
    use world::{
        access::{OwnerCap, AdminACL},
        character::{Self, Character},
        energy::EnergyConfig,
        killmail,
        killmail_registry::KillmailRegistry,
        network_node::{Self, NetworkNode},
        object_registry::ObjectRegistry,
        storage_unit::{Self, StorageUnit},
        test_helpers::{Self, governor, admin, user_a, user_b, tenant}
    };

    // === Test Constants ===
    const CHARACTER_A_ITEM_ID: u32 = 1234;
    const CHARACTER_B_ITEM_ID: u32 = 5678;

    const LOCATION_HASH: vector<u8> =
        x"7a8f3b2e9c4d1a6f5e8b2d9c3f7a1e5b7a8f3b2e9c4d1a6f5e8b2d9c3f7a1e5b";
    const MAX_CAPACITY: u64 = 100000;
    const STORAGE_TYPE_ID: u64 = 5555;
    const STORAGE_ITEM_ID: u64 = 90002;

    // Item constants (reward for bounty)
    const REWARD_TYPE_ID: u64 = 88069;
    const REWARD_ITEM_ID: u64 = 1000004145107;
    const REWARD_VOLUME: u64 = 100;
    const REWARD_QUANTITY: u32 = 10;

    // Network node constants
    const MS_PER_SECOND: u64 = 1000;
    const NWN_TYPE_ID: u64 = 111000;
    const NWN_ITEM_ID: u64 = 5000;
    const FUEL_MAX_CAPACITY: u64 = 1000;
    const FUEL_BURN_RATE_IN_MS: u64 = 3600 * MS_PER_SECOND;
    const MAX_PRODUCTION: u64 = 100;
    const FUEL_TYPE_ID: u64 = 1;
    const FUEL_VOLUME: u64 = 10;

    // Killmail constants
    const KILLMAIL_ID: u64 = 1001;
    const VICTIM_GAME_ID: u64 = 9999;
    const SOLAR_SYSTEM_ID: u64 = 300001;
    const KILL_TIMESTAMP: u64 = 1640995200; // seconds
    const LOSS_TYPE_SHIP: u8 = 1;

    // Bounty config
    const MAX_DURATION_MS: u64 = 604800000; // 7 days
    const DEFAULT_DURATION_MS: u64 = 86400000; // 1 day
    const MIN_RECENCY_MS: u64 = 0; // no check

    // === Helper Functions ===

    fun setup_world_and_energy(ts: &mut ts::Scenario) {
        test_helpers::setup_world(ts);
        test_helpers::configure_assembly_energy(ts);
    }

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

    fun create_network_node(ts: &mut ts::Scenario, character_id: ID): ID {
        ts::next_tx(ts, admin());
        let mut registry = ts::take_shared<ObjectRegistry>(ts);
        let character = ts::take_shared_by_id<Character>(ts, character_id);
        let admin_acl = ts::take_shared<AdminACL>(ts);

        let nwn = network_node::anchor(
            &mut registry,
            &character,
            &admin_acl,
            NWN_ITEM_ID,
            NWN_TYPE_ID,
            LOCATION_HASH,
            FUEL_MAX_CAPACITY,
            FUEL_BURN_RATE_IN_MS,
            MAX_PRODUCTION,
            ts.ctx(),
        );
        let nwn_id = object::id(&nwn);
        nwn.share_network_node(&admin_acl, ts.ctx());

        ts::return_shared(character);
        ts::return_shared(admin_acl);
        ts::return_shared(registry);
        nwn_id
    }

    fun create_storage_unit(ts: &mut ts::Scenario, character_id: ID): (ID, ID) {
        let nwn_id = create_network_node(ts, character_id);
        ts::next_tx(ts, admin());
        let mut registry = ts::take_shared<ObjectRegistry>(ts);
        let mut nwn = ts::take_shared_by_id<NetworkNode>(ts, nwn_id);
        let character = ts::take_shared_by_id<Character>(ts, character_id);
        let admin_acl = ts::take_shared<AdminACL>(ts);

        let storage_unit = storage_unit::anchor(
            &mut registry,
            &mut nwn,
            &character,
            &admin_acl,
            STORAGE_ITEM_ID,
            STORAGE_TYPE_ID,
            MAX_CAPACITY,
            LOCATION_HASH,
            ts.ctx(),
        );
        let storage_id = object::id(&storage_unit);
        storage_unit.share_storage_unit(&admin_acl, ts.ctx());

        ts::return_shared(admin_acl);
        ts::return_shared(character);
        ts::return_shared(registry);
        ts::return_shared(nwn);
        (storage_id, nwn_id)
    }

    fun online_storage_unit(
        ts: &mut ts::Scenario,
        user: address,
        character_id: ID,
        storage_id: ID,
        nwn_id: ID,
    ) {
        let clock = clock::create_for_testing(ts.ctx());

        // Fuel and online the NWN
        ts::next_tx(ts, user);
        let mut character = ts::take_shared_by_id<Character>(ts, character_id);
        let (owner_cap, receipt) = character.borrow_owner_cap<NetworkNode>(
            ts::most_recent_receiving_ticket<OwnerCap<NetworkNode>>(&character_id),
            ts.ctx(),
        );
        ts::next_tx(ts, user);
        {
            let mut nwn = ts::take_shared_by_id<NetworkNode>(ts, nwn_id);
            nwn.deposit_fuel_test(&owner_cap, FUEL_TYPE_ID, FUEL_VOLUME, 10, &clock);
            ts::return_shared(nwn);
        };
        ts::next_tx(ts, user);
        {
            let mut nwn = ts::take_shared_by_id<NetworkNode>(ts, nwn_id);
            nwn.online(&owner_cap, &clock);
            ts::return_shared(nwn);
        };
        character.return_owner_cap(owner_cap, receipt);

        // Online the storage unit
        ts::next_tx(ts, user);
        {
            let mut storage_unit = ts::take_shared_by_id<StorageUnit>(ts, storage_id);
            let mut nwn = ts::take_shared_by_id<NetworkNode>(ts, nwn_id);
            let energy_config = ts::take_shared<EnergyConfig>(ts);
            let (owner_cap, receipt) = character.borrow_owner_cap<StorageUnit>(
                ts::most_recent_receiving_ticket<OwnerCap<StorageUnit>>(&character_id),
                ts.ctx(),
            );
            storage_unit.online(&mut nwn, &energy_config, &owner_cap);
            character.return_owner_cap(owner_cap, receipt);
            ts::return_shared(storage_unit);
            ts::return_shared(nwn);
            ts::return_shared(energy_config);
        };

        ts::return_shared(character);
        clock.destroy_for_testing();
    }

    fun mint_reward_items(ts: &mut ts::Scenario, storage_id: ID, character_id: ID, user: address) {
        ts::next_tx(ts, user);
        let mut character = ts::take_shared_by_id<Character>(ts, character_id);
        let (owner_cap, receipt) = character.borrow_owner_cap<Character>(
            ts::most_recent_receiving_ticket<OwnerCap<Character>>(&character_id),
            ts.ctx(),
        );
        let mut storage_unit = ts::take_shared_by_id<StorageUnit>(ts, storage_id);
        storage_unit.game_item_to_chain_inventory_test<Character>(
            &character,
            &owner_cap,
            REWARD_ITEM_ID,
            REWARD_TYPE_ID,
            REWARD_VOLUME,
            REWARD_QUANTITY,
            ts.ctx(),
        );
        character.return_owner_cap(owner_cap, receipt);
        ts::return_shared(character);
        ts::return_shared(storage_unit);
    }

    fun setup_bounty_board_config(ts: &mut ts::Scenario): (ID, ID) {
        // Init bounty_board package (simulated)
        ts::next_tx(ts, admin());
        {
            config::init_for_testing(ts.ctx());
        };

        // Get the created objects
        ts::next_tx(ts, admin());
        let admin_cap = ts::take_from_sender<AdminCap>(ts);
        let mut extension_config = ts::take_shared<ExtensionConfig>(ts);

        // Set board config
        bounty_board::set_board_config(
            &mut extension_config,
            &admin_cap,
            MAX_DURATION_MS,
            DEFAULT_DURATION_MS,
            MIN_RECENCY_MS,
        );

        let admin_cap_id = object::id(&admin_cap);
        let extension_config_id = object::id(&extension_config);

        ts::return_to_sender(ts, admin_cap);
        ts::return_shared(extension_config);
        (admin_cap_id, extension_config_id)
    }

    fun create_bounty_board(ts: &mut ts::Scenario, storage_id: ID): ID {
        ts::next_tx(ts, admin());
        let admin_cap = ts::take_from_sender<AdminCap>(ts);

        bounty_board::create_board(&admin_cap, storage_id, ts.ctx());

        ts::return_to_sender(ts, admin_cap);

        // Get the board ID
        ts::next_tx(ts, admin());
        let board = ts::take_shared<bounty_board::BountyBoard>(ts);
        let board_id = object::id(&board);
        ts::return_shared(board);
        board_id
    }

    fun authorize_extension(
        ts: &mut ts::Scenario,
        user: address,
        character_id: ID,
        storage_id: ID,
    ) {
        ts::next_tx(ts, user);
        let mut character = ts::take_shared_by_id<Character>(ts, character_id);
        let (owner_cap, receipt) = character.borrow_owner_cap<StorageUnit>(
            ts::most_recent_receiving_ticket<OwnerCap<StorageUnit>>(&character_id),
            ts.ctx(),
        );
        let mut storage_unit = ts::take_shared_by_id<StorageUnit>(ts, storage_id);
        storage_unit.authorize_extension<config::XAuth>(&owner_cap);
        character.return_owner_cap(owner_cap, receipt);
        ts::return_shared(storage_unit);
        ts::return_shared(character);
    }

    fun create_killmail(
        ts: &mut ts::Scenario,
        reporter_id: ID,
        killer_game_id: u64,
        victim_game_id: u64,
    ): ID {
        ts::next_tx(ts, admin());
        let mut registry = ts::take_shared<KillmailRegistry>(ts);
        let admin_acl = ts::take_shared<AdminACL>(ts);
        let reporter = ts::take_shared_by_id<Character>(ts, reporter_id);

        killmail::create_killmail(
            &mut registry,
            &admin_acl,
            KILLMAIL_ID,
            killer_game_id,
            victim_game_id,
            &reporter,
            KILL_TIMESTAMP,
            LOSS_TYPE_SHIP,
            SOLAR_SYSTEM_ID,
            ts::ctx(ts),
        );

        ts::return_shared(reporter);
        ts::return_shared(admin_acl);
        ts::return_shared(registry);

        // Get killmail ID
        ts::next_tx(ts, admin());
        let killmail = ts::take_shared<killmail::Killmail>(ts);
        let killmail_id = object::id(&killmail);
        ts::return_shared(killmail);
        killmail_id
    }

    // === Tests ===

    #[test]
    fun test_create_board() {
        let mut ts = ts::begin(governor());
        setup_world_and_energy(&mut ts);
        let char_id = create_character(&mut ts, user_a(), CHARACTER_A_ITEM_ID);
        let (storage_id, _) = create_storage_unit(&mut ts, char_id);

        setup_bounty_board_config(&mut ts);
        let board_id = create_bounty_board(&mut ts, storage_id);

        ts::next_tx(&mut ts, admin());
        let board = ts::take_shared_by_id<bounty_board::BountyBoard>(&ts, board_id);
        assert!(bounty_board::bounty_count(&board) == 0);
        assert!(bounty_board::storage_unit_id(&board) == storage_id);
        ts::return_shared(board);

        ts::end(ts);
    }

    #[test]
    fun test_post_bounty() {
        let mut ts = ts::begin(governor());
        setup_world_and_energy(&mut ts);

        let poster_char_id = create_character(&mut ts, user_a(), CHARACTER_A_ITEM_ID);
        let (storage_id, nwn_id) = create_storage_unit(&mut ts, poster_char_id);

        test_helpers::configure_fuel(&mut ts);
        online_storage_unit(&mut ts, user_a(), poster_char_id, storage_id, nwn_id);

        // Mint reward items into poster's inventory
        mint_reward_items(&mut ts, storage_id, poster_char_id, user_a());

        // Setup bounty board
        setup_bounty_board_config(&mut ts);
        let board_id = create_bounty_board(&mut ts, storage_id);
        authorize_extension(&mut ts, user_a(), poster_char_id, storage_id);

        // Post bounty
        let clock = clock::create_for_testing(ts.ctx());
        ts::next_tx(&mut ts, user_a());
        {
            let mut board = ts::take_shared_by_id<bounty_board::BountyBoard>(&ts, board_id);
            let extension_config = ts::take_shared<ExtensionConfig>(&ts);
            let mut storage_unit = ts::take_shared_by_id<StorageUnit>(&ts, storage_id);
            let mut character = ts::take_shared_by_id<Character>(&ts, poster_char_id);
            let (owner_cap, receipt) = character.borrow_owner_cap<Character>(
                ts::most_recent_receiving_ticket<OwnerCap<Character>>(&poster_char_id),
                ts.ctx(),
            );

            bounty_board::post_bounty(
                &mut board,
                &extension_config,
                &mut storage_unit,
                &character,
                &owner_cap,
                VICTIM_GAME_ID,
                tenant(),
                REWARD_TYPE_ID,
                REWARD_QUANTITY,
                0, // use default duration
                &clock,
                ts::ctx(&mut ts),
            );

            assert!(bounty_board::bounty_count(&board) == 1);

            character.return_owner_cap(owner_cap, receipt);
            ts::return_shared(character);
            ts::return_shared(storage_unit);
            ts::return_shared(extension_config);
            ts::return_shared(board);
        };

        clock.destroy_for_testing();
        ts::end(ts);
    }

    #[test]
    fun test_cancel_bounty() {
        let mut ts = ts::begin(governor());
        setup_world_and_energy(&mut ts);

        let poster_char_id = create_character(&mut ts, user_a(), CHARACTER_A_ITEM_ID);
        let (storage_id, nwn_id) = create_storage_unit(&mut ts, poster_char_id);

        test_helpers::configure_fuel(&mut ts);
        online_storage_unit(&mut ts, user_a(), poster_char_id, storage_id, nwn_id);
        mint_reward_items(&mut ts, storage_id, poster_char_id, user_a());

        setup_bounty_board_config(&mut ts);
        let board_id = create_bounty_board(&mut ts, storage_id);
        authorize_extension(&mut ts, user_a(), poster_char_id, storage_id);

        // Post bounty
        let clock = clock::create_for_testing(ts.ctx());
        ts::next_tx(&mut ts, user_a());
        {
            let mut board = ts::take_shared_by_id<bounty_board::BountyBoard>(&ts, board_id);
            let extension_config = ts::take_shared<ExtensionConfig>(&ts);
            let mut storage_unit = ts::take_shared_by_id<StorageUnit>(&ts, storage_id);
            let mut character = ts::take_shared_by_id<Character>(&ts, poster_char_id);
            let (owner_cap, receipt) = character.borrow_owner_cap<Character>(
                ts::most_recent_receiving_ticket<OwnerCap<Character>>(&poster_char_id),
                ts.ctx(),
            );

            bounty_board::post_bounty(
                &mut board,
                &extension_config,
                &mut storage_unit,
                &character,
                &owner_cap,
                VICTIM_GAME_ID,
                tenant(),
                REWARD_TYPE_ID,
                REWARD_QUANTITY,
                0,
                &clock,
                ts::ctx(&mut ts),
            );

            character.return_owner_cap(owner_cap, receipt);
            ts::return_shared(character);
            ts::return_shared(storage_unit);
            ts::return_shared(extension_config);
            ts::return_shared(board);
        };

        // Cancel bounty
        ts::next_tx(&mut ts, user_a());
        {
            let mut board = ts::take_shared_by_id<bounty_board::BountyBoard>(&ts, board_id);
            let mut storage_unit = ts::take_shared_by_id<StorageUnit>(&ts, storage_id);
            let character = ts::take_shared_by_id<Character>(&ts, poster_char_id);

            bounty_board::cancel_bounty(
                &mut board,
                &mut storage_unit,
                &character,
                1, // bounty_id
                ts::ctx(&mut ts),
            );

            assert!(bounty_board::bounty_count(&board) == 0);

            ts::return_shared(character);
            ts::return_shared(storage_unit);
            ts::return_shared(board);
        };

        clock.destroy_for_testing();
        ts::end(ts);
    }

    #[test]
    fun test_claim_bounty() {
        let mut ts = ts::begin(governor());
        setup_world_and_energy(&mut ts);

        // Create poster (user_a) and hunter (user_b) characters
        let poster_char_id = create_character(&mut ts, user_a(), CHARACTER_A_ITEM_ID);
        let hunter_char_id = create_character(&mut ts, user_b(), CHARACTER_B_ITEM_ID);

        let (storage_id, nwn_id) = create_storage_unit(&mut ts, poster_char_id);

        test_helpers::configure_fuel(&mut ts);
        online_storage_unit(&mut ts, user_a(), poster_char_id, storage_id, nwn_id);
        mint_reward_items(&mut ts, storage_id, poster_char_id, user_a());

        setup_bounty_board_config(&mut ts);
        let board_id = create_bounty_board(&mut ts, storage_id);
        authorize_extension(&mut ts, user_a(), poster_char_id, storage_id);

        // Create killmail first: hunter (CHARACTER_B_ITEM_ID) killed victim (VICTIM_GAME_ID)
        let killmail_id = create_killmail(
            &mut ts,
            poster_char_id, // reporter
            CHARACTER_B_ITEM_ID as u64, // killer = hunter's game ID
            VICTIM_GAME_ID,
        );

        // Set clock to killmail timestamp (in ms) — post bounty at this time
        let mut clock = clock::create_for_testing(ts.ctx());
        clock.set_for_testing(KILL_TIMESTAMP * 1000);

        // Post bounty targeting VICTIM_GAME_ID
        ts::next_tx(&mut ts, user_a());
        {
            let mut board = ts::take_shared_by_id<bounty_board::BountyBoard>(&ts, board_id);
            let extension_config = ts::take_shared<ExtensionConfig>(&ts);
            let mut storage_unit = ts::take_shared_by_id<StorageUnit>(&ts, storage_id);
            let mut character = ts::take_shared_by_id<Character>(&ts, poster_char_id);
            let (owner_cap, receipt) = character.borrow_owner_cap<Character>(
                ts::most_recent_receiving_ticket<OwnerCap<Character>>(&poster_char_id),
                ts.ctx(),
            );

            bounty_board::post_bounty(
                &mut board,
                &extension_config,
                &mut storage_unit,
                &character,
                &owner_cap,
                VICTIM_GAME_ID,
                tenant(),
                REWARD_TYPE_ID,
                REWARD_QUANTITY,
                0,
                &clock,
                ts::ctx(&mut ts),
            );

            character.return_owner_cap(owner_cap, receipt);
            ts::return_shared(character);
            ts::return_shared(storage_unit);
            ts::return_shared(extension_config);
            ts::return_shared(board);
        };

        // Claim bounty as hunter
        ts::next_tx(&mut ts, user_b());
        {
            let mut board = ts::take_shared_by_id<bounty_board::BountyBoard>(&ts, board_id);
            let extension_config = ts::take_shared<ExtensionConfig>(&ts);
            let mut storage_unit = ts::take_shared_by_id<StorageUnit>(&ts, storage_id);
            let hunter_character = ts::take_shared_by_id<Character>(&ts, hunter_char_id);
            let killmail = ts::take_shared_by_id<killmail::Killmail>(&ts, killmail_id);

            bounty_board::claim_bounty(
                &mut board,
                &extension_config,
                &mut storage_unit,
                &hunter_character,
                &killmail,
                1, // bounty_id
                &clock,
                ts::ctx(&mut ts),
            );

            // Bounty should be claimed (removed from active)
            assert!(bounty_board::bounty_count(&board) == 0);

            // Bounty record should still exist but be marked claimed
            let bounty = bounty_board::get_bounty(&board, 1);
            assert!(bounty_board::bounty_is_claimed(bounty));

            ts::return_shared(killmail);
            ts::return_shared(hunter_character);
            ts::return_shared(storage_unit);
            ts::return_shared(extension_config);
            ts::return_shared(board);
        };

        clock.destroy_for_testing();
        ts::end(ts);
    }

    #[test]
    #[expected_failure(abort_code = bounty_board::EBountyAlreadyClaimed)]
    fun test_double_claim_fails() {
        let mut ts = ts::begin(governor());
        setup_world_and_energy(&mut ts);

        let poster_char_id = create_character(&mut ts, user_a(), CHARACTER_A_ITEM_ID);
        let hunter_char_id = create_character(&mut ts, user_b(), CHARACTER_B_ITEM_ID);
        let (storage_id, nwn_id) = create_storage_unit(&mut ts, poster_char_id);

        test_helpers::configure_fuel(&mut ts);
        online_storage_unit(&mut ts, user_a(), poster_char_id, storage_id, nwn_id);
        mint_reward_items(&mut ts, storage_id, poster_char_id, user_a());

        setup_bounty_board_config(&mut ts);
        let board_id = create_bounty_board(&mut ts, storage_id);
        authorize_extension(&mut ts, user_a(), poster_char_id, storage_id);

        let killmail_id = create_killmail(
            &mut ts,
            poster_char_id,
            CHARACTER_B_ITEM_ID as u64,
            VICTIM_GAME_ID,
        );

        // Set clock to killmail time before posting
        let mut clock = clock::create_for_testing(ts.ctx());
        clock.set_for_testing(KILL_TIMESTAMP * 1000);

        // Post bounty
        ts::next_tx(&mut ts, user_a());
        {
            let mut board = ts::take_shared_by_id<bounty_board::BountyBoard>(&ts, board_id);
            let extension_config = ts::take_shared<ExtensionConfig>(&ts);
            let mut storage_unit = ts::take_shared_by_id<StorageUnit>(&ts, storage_id);
            let mut character = ts::take_shared_by_id<Character>(&ts, poster_char_id);
            let (owner_cap, receipt) = character.borrow_owner_cap<Character>(
                ts::most_recent_receiving_ticket<OwnerCap<Character>>(&poster_char_id),
                ts.ctx(),
            );
            bounty_board::post_bounty(
                &mut board,
                &extension_config,
                &mut storage_unit,
                &character,
                &owner_cap,
                VICTIM_GAME_ID,
                tenant(),
                REWARD_TYPE_ID,
                REWARD_QUANTITY,
                0,
                &clock,
                ts::ctx(&mut ts),
            );
            character.return_owner_cap(owner_cap, receipt);
            ts::return_shared(character);
            ts::return_shared(storage_unit);
            ts::return_shared(extension_config);
            ts::return_shared(board);
        };

        // First claim succeeds
        ts::next_tx(&mut ts, user_b());
        {
            let mut board = ts::take_shared_by_id<bounty_board::BountyBoard>(&ts, board_id);
            let extension_config = ts::take_shared<ExtensionConfig>(&ts);
            let mut storage_unit = ts::take_shared_by_id<StorageUnit>(&ts, storage_id);
            let hunter = ts::take_shared_by_id<Character>(&ts, hunter_char_id);
            let killmail = ts::take_shared_by_id<killmail::Killmail>(&ts, killmail_id);
            bounty_board::claim_bounty(
                &mut board,
                &extension_config,
                &mut storage_unit,
                &hunter,
                &killmail,
                1,
                &clock,
                ts::ctx(&mut ts),
            );
            ts::return_shared(killmail);
            ts::return_shared(hunter);
            ts::return_shared(storage_unit);
            ts::return_shared(extension_config);
            ts::return_shared(board);
        };

        // Second claim should fail
        ts::next_tx(&mut ts, user_b());
        {
            let mut board = ts::take_shared_by_id<bounty_board::BountyBoard>(&ts, board_id);
            let extension_config = ts::take_shared<ExtensionConfig>(&ts);
            let mut storage_unit = ts::take_shared_by_id<StorageUnit>(&ts, storage_id);
            let hunter = ts::take_shared_by_id<Character>(&ts, hunter_char_id);
            let killmail = ts::take_shared_by_id<killmail::Killmail>(&ts, killmail_id);
            bounty_board::claim_bounty(
                &mut board,
                &extension_config,
                &mut storage_unit,
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
        setup_world_and_energy(&mut ts);

        let poster_char_id = create_character(&mut ts, user_a(), CHARACTER_A_ITEM_ID);
        let _other_char_id = create_character(&mut ts, user_b(), CHARACTER_B_ITEM_ID);
        let (storage_id, nwn_id) = create_storage_unit(&mut ts, poster_char_id);

        test_helpers::configure_fuel(&mut ts);
        online_storage_unit(&mut ts, user_a(), poster_char_id, storage_id, nwn_id);
        mint_reward_items(&mut ts, storage_id, poster_char_id, user_a());

        setup_bounty_board_config(&mut ts);
        let board_id = create_bounty_board(&mut ts, storage_id);
        authorize_extension(&mut ts, user_a(), poster_char_id, storage_id);

        // Post bounty as user_a
        let clock = clock::create_for_testing(ts.ctx());
        ts::next_tx(&mut ts, user_a());
        {
            let mut board = ts::take_shared_by_id<bounty_board::BountyBoard>(&ts, board_id);
            let extension_config = ts::take_shared<ExtensionConfig>(&ts);
            let mut storage_unit = ts::take_shared_by_id<StorageUnit>(&ts, storage_id);
            let mut character = ts::take_shared_by_id<Character>(&ts, poster_char_id);
            let (owner_cap, receipt) = character.borrow_owner_cap<Character>(
                ts::most_recent_receiving_ticket<OwnerCap<Character>>(&poster_char_id),
                ts.ctx(),
            );
            bounty_board::post_bounty(
                &mut board,
                &extension_config,
                &mut storage_unit,
                &character,
                &owner_cap,
                VICTIM_GAME_ID,
                tenant(),
                REWARD_TYPE_ID,
                REWARD_QUANTITY,
                0,
                &clock,
                ts::ctx(&mut ts),
            );
            character.return_owner_cap(owner_cap, receipt);
            ts::return_shared(character);
            ts::return_shared(storage_unit);
            ts::return_shared(extension_config);
            ts::return_shared(board);
        };

        // Try cancel as user_b — should fail
        ts::next_tx(&mut ts, user_b());
        {
            let mut board = ts::take_shared_by_id<bounty_board::BountyBoard>(&ts, board_id);
            let mut storage_unit = ts::take_shared_by_id<StorageUnit>(&ts, storage_id);
            let character = ts::take_shared_by_id<Character>(&ts, poster_char_id);
            bounty_board::cancel_bounty(
                &mut board,
                &mut storage_unit,
                &character,
                1,
                ts::ctx(&mut ts),
            );
            abort 999
        }
    }
}
