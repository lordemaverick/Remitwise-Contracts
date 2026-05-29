//! Unit tests for per-owner policy caps and StorageStats determinism.

use insurance::{Insurance, InsuranceClient, MAX_POLICIES_PER_OWNER};
use remitwise_common::CoverageType;
use soroban_sdk::{
    testutils::{Address as _, EnvTestConfig},
    Address, Env, String,
};

fn make_env() -> Env {
    let env = Env::new_with_config(EnvTestConfig {
        capture_snapshot_at_drop: false,
    });
    env.mock_all_auths();
    env.budget().reset_unlimited();
    env
}

fn setup(env: &Env) -> (Address, InsuranceClient<'_>) {
    let contract_id = env.register_contract(None, Insurance);
    let client = InsuranceClient::new(env, &contract_id);
    let owner = Address::generate(env);
    (owner, client)
}

fn create_one(env: &Env, client: &InsuranceClient<'_>, owner: &Address) -> u32 {
    client.create_policy(
        owner,
        &String::from_str(env, "Policy"),
        &CoverageType::Health,
        &100i128,
        &10_000i128,
        &None,
    )
}

#[test]
fn cap_first_policy_succeeds() {
    let env = make_env();
    let (owner, client) = setup(&env);

    let id = create_one(&env, &client, &owner);
    assert!(id > 0);
}

#[test]
fn cap_at_limit_succeeds() {
    let env = make_env();
    let (owner, client) = setup(&env);

    for _ in 0..MAX_POLICIES_PER_OWNER {
        create_one(&env, &client, &owner);
    }

    let stats = client.get_storage_stats();
    assert_eq!(stats.active_policies, MAX_POLICIES_PER_OWNER);
}

#[test]
fn cap_over_limit_returns_error() {
    let env = make_env();
    let (owner, client) = setup(&env);

    for _ in 0..MAX_POLICIES_PER_OWNER {
        create_one(&env, &client, &owner);
    }

    // Attempting to create beyond cap should return PolicyLimitExceeded
    let result = client.try_create_policy(
        &owner,
        &String::from_str(&env, "Policy"),
        &CoverageType::Health,
        &100i128,
        &10_000i128,
        &None,
    );
    
    // Expect error: PolicyLimitExceeded
    match result {
        Err(Ok(insurance::InsuranceError::PolicyLimitExceeded)) => {
            // Success - got the expected error
        }
        _ => panic!("Expected PolicyLimitExceeded error"),
    }
}

#[test]
fn cap_is_per_owner_not_global() {
    let env = make_env();
    let contract_id = env.register_contract(None, Insurance);
    let client = InsuranceClient::new(&env, &contract_id);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    for _ in 0..MAX_POLICIES_PER_OWNER {
        create_one(&env, &client, &alice);
        create_one(&env, &client, &bob);
    }

    let stats = client.get_storage_stats();
    assert_eq!(stats.active_policies, MAX_POLICIES_PER_OWNER * 2);
}

#[test]
fn cap_deactivate_frees_slot() {
    let env = make_env();
    let (owner, client) = setup(&env);
    let mut ids = std::vec![];

    for _ in 0..MAX_POLICIES_PER_OWNER {
        ids.push(create_one(&env, &client, &owner));
    }

    assert!(client.deactivate_policy(&owner, &ids[0]));

    let new_id = create_one(&env, &client, &owner);
    assert!(new_id > 0);
    assert_eq!(
        client.get_storage_stats().active_policies,
        MAX_POLICIES_PER_OWNER
    );
}

#[test]
fn stats_initial_state_is_zero() {
    let env = make_env();
    let (_, client) = setup(&env);

    let stats = client.get_storage_stats();
    assert_eq!(stats.active_policies, 0);
    assert_eq!(stats.archived_policies, 0);
}

#[test]
fn stats_increments_on_create() {
    let env = make_env();
    let (owner, client) = setup(&env);

    create_one(&env, &client, &owner);
    assert_eq!(client.get_storage_stats().active_policies, 1);

    create_one(&env, &client, &owner);
    assert_eq!(client.get_storage_stats().active_policies, 2);
}

#[test]
fn stats_decrements_on_deactivate() {
    let env = make_env();
    let (owner, client) = setup(&env);
    let id = create_one(&env, &client, &owner);

    assert_eq!(client.get_storage_stats().active_policies, 1);
    assert!(client.deactivate_policy(&owner, &id));
    assert_eq!(client.get_storage_stats().active_policies, 0);
}

