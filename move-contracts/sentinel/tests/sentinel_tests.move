#[test_only]
module sentinel::sentinel_tests;

use sentinel::config::{Self, AdminCap, ExtensionConfig};
use sentinel::threat_registry;
use sentinel::smart_gate;
use std::string;
use sui::clock;
use sui::test_scenario as ts;

const ADMIN: address = @0xAD;

#[test]
fun test_create_registry() {
    let mut scenario = ts::begin(ADMIN);

    // Deploy config (creates AdminCap + ExtensionConfig)
    {
        let ctx = ts::ctx(&mut scenario);
        config::init_for_testing(ctx);
    };

    // Create registry
    ts::next_tx(&mut scenario, ADMIN);
    {
        let admin_cap = ts::take_from_sender<AdminCap>(&scenario);
        let clock = clock::create_for_testing(ts::ctx(&mut scenario));

        threat_registry::create_registry(&admin_cap, &clock, ts::ctx(&mut scenario));

        ts::return_to_sender(&scenario, admin_cap);
        clock::destroy_for_testing(clock);
    };

    // Verify registry exists and is empty
    ts::next_tx(&mut scenario, ADMIN);
    {
        let registry = ts::take_shared<threat_registry::ThreatRegistry>(&scenario);
        assert!(threat_registry::entry_count(&registry) == 0);
        ts::return_shared(registry);
    };

    ts::end(scenario);
}

#[test]
fun test_batch_update_single() {
    let mut scenario = ts::begin(ADMIN);

    // Setup
    {
        config::init_for_testing(ts::ctx(&mut scenario));
    };
    ts::next_tx(&mut scenario, ADMIN);
    {
        let admin_cap = ts::take_from_sender<AdminCap>(&scenario);
        let clock = clock::create_for_testing(ts::ctx(&mut scenario));
        threat_registry::create_registry(&admin_cap, &clock, ts::ctx(&mut scenario));
        ts::return_to_sender(&scenario, admin_cap);
        clock::destroy_for_testing(clock);
    };

    // Batch update with one entry
    ts::next_tx(&mut scenario, ADMIN);
    {
        let admin_cap = ts::take_from_sender<AdminCap>(&scenario);
        let mut registry = ts::take_shared<threat_registry::ThreatRegistry>(&scenario);
        let clock = clock::create_for_testing(ts::ctx(&mut scenario));

        threat_registry::batch_update(
            &mut registry,
            &admin_cap,
            vector[12345],
            vector[7500],
            vector[100],
            vector[10],
            vector[3],
            vector[1000000],
            vector[string::utf8(b"J-1042")],
            &clock,
        );

        assert!(threat_registry::entry_count(&registry) == 1);
        assert!(threat_registry::has_entry(&registry, 12345));
        assert!(threat_registry::get_threat_score(&registry, 12345) == 7500);

        let entry = threat_registry::get_entry(&registry, 12345);
        assert!(threat_registry::entry_kill_count(entry) == 100);
        assert!(threat_registry::entry_death_count(entry) == 10);
        assert!(threat_registry::entry_bounty_count(entry) == 3);

        ts::return_shared(registry);
        ts::return_to_sender(&scenario, admin_cap);
        clock::destroy_for_testing(clock);
    };

    ts::end(scenario);
}

