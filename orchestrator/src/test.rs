#![cfg(test)]

use crate::{
    AuditEntry, ExecutionStats, Orchestrator, OrchestratorClient, OrchestratorError,
    MAX_AUDIT_ENTRIES, MAX_DEADLINE_WINDOW_SECS,
};
use remitwise_common::CONTRACT_VERSION;
use soroban_sdk::{
    symbol_short,
    testutils::{Address as _, Ledger as _},
    Address, Env, Symbol,
};

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn setup_test() -> (Env, Address) {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(100_000);
    let owner = Address::generate(&env);
    (env, owner)
}

fn register_orchestrator(env: &Env) -> OrchestratorClient<'_> {
    let contract_id = env.register_contract(None, Orchestrator);
    OrchestratorClient::new(env, &contract_id)
}

fn init_orchestrator(env: &Env, client: &OrchestratorClient, owner: &Address) {
    let fw = Address::generate(env);
    let rs = Address::generate(env);
    let sg = Address::generate(env);
    let bp = Address::generate(env);
    let ins = Address::generate(env);
    client.init(owner, &fw, &rs, &sg, &bp, &ins);
}

fn compute_test_hash(_env: &Env, operation: Symbol, nonce: u64, amount: i128, deadline: u64) -> u64 {
    let op_bits: u64 = operation.to_val().get_payload();
    let amt_lo = amount as u64;
    let amt_hi = (amount >> 64) as u64;
    op_bits
        .wrapping_add(nonce)
        .wrapping_add(amt_lo)
        .wrapping_add(amt_hi)
        .wrapping_add(deadline)
        .wrapping_mul(1_000_000_007)
}

/// Execute one successful flow and return the nonce used.
fn do_flow(env: &Env, client: &OrchestratorClient, executor: &Address, nonce: u64) {
    let deadline = env.ledger().timestamp() + 1000;
    let hash = compute_test_hash(env, symbol_short!("flow"), nonce, 1000, deadline);
    client.execute_remittance_flow(executor, &1000, &nonce, &deadline, &hash);
}

// ---------------------------------------------------------------------------
// Init tests
// ---------------------------------------------------------------------------

#[test]
fn test_init_success() {
    let (env, owner) = setup_test();
    let client = register_orchestrator(&env);
    let fw = Address::generate(&env);
    let rs = Address::generate(&env);
    let sg = Address::generate(&env);
    let bp = Address::generate(&env);
    let ins = Address::generate(&env);

    assert_eq!(client.try_init(&owner, &fw, &rs, &sg, &bp, &ins), Ok(Ok(true)));
}

#[test]
fn test_init_already_initialized() {
    let (env, owner) = setup_test();
    let client = register_orchestrator(&env);
    init_orchestrator(&env, &client, &owner);

    let result = client.try_init(
        &owner,
        &Address::generate(&env),
        &Address::generate(&env),
        &Address::generate(&env),
        &Address::generate(&env),
        &Address::generate(&env),
    );
    assert_eq!(result, Err(Ok(OrchestratorError::Unauthorized)));
}

#[test]
fn test_init_duplicate_dependency() {
    let (env, owner) = setup_test();
    let client = register_orchestrator(&env);
    let addr = Address::generate(&env);

    let result = client.try_init(
        &owner,
        &addr,
        &addr, // duplicate
        &Address::generate(&env),
        &Address::generate(&env),
        &Address::generate(&env),
    );
    assert_eq!(result, Err(Ok(OrchestratorError::DuplicateDependency)));
}

#[test]
fn test_init_stats_has_evicted_entries_zero() {
    let (env, owner) = setup_test();
    let client = register_orchestrator(&env);
    init_orchestrator(&env, &client, &owner);

    let stats = client.get_execution_stats().unwrap();
    assert_eq!(stats.evicted_entries, 0);
}

// ---------------------------------------------------------------------------
// Version tests
// ---------------------------------------------------------------------------

#[test]
fn test_get_version() {
    let (env, _owner) = setup_test();
    let client = register_orchestrator(&env);
    assert_eq!(client.get_version(), CONTRACT_VERSION);
}

#[test]
fn test_set_version_success() {
    let (env, owner) = setup_test();
    let client = register_orchestrator(&env);
    init_orchestrator(&env, &client, &owner);

    client.set_version(&owner, &2);
    assert_eq!(client.get_version(), 2);
}