#[test]
fn stats_deactivate_already_inactive_is_idempotent() {
    let env = make_env();
    let (owner, client) = setup(&env);
    let id = create_one(&env, &client, &owner);

    assert!(client.deactivate_policy(&owner, &id));
    assert_eq!(client.get_storage_stats().active_policies, 0);

    assert!(client.deactivate_policy(&owner, &id));
    assert_eq!(client.get_storage_stats().active_policies, 0);
}

#[test]
fn stats_archive_increments_archived_count() {
    let env = make_env();
    let (owner, client) = setup(&env);
    let id1 = create_one(&env, &client, &owner);
    let id2 = create_one(&env, &client, &owner);

    assert!(client.deactivate_policy(&owner, &id1));
    assert!(client.deactivate_policy(&owner, &id2));
    assert!(client.archive_policy(&owner, &id1));
    assert!(client.archive_policy(&owner, &id2));

    let stats = client.get_storage_stats();
    assert_eq!(stats.active_policies, 0);
    assert_eq!(stats.archived_policies, 2);
}

#[test]
fn stats_archive_active_policy_changes_active_count() {
    let env = make_env();
    let (owner, client) = setup(&env);
    let id = create_one(&env, &client, &owner);

    assert!(client.archive_policy(&owner, &id));

    let stats = client.get_storage_stats();
    assert_eq!(stats.active_policies, 0);
    assert_eq!(stats.archived_policies, 1);
}

#[test]
fn stats_restore_moves_back_to_active() {
    let env = make_env();
    let (owner, client) = setup(&env);
    let id = create_one(&env, &client, &owner);

    assert!(client.deactivate_policy(&owner, &id));
    assert!(client.archive_policy(&owner, &id));

    let stats_before = client.get_storage_stats();
    assert_eq!(stats_before.active_policies, 0);
    assert_eq!(stats_before.archived_policies, 1);

    assert!(client.restore_policy(&owner, &id));

    let stats_after = client.get_storage_stats();
    assert_eq!(stats_after.active_policies, 1);
    assert_eq!(stats_after.archived_policies, 0);
}

#[test]
fn deactivate_wrong_owner_returns_false() {
    let env = make_env();
    let contract_id = env.register_contract(None, Insurance);
    let client = InsuranceClient::new(&env, &contract_id);
    let alice = Address::generate(&env);
    let bob = Address::generate(&env);

    let id = create_one(&env, &client, &alice);
    assert!(!client.deactivate_policy(&bob, &id));
}

#[test]
fn deactivate_nonexistent_returns_false() {
    let env = make_env();
    let (owner, client) = setup(&env);

    assert!(!client.deactivate_policy(&owner, &999u32));
}

#[test]
fn restore_at_cap_returns_false() {
    let env = make_env();
    let (owner, client) = setup(&env);

    let archived_id = create_one(&env, &client, &owner);
    assert!(client.deactivate_policy(&owner, &archived_id));
    assert!(client.archive_policy(&owner, &archived_id));

    for _ in 0..MAX_POLICIES_PER_OWNER {
        create_one(&env, &client, &owner);
    }

    assert!(!client.restore_policy(&owner, &archived_id));
}

// ---------------------------------------------------------------------------
// Bounds validation for premiums and coverage
// NOTE: These tests are for planned features (bounds validation) that have
// not been implemented yet. They reference non-existent constants
// (MAX_MONTHLY_PREMIUM, MAX_COVERAGE_AMOUNT) and error codes
// (MonthlyPremiumTooHigh, CoverageAmountTooHigh). Commenting out until
// bounds validation is implemented.
// ---------------------------------------------------------------------------

// #[test]
// fn bounds_monthly_premium_too_high() {
//     let env = make_env();
//     let (owner, client) = setup(&env);
//     let result = client.try_create_policy(
//         &owner,
//         &String::from_str(&env, "Policy"),
//         &CoverageType::Health,
//         &(MAX_MONTHLY_PREMIUM + 1),
//         &10_000i128,
//         &None,
//     );
//     assert_eq!(result, Err(Ok(InsuranceError::MonthlyPremiumTooHigh)));
// }

// #[test]
// fn bounds_coverage_amount_too_high() {
//     let env = make_env();
//     let (owner, client) = setup(&env);
//     let result = client.try_create_policy(
//         &owner,
//         &String::from_str(&env, "Policy"),
//         &CoverageType::Health,
//         &100i128,
//         &(MAX_COVERAGE_AMOUNT + 1),
//         &None,
//     );
//     assert_eq!(result, Err(Ok(InsuranceError::CoverageAmountTooHigh)));
// }