#[test]
fun test_batch_update_multiple() {
    let mut scenario = ts::begin(ADMIN);

    {
        config::init_for_testing(ts::ctx(&mut scenario));
    };
    ts::next_tx(&mut scenario, ADMIN);
    {
        let admin_cap = ts::take_from_sender<AdminCap>(&scenario);
        let clock = clock::create_for_testing(ts::ctx(&mut scenario));
        threat_registry::create_registry(&admin_cap, &clock, ts::ctx(&mut scenario));
        ts::return_to_sender(&scenario, admin_cap);
        clock::destroy_for_testing(clock);
    };

    // Update 3 entries
    ts::next_tx(&mut scenario, ADMIN);
    {
        let admin_cap = ts::take_from_sender<AdminCap>(&scenario);
        let mut registry = ts::take_shared<threat_registry::ThreatRegistry>(&scenario);
        let clock = clock::create_for_testing(ts::ctx(&mut scenario));

        threat_registry::batch_update(
            &mut registry,
            &admin_cap,
            vector[111, 222, 333],
            vector[2000, 5000, 9000],
            vector[10, 50, 200],
            vector[5, 20, 8],
            vector[0, 1, 4],
            vector[0, 0, 0],
            vector[string::utf8(b"A"), string::utf8(b"B"), string::utf8(b"C")],
            &clock,
        );

        assert!(threat_registry::entry_count(&registry) == 3);
        assert!(threat_registry::get_threat_score(&registry, 111) == 2000);
        assert!(threat_registry::get_threat_score(&registry, 222) == 5000);
        assert!(threat_registry::get_threat_score(&registry, 333) == 9000);

        ts::return_shared(registry);
        ts::return_to_sender(&scenario, admin_cap);
        clock::destroy_for_testing(clock);
    };

    ts::end(scenario);
}

#[test]
fun test_batch_update_overwrites() {
    let mut scenario = ts::begin(ADMIN);

    {
        config::init_for_testing(ts::ctx(&mut scenario));
    };
    ts::next_tx(&mut scenario, ADMIN);
    {
        let admin_cap = ts::take_from_sender<AdminCap>(&scenario);
        let clock = clock::create_for_testing(ts::ctx(&mut scenario));
        threat_registry::create_registry(&admin_cap, &clock, ts::ctx(&mut scenario));
        ts::return_to_sender(&scenario, admin_cap);
        clock::destroy_for_testing(clock);
    };

    // First update
    ts::next_tx(&mut scenario, ADMIN);
    {
        let admin_cap = ts::take_from_sender<AdminCap>(&scenario);
        let mut registry = ts::take_shared<threat_registry::ThreatRegistry>(&scenario);
        let clock = clock::create_for_testing(ts::ctx(&mut scenario));

        threat_registry::batch_update(
            &mut registry,
            &admin_cap,
            vector[42],
            vector[3000],
            vector[20],
            vector[5],
            vector[1],
            vector[0],
            vector[string::utf8(b"X")],
            &clock,
        );

        assert!(threat_registry::get_threat_score(&registry, 42) == 3000);
        assert!(threat_registry::entry_count(&registry) == 1);

        ts::return_shared(registry);
        ts::return_to_sender(&scenario, admin_cap);
        clock::destroy_for_testing(clock);
    };

    // Overwrite same character with new score
    ts::next_tx(&mut scenario, ADMIN);
    {
        let admin_cap = ts::take_from_sender<AdminCap>(&scenario);
        let mut registry = ts::take_shared<threat_registry::ThreatRegistry>(&scenario);
        let clock = clock::create_for_testing(ts::ctx(&mut scenario));

        threat_registry::batch_update(
            &mut registry,
            &admin_cap,
            vector[42],
            vector[8000],
            vector[50],
            vector[10],
            vector[3],
            vector[0],
            vector[string::utf8(b"Y")],
            &clock,
        );

        assert!(threat_registry::get_threat_score(&registry, 42) == 8000);
        // Count should still be 1, not 2
        assert!(threat_registry::entry_count(&registry) == 1);

        let entry = threat_registry::get_entry(&registry, 42);
        assert!(threat_registry::entry_kill_count(entry) == 50);
        assert!(threat_registry::entry_last_seen_system(entry) == string::utf8(b"Y"));

        ts::return_shared(registry);
        ts::return_to_sender(&scenario, admin_cap);
        clock::destroy_for_testing(clock);
    };

    ts::end(scenario);
}

#[test]
fun test_has_entry_false_for_unknown() {
    let mut scenario = ts::begin(ADMIN);

    {
        config::init_for_testing(ts::ctx(&mut scenario));
    };
    ts::next_tx(&mut scenario, ADMIN);
    {
        let admin_cap = ts::take_from_sender<AdminCap>(&scenario);
        let clock = clock::create_for_testing(ts::ctx(&mut scenario));
        threat_registry::create_registry(&admin_cap, &clock, ts::ctx(&mut scenario));
        ts::return_to_sender(&scenario, admin_cap);
        clock::destroy_for_testing(clock);
    };

    ts::next_tx(&mut scenario, ADMIN);
    {
        let registry = ts::take_shared<threat_registry::ThreatRegistry>(&scenario);
        assert!(!threat_registry::has_entry(&registry, 99999));
        ts::return_shared(registry);
    };

    ts::end(scenario);
}