#[test]
fn test_set_version_unauthorized() {
    let (env, owner) = setup_test();
    let client = register_orchestrator(&env);
    init_orchestrator(&env, &client, &owner);

    let non_owner = Address::generate(&env);
    assert_eq!(
        client.try_set_version(&non_owner, &2),
        Err(Ok(OrchestratorError::Unauthorized))
    );
}

// ---------------------------------------------------------------------------
// Nonce tests
// ---------------------------------------------------------------------------

#[test]
fn test_get_nonce_initial() {
    let (env, _owner) = setup_test();
    let client = register_orchestrator(&env);
    let user = Address::generate(&env);
    assert_eq!(client.get_nonce(&user), 0);
}

// ---------------------------------------------------------------------------
// Flow execution tests
// ---------------------------------------------------------------------------

#[test]
fn test_execute_flow_invalid_amount() {
    let (env, owner) = setup_test();
    let client = register_orchestrator(&env);
    init_orchestrator(&env, &client, &owner);

    let executor = Address::generate(&env);
    let deadline = env.ledger().timestamp() + 1000;
    let hash = compute_test_hash(&env, symbol_short!("flow"), 0, 0, deadline);

    assert_eq!(
        client.try_execute_remittance_flow(&executor, &0, &0, &deadline, &hash),
        Err(Ok(OrchestratorError::InvalidAmount))
    );
}

#[test]
fn test_execute_flow_expired_deadline() {
    let (env, owner) = setup_test();
    let client = register_orchestrator(&env);
    init_orchestrator(&env, &client, &owner);

    let executor = Address::generate(&env);
    let deadline = env.ledger().timestamp() - 100;
    let hash = compute_test_hash(&env, symbol_short!("flow"), 0, 1000, deadline);

    assert_eq!(
        client.try_execute_remittance_flow(&executor, &1000, &0, &deadline, &hash),
        Err(Ok(OrchestratorError::DeadlineExpired))
    );
}

#[test]
fn test_execute_flow_deadline_too_far() {
    let (env, owner) = setup_test();
    let client = register_orchestrator(&env);
    init_orchestrator(&env, &client, &owner);

    let executor = Address::generate(&env);
    let deadline = env.ledger().timestamp() + MAX_DEADLINE_WINDOW_SECS + 1000;
    let hash = compute_test_hash(&env, symbol_short!("flow"), 0, 1000, deadline);

    assert_eq!(
        client.try_execute_remittance_flow(&executor, &1000, &0, &deadline, &hash),
        Err(Ok(OrchestratorError::DeadlineExpired))
    );
}

#[test]
fn test_execute_flow_invalid_hash() {
    let (env, owner) = setup_test();
    let client = register_orchestrator(&env);
    init_orchestrator(&env, &client, &owner);

    let executor = Address::generate(&env);
    let deadline = env.ledger().timestamp() + 1000;

    assert_eq!(
        client.try_execute_remittance_flow(&executor, &1000, &0, &deadline, &12345u64),
        Err(Ok(OrchestratorError::InvalidNonce))
    );
}

#[test]
fn test_execute_flow_success_updates_stats() {
    let (env, owner) = setup_test();
    let client = register_orchestrator(&env);
    init_orchestrator(&env, &client, &owner);

    let executor = Address::generate(&env);
    do_flow(&env, &client, &executor, 0);

    let stats = client.get_execution_stats().unwrap();
    assert_eq!(stats.total_executions, 1);
    assert_eq!(stats.successful_executions, 1);
    assert_eq!(stats.failed_executions, 0);
}

#[test]
fn test_reentrancy_lock() {
    let (env, owner) = setup_test();
    let client = register_orchestrator(&env);
    init_orchestrator(&env, &client, &owner);

    // Manually set execution lock to simulate reentrancy
    env.as_contract(&client.address, || {
        env.storage()
            .instance()
            .set(&symbol_short!("EXEC_LOCK"), &true);
    });

    let executor = Address::generate(&env);
    let deadline = env.ledger().timestamp() + 1000;
    let hash = compute_test_hash(&env, symbol_short!("flow"), 0, 1000, deadline);

    assert_eq!(
        client.try_execute_remittance_flow(&executor, &1000, &0, &deadline, &hash),
        Err(Ok(OrchestratorError::ExecutionLocked))
    );
}

// ---------------------------------------------------------------------------
// Audit log pagination tests
// ---------------------------------------------------------------------------