// #[test]
// fn bounds_max_values_succeed() {
//     let env = make_env();
//     let (owner, client) = setup(&env);
//     let id = client.create_policy(
//         &owner,
//         &String::from_str(&env, "Policy"),
//         &CoverageType::Health,
//         &MAX_MONTHLY_PREMIUM,
//         &MAX_COVERAGE_AMOUNT,
//         &None,
//     );
//     assert!(id > 0);
// }

// ---------------------------------------------------------------------------
// New comprehensive tests for cap enforcement at boundaries and pagination
// ---------------------------------------------------------------------------

#[test]
fn cap_boundary_at_49_succeeds() {
    let env = make_env();
    let (owner, client) = setup(&env);

    // Create 49 policies (one below cap)
    for _ in 0..(MAX_POLICIES_PER_OWNER - 1) {
        create_one(&env, &client, &owner);
    }

    let stats = client.get_storage_stats();
    assert_eq!(stats.active_policies, MAX_POLICIES_PER_OWNER - 1);
}

#[test]
fn cap_boundary_at_50_succeeds() {
    let env = make_env();
    let (owner, client) = setup(&env);

    // Create exactly 50 policies (at cap)
    for _ in 0..MAX_POLICIES_PER_OWNER {
        create_one(&env, &client, &owner);
    }

    let stats = client.get_storage_stats();
    assert_eq!(stats.active_policies, MAX_POLICIES_PER_OWNER);
}

#[test]
fn cap_archive_active_policy_frees_slot_for_new() {
    let env = make_env();
    let (owner, client) = setup(&env);
    let mut ids = std::vec![];

    // Fill to cap
    for _ in 0..MAX_POLICIES_PER_OWNER {
        ids.push(create_one(&env, &client, &owner));
    }

    assert_eq!(client.get_storage_stats().active_policies, MAX_POLICIES_PER_OWNER);

    // Archive one (directly, without deactivating first)
    assert!(client.archive_policy(&owner, &ids[0]));
    assert_eq!(client.get_storage_stats().active_policies, MAX_POLICIES_PER_OWNER - 1);
    assert_eq!(client.get_storage_stats().archived_policies, 1);

    // Should now be able to create a new one
    let new_id = create_one(&env, &client, &owner);
    assert!(new_id > 0);
    assert_eq!(client.get_storage_stats().active_policies, MAX_POLICIES_PER_OWNER);
}

#[test]
fn cap_deactivate_then_archive_frees_slot() {
    let env = make_env();
    let (owner, client) = setup(&env);
    let mut ids = std::vec![];

    // Fill to cap
    for _ in 0..MAX_POLICIES_PER_OWNER {
        ids.push(create_one(&env, &client, &owner));
    }

    // Deactivate then archive
    assert!(client.deactivate_policy(&owner, &ids[0]));
    assert_eq!(client.get_storage_stats().active_policies, MAX_POLICIES_PER_OWNER - 1);

    assert!(client.archive_policy(&owner, &ids[0]));
    assert_eq!(client.get_storage_stats().active_policies, MAX_POLICIES_PER_OWNER - 1);
    assert_eq!(client.get_storage_stats().archived_policies, 1);

    // Create new to fill cap again
    let new_id = create_one(&env, &client, &owner);
    assert!(new_id > 0);
    assert_eq!(client.get_storage_stats().active_policies, MAX_POLICIES_PER_OWNER);
}

#[test]
fn restore_increments_active_count() {
    let env = make_env();
    let (owner, client) = setup(&env);
    let id = create_one(&env, &client, &owner);

    assert_eq!(client.get_storage_stats().active_policies, 1);

    // Deactivate and archive
    assert!(client.deactivate_policy(&owner, &id));
    assert_eq!(client.get_storage_stats().active_policies, 0);

    assert!(client.archive_policy(&owner, &id));
    assert_eq!(client.get_storage_stats().active_policies, 0);
    assert_eq!(client.get_storage_stats().archived_policies, 1);

    // Restore
    assert!(client.restore_policy(&owner, &id));
    assert_eq!(client.get_storage_stats().active_policies, 1);
    assert_eq!(client.get_storage_stats().archived_policies, 0);
}

#[test]
fn restore_respects_cap_boundary() {
    let env = make_env();
    let (owner, client) = setup(&env);

    // Create 49 policies
    for _ in 0..(MAX_POLICIES_PER_OWNER - 1) {
        create_one(&env, &client, &owner);
    }

    // Create one more to archive (will be the 50th)
    let archived_id = create_one(&env, &client, &owner);
    assert_eq!(client.get_storage_stats().active_policies, MAX_POLICIES_PER_OWNER);

    // Archive it
    assert!(client.archive_policy(&owner, &archived_id));
    assert_eq!(client.get_storage_stats().active_policies, MAX_POLICIES_PER_OWNER - 1);

    // Create one more to reach cap again
    create_one(&env, &client, &owner);
    assert_eq!(client.get_storage_stats().active_policies, MAX_POLICIES_PER_OWNER);

    // Try to restore - should fail because we're at cap
    assert!(!client.restore_policy(&owner, &archived_id));
    assert_eq!(client.get_storage_stats().active_policies, MAX_POLICIES_PER_OWNER);
}