#[test]
#[expected_failure(abort_code = threat_registry::EScoreOutOfRange)]
fun test_score_out_of_range() {
    let mut scenario = ts::begin(ADMIN);

    {
        config::init_for_testing(ts::ctx(&mut scenario));
    };
    ts::next_tx(&mut scenario, ADMIN);
    {
        let admin_cap = ts::take_from_sender<AdminCap>(&scenario);
        let clock = clock::create_for_testing(ts::ctx(&mut scenario));
        threat_registry::create_registry(&admin_cap, &clock, ts::ctx(&mut scenario));
        ts::return_to_sender(&scenario, admin_cap);
        clock::destroy_for_testing(clock);
    };

    ts::next_tx(&mut scenario, ADMIN);
    {
        let admin_cap = ts::take_from_sender<AdminCap>(&scenario);
        let mut registry = ts::take_shared<threat_registry::ThreatRegistry>(&scenario);
        let clock = clock::create_for_testing(ts::ctx(&mut scenario));

        // Score 10001 should abort
        threat_registry::batch_update(
            &mut registry,
            &admin_cap,
            vector[1],
            vector[10001],
            vector[0],
            vector[0],
            vector[0],
            vector[0],
            vector[string::utf8(b"")],
            &clock,
        );

        ts::return_shared(registry);
        ts::return_to_sender(&scenario, admin_cap);
        clock::destroy_for_testing(clock);
    };

    ts::end(scenario);
}

#[test]
#[expected_failure(abort_code = threat_registry::EBatchLengthMismatch)]
fun test_batch_length_mismatch() {
    let mut scenario = ts::begin(ADMIN);

    {
        config::init_for_testing(ts::ctx(&mut scenario));
    };
    ts::next_tx(&mut scenario, ADMIN);
    {
        let admin_cap = ts::take_from_sender<AdminCap>(&scenario);
        let clock = clock::create_for_testing(ts::ctx(&mut scenario));
        threat_registry::create_registry(&admin_cap, &clock, ts::ctx(&mut scenario));
        ts::return_to_sender(&scenario, admin_cap);
        clock::destroy_for_testing(clock);
    };

    ts::next_tx(&mut scenario, ADMIN);
    {
        let admin_cap = ts::take_from_sender<AdminCap>(&scenario);
        let mut registry = ts::take_shared<threat_registry::ThreatRegistry>(&scenario);
        let clock = clock::create_for_testing(ts::ctx(&mut scenario));

        // Mismatched lengths: 2 IDs but 1 score
        threat_registry::batch_update(
            &mut registry,
            &admin_cap,
            vector[1, 2],
            vector[5000],
            vector[0, 0],
            vector[0, 0],
            vector[0, 0],
            vector[0, 0],
            vector[string::utf8(b""), string::utf8(b"")],
            &clock,
        );

        ts::return_shared(registry);
        ts::return_to_sender(&scenario, admin_cap);
        clock::destroy_for_testing(clock);
    };

    ts::end(scenario);
}

#[test]
fun test_set_gate_threshold() {
    let mut scenario = ts::begin(ADMIN);

    {
        config::init_for_testing(ts::ctx(&mut scenario));
    };

    ts::next_tx(&mut scenario, ADMIN);
    {
        let admin_cap = ts::take_from_sender<AdminCap>(&scenario);
        let mut ext_config = ts::take_shared<ExtensionConfig>(&scenario);

        smart_gate::set_gate_threshold(&mut ext_config, &admin_cap, 5000);
        assert!(smart_gate::get_gate_threshold(&ext_config) == 5000);

        // Update threshold
        smart_gate::set_gate_threshold(&mut ext_config, &admin_cap, 7500);
        assert!(smart_gate::get_gate_threshold(&ext_config) == 7500);

        ts::return_shared(ext_config);
        ts::return_to_sender(&scenario, admin_cap);
    };

    ts::end(scenario);
}