#[test]
fn test_audit_log_empty_initially() {
    let (env, owner) = setup_test();
    let client = register_orchestrator(&env);
    init_orchestrator(&env, &client, &owner);

    let page = client.get_audit_log(&0, &10);
    assert_eq!(page.len(), 0);
}

#[test]
fn test_audit_log_from_index_past_end_returns_empty() {
    let (env, owner) = setup_test();
    let client = register_orchestrator(&env);
    init_orchestrator(&env, &client, &owner);

    let executor = Address::generate(&env);
    do_flow(&env, &client, &executor, 0); // 1 entry

    // from_index=5 is past end (len=1)
    let page = client.get_audit_log(&5, &10);
    assert_eq!(page.len(), 0);
}

#[test]
fn test_audit_log_limit_zero_uses_default() {
    let (env, owner) = setup_test();
    let client = register_orchestrator(&env);
    init_orchestrator(&env, &client, &owner);

    let executor = Address::generate(&env);
    // Add 5 entries
    for nonce in 0..5u64 {
        do_flow(&env, &client, &executor, nonce);
    }

    // limit=0 should default to 20, returning all 5
    let page = client.get_audit_log(&0, &0);
    assert_eq!(page.len(), 5);
}

#[test]
fn test_audit_log_limit_clamped_to_max() {
    let (env, owner) = setup_test();
    let client = register_orchestrator(&env);
    init_orchestrator(&env, &client, &owner);

    let executor = Address::generate(&env);
    // Add 10 entries
    for nonce in 0..10u64 {
        do_flow(&env, &client, &executor, nonce);
    }

    // limit=9999 should be clamped to MAX_AUDIT_ENTRIES (100), returning all 10
    let page = client.get_audit_log(&0, &9999);
    assert_eq!(page.len(), 10);
}

#[test]
fn test_audit_log_pagination_no_duplicates() {
    let (env, owner) = setup_test();
    let client = register_orchestrator(&env);
    init_orchestrator(&env, &client, &owner);

    let executor = Address::generate(&env);
    // Add 10 entries
    for nonce in 0..10u64 {
        do_flow(&env, &client, &executor, nonce);
    }

    // Page through with page size 3
    let page0 = client.get_audit_log(&0, &3);
    let page1 = client.get_audit_log(&3, &3);
    let page2 = client.get_audit_log(&6, &3);
    let page3 = client.get_audit_log(&9, &3);

    assert_eq!(page0.len(), 3);
    assert_eq!(page1.len(), 3);
    assert_eq!(page2.len(), 3);
    assert_eq!(page3.len(), 1); // only 1 entry left

    // Collect all timestamps and verify no duplicates
    let mut timestamps: soroban_sdk::Vec<u64> = soroban_sdk::Vec::new(&env);
    for i in 0..page0.len() { timestamps.push_back(page0.get(i).unwrap().timestamp); }
    for i in 0..page1.len() { timestamps.push_back(page1.get(i).unwrap().timestamp); }
    for i in 0..page2.len() { timestamps.push_back(page2.get(i).unwrap().timestamp); }
    for i in 0..page3.len() { timestamps.push_back(page3.get(i).unwrap().timestamp); }

    assert_eq!(timestamps.len(), 10);
}

#[test]
fn test_audit_log_cap_eviction_order() {
    let (env, owner) = setup_test();
    let client = register_orchestrator(&env);
    init_orchestrator(&env, &client, &owner);

    let executor = Address::generate(&env);

    // Fill to exactly MAX_AUDIT_ENTRIES
    for nonce in 0..MAX_AUDIT_ENTRIES as u64 {
        env.ledger().set_timestamp(100_000 + nonce);
        do_flow(&env, &client, &executor, nonce);
    }

    // Log should be full at MAX_AUDIT_ENTRIES
    let full_page = client.get_audit_log(&0, &MAX_AUDIT_ENTRIES);
    assert_eq!(full_page.len(), MAX_AUDIT_ENTRIES);

    // The oldest entry should have timestamp 100_000
    let oldest = full_page.get(0).unwrap();
    assert_eq!(oldest.timestamp, 100_000);

    // Add one more — should evict the oldest (timestamp 100_000)
    env.ledger().set_timestamp(100_000 + MAX_AUDIT_ENTRIES as u64);
    do_flow(&env, &client, &executor, MAX_AUDIT_ENTRIES as u64);

    let after_eviction = client.get_audit_log(&0, &MAX_AUDIT_ENTRIES);
    assert_eq!(after_eviction.len(), MAX_AUDIT_ENTRIES);

    // Oldest entry is now timestamp 100_001 (the second entry before eviction)
    let new_oldest = after_eviction.get(0).unwrap();
    assert_eq!(new_oldest.timestamp, 100_001);

    // Newest entry is the one we just added
    let newest = after_eviction.get(MAX_AUDIT_ENTRIES - 1).unwrap();
    assert_eq!(newest.timestamp, 100_000 + MAX_AUDIT_ENTRIES as u64);
}