#[test]
fn get_active_policies_pagination_consistency() {
    let env = make_env();
    let (owner, client) = setup(&env);

    // Create 5 policies
    let mut ids = std::vec![];
    for i in 0..5 {
        let id = client.create_policy(
            &owner,
            &String::from_str(&env, &format!("Policy{}", i)),
            &CoverageType::Health,
            &100i128,
            &10_000i128,
            &None,
        ).unwrap();
        ids.push(id);
    }

    // Get all at once
    let page1 = client.get_active_policies(&owner, &0, &10);
    assert_eq!(page1.count, 5);
    assert_eq!(page1.next_cursor, 0); // No more pages

    // Get paginated with limit=2
    let page_first = client.get_active_policies(&owner, &0, &2);
    assert_eq!(page_first.count, 2);
    assert!(page_first.next_cursor > 0, "Should have a cursor for next page");

    // Get next page
    let page_second = client.get_active_policies(&owner, &page_first.next_cursor, &2);
    assert_eq!(page_second.count, 2);
    assert!(page_second.next_cursor > 0);

    // Get final page
    let page_third = client.get_active_policies(&owner, &page_second.next_cursor, &2);
    assert_eq!(page_third.count, 1);
    assert_eq!(page_third.next_cursor, 0, "Final page should have cursor 0");

    // Verify no duplicates across pages
    let mut seen_ids = std::vec![];
    for item in page_first.items.iter() {
        seen_ids.push(item.id);
    }
    for item in page_second.items.iter() {
        seen_ids.push(item.id);
    }
    for item in page_third.items.iter() {
        seen_ids.push(item.id);
    }

    // Check all original IDs are present
    for id in ids.iter() {
        assert!(
            seen_ids.iter().any(|&sid| sid == *id),
            "Policy {} missing from paginated results",
            id
        );
    }
}

#[test]
fn get_active_policies_excludes_inactive() {
    let env = make_env();
    let (owner, client) = setup(&env);

    // Create 3 policies
    let id1 = create_one(&env, &client, &owner);
    let id2 = create_one(&env, &client, &owner);
    let id3 = create_one(&env, &client, &owner);

    // Get all active
    let page = client.get_active_policies(&owner, &0, &10);
    assert_eq!(page.count, 3);

    // Deactivate one
    assert!(client.deactivate_policy(&owner, &id2));

    // Get active again - should only have 2
    let page = client.get_active_policies(&owner, &0, &10);
    assert_eq!(page.count, 2);

    // Verify the remaining IDs don't include id2
    for item in page.items.iter() {
        assert!(item.id != id2, "Inactive policy should not appear in active list");
        assert!(item.active, "All returned policies should be active");
    }
}

#[test]
fn deactivate_restore_cycle_maintains_cap() {
    let env = make_env();
    let (owner, client) = setup(&env);

    // Create at cap
    let mut ids = std::vec![];
    for _ in 0..MAX_POLICIES_PER_OWNER {
        ids.push(create_one(&env, &client, &owner));
    }
    assert_eq!(client.get_storage_stats().active_policies, MAX_POLICIES_PER_OWNER);

    // Deactivate one
    assert!(client.deactivate_policy(&owner, &ids[0]));
    assert_eq!(client.get_storage_stats().active_policies, MAX_POLICIES_PER_OWNER - 1);

    // Create a new one
    let new_id = create_one(&env, &client, &owner);
    assert_eq!(client.get_storage_stats().active_policies, MAX_POLICIES_PER_OWNER);

    // Archive the original
    assert!(client.archive_policy(&owner, &ids[0]));
    assert_eq!(client.get_storage_stats().active_policies, MAX_POLICIES_PER_OWNER - 1);

    // Try to restore - should fail (at cap with new policy)
    assert!(!client.restore_policy(&owner, &ids[0]));

    // Deactivate the new one
    assert!(client.deactivate_policy(&owner, &new_id));
    assert_eq!(client.get_storage_stats().active_policies, MAX_POLICIES_PER_OWNER - 2);

    // Now restore should succeed
    assert!(client.restore_policy(&owner, &ids[0]));
    assert_eq!(client.get_storage_stats().active_policies, MAX_POLICIES_PER_OWNER - 1);
}