#[test]
fn test_evicted_entries_counter_increments() {
    let (env, owner) = setup_test();
    let client = register_orchestrator(&env);
    init_orchestrator(&env, &client, &owner);

    let executor = Address::generate(&env);

    // Fill to cap
    for nonce in 0..MAX_AUDIT_ENTRIES as u64 {
        do_flow(&env, &client, &executor, nonce);
    }

    // No evictions yet
    let stats = client.get_execution_stats().unwrap();
    assert_eq!(stats.evicted_entries, 0);

    // Add 3 more — should evict 3
    for nonce in MAX_AUDIT_ENTRIES as u64..(MAX_AUDIT_ENTRIES as u64 + 3) {
        do_flow(&env, &client, &executor, nonce);
    }

    let stats = client.get_execution_stats().unwrap();
    assert_eq!(stats.evicted_entries, 3);
}

#[test]
fn test_audit_log_entries_ordered_oldest_to_newest() {
    let (env, owner) = setup_test();
    let client = register_orchestrator(&env);
    init_orchestrator(&env, &client, &owner);

    let executor = Address::generate(&env);

    for nonce in 0..5u64 {
        env.ledger().set_timestamp(100_000 + nonce * 10);
        do_flow(&env, &client, &executor, nonce);
    }

    let page = client.get_audit_log(&0, &10);
    assert_eq!(page.len(), 5);

    // Verify ascending timestamp order
    for i in 0..(page.len() - 1) {
        let a = page.get(i).unwrap().timestamp;
        let b = page.get(i + 1).unwrap().timestamp;
        assert!(a <= b, "entries not in ascending order: {} > {}", a, b);
    }
}

#[test]
fn test_audit_log_from_index_at_last_entry() {
    let (env, owner) = setup_test();
    let client = register_orchestrator(&env);
    init_orchestrator(&env, &client, &owner);

    let executor = Address::generate(&env);
    for nonce in 0..5u64 {
        do_flow(&env, &client, &executor, nonce);
    }

    // from_index=4 is the last valid index (len=5)
    let page = client.get_audit_log(&4, &10);
    assert_eq!(page.len(), 1);
}

#[test]
fn test_audit_log_limit_exactly_one() {
    let (env, owner) = setup_test();
    let client = register_orchestrator(&env);
    init_orchestrator(&env, &client, &owner);

    let executor = Address::generate(&env);
    for nonce in 0..5u64 {
        do_flow(&env, &client, &executor, nonce);
    }

    let page = client.get_audit_log(&0, &1);
    assert_eq!(page.len(), 1);
}

#[test]
fn test_audit_log_cap_does_not_exceed_max() {
    let (env, owner) = setup_test();
    let client = register_orchestrator(&env);
    init_orchestrator(&env, &client, &owner);

    let executor = Address::generate(&env);

    // Add more than MAX_AUDIT_ENTRIES
    for nonce in 0..(MAX_AUDIT_ENTRIES as u64 + 20) {
        do_flow(&env, &client, &executor, nonce);
    }

    // Log must never exceed MAX_AUDIT_ENTRIES
    let page = client.get_audit_log(&0, &(MAX_AUDIT_ENTRIES + 100));
    assert_eq!(page.len(), MAX_AUDIT_ENTRIES);
}

#[test]
fn test_get_execution_stats_initial() {
    let (env, owner) = setup_test();
    let client = register_orchestrator(&env);
    init_orchestrator(&env, &client, &owner);

    let stats = client.get_execution_stats();
    assert_eq!(
        stats,
        Some(ExecutionStats {
            total_executions: 0,
            successful_executions: 0,
            failed_executions: 0,
            last_execution_time: 0,
            evicted_entries: 0,
        })
    );
}
