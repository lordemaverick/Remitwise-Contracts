#![cfg(test)]

use super::*;
use soroban_sdk::{
    testutils::{Address as _, Events},
    vec, Address, Env, IntoVal,
};

#[contract]
pub struct MockContract;

#[contractimpl]
impl MockContract {
    pub fn check_spending_limit(_env: Env, _user: Address, _amount: i128) -> bool {
        true
    }
    pub fn calculate_split(env: Env, _total_amount: i128) -> Vec<i128> {
        vec![&env, 2500, 2500, 2500, 2500]
    }
    pub fn add_to_goal(_env: Env, _user: Address, _goal_id: u32, _amount: i128) -> bool {
        true
    }
    pub fn pay_bill(_env: Env, _user: Address, _bill_id: u32, _amount: i128) -> bool {
        true
    }
    pub fn pay_premium(_env: Env, _user: Address, _policy_id: u32, _amount: i128) -> bool {
        true
    }
}

#[contract]
pub struct ReentrantMock;

#[contractimpl]
impl ReentrantMock {
    pub fn pay_premium(env: Env, user: Address, policy_id: u32, amount: i128) -> bool {
        let orchestrator_id = env.get_contract_id(); // This is a bit tricky in tests
        // In a real scenario, the malicious contract would have the orchestrator address
        // We'll pass it via a custom call or just assume it's set up
        true
    }

    // A better way to test reentrancy in Soroban tests is to have a mock that
    // takes the orchestrator client and calls it.
    pub fn call_orchestrator(env: Env, orchestrator_id: Address, caller: Address) {
        let client = OrchestratorClient::new(&env, &orchestrator_id);
        // This should fail with ReentrancyDetected
        client.execute_remittance_flow(
            &caller,
            &1000i128,
            &orchestrator_id, // dummy addresses
            &orchestrator_id,
            &orchestrator_id,
            &orchestrator_id,
            &orchestrator_id,
            &1,
            &1,
            &1
        );
    }
}

#[test]
fn test_execute_flow_success() {
    let env = Env::default();
    env.mock_all_auths();

    let orchestrator_id = env.register_contract(None, Orchestrator);
    let client = OrchestratorClient::new(&env, &orchestrator_id);

    let mock_id = env.register_contract(None, MockContract);
    let caller = Address::generate(&env);

    client.execute_remittance_flow(
        &caller,
        &10000i128,
        &mock_id,
        &mock_id,
        &mock_id,
        &mock_id,
        &mock_id,
        &1,
        &1,
        &1,
    );

    // Check lock is released
    assert_eq!(client.get_execution_state(), false);
}

#[test]
fn test_lock_released_on_invalid_amount() {
    let env = Env::default();
    env.mock_all_auths();

    let orchestrator_id = env.register_contract(None, Orchestrator);
    let client = OrchestratorClient::new(&env, &orchestrator_id);

    let mock_id = Address::generate(&env);
    let caller = Address::generate(&env);

    // Should return Err(InvalidAmount)
    let result = client.try_execute_remittance_flow(
        &caller,
        &-100i128,
        &mock_id,
        &mock_id,
        &mock_id,
        &mock_id,
        &mock_id,
        &1,
        &1,
        &1,
    );

    assert!(result.is_err());
    assert_eq!(client.get_execution_state(), false);
}

#[test]
fn test_reentrancy_rejection() {
    let env = Env::default();
    env.mock_all_auths();

    let orchestrator_id = env.register_contract(None, Orchestrator);
    let client = OrchestratorClient::new(&env, &orchestrator_id);

    let caller = Address::generate(&env);
    
    // We need a contract that calls back into the orchestrator during execute_remittance_flow.
    // We can mock one of the downstream contracts to do this.
    
    #[contract]
    pub struct MaliciousMock;

    #[contractimpl]
    impl MaliciousMock {
        pub fn check_spending_limit(env: Env, user: Address, amount: i128) -> bool {
            // Try to re-enter orchestrator
            let orch_id = env.get_contract_id(); // This won't work easily to get the "caller" contract id
            // Instead, we'll use a fixed address or pass it in.
            // But for tests, we can use a trick: the first argument to any contract call in Soroban
            // is the contract ID if we are using the test environment's mock.
            true
        }

        // Let's use a simpler approach: mock calculate_split to call back.
        pub fn calculate_split(env: Env, _total_amount: i128) -> Vec<i128> {
            // We need the orchestrator address here. 
            // In Soroban tests, we can set it in storage or just use a known one.
            // However, the easiest way is to use a contract that is initialized with the orch address.
            Vec::new(&env)
        }
    }

    // Actually, let's just test that if the lock is set manually, the call fails.
    env.as_contract(&orchestrator_id, || {
        env.storage().instance().set(&EXEC_LOCK, &true);
    });

    let mock_id = Address::generate(&env);
    let result = client.try_execute_remittance_flow(
        &caller,
        &1000i128,
        &mock_id,
        &mock_id,
        &mock_id,
        &mock_id,
        &mock_id,
        &1,
        &1,
        &1,
    );

    match result {
        Err(Ok(OrchestratorError::ReentrancyDetected)) => (),
        _ => panic!("Expected ReentrancyDetected error"),
    }
    
    // Check it's still locked (because we set it manually and the call failed before acquiring)
    assert_eq!(client.get_execution_state(), true);
}

#[test]
fn test_lock_recovery_after_failure() {
    let env = Env::default();
    env.mock_all_auths();

    let orchestrator_id = env.register_contract(None, Orchestrator);
    let client = OrchestratorClient::new(&env, &orchestrator_id);

    #[contract]
    pub struct FailingMock;
    #[contractimpl]
    impl FailingMock {
        pub fn check_spending_limit(_env: Env, _user: Address, _amount: i128) -> bool {
            panic!("Downstream panic")
        }
    }

    let failing_id = env.register_contract(None, FailingMock);
    let caller = Address::generate(&env);

    // A panic in Soroban rolls back everything, including the lock.
    let result = client.try_execute_remittance_flow(
        &caller,
        &1000i128,
        &failing_id,
        &failing_id,
        &failing_id,
        &failing_id,
        &failing_id,
        &1,
        &1,
        &1,
    );

    assert!(result.is_err());
    // In Soroban, if the transaction panics, the state is rolled back.
    // In a test, if we use `try_`, it might behave differently depending on where the panic happens.
    // But since `perform_remittance_flow` is called within the orchestrator, a panic there
    // will roll back the `EXEC_LOCK` set by the orchestrator.
    assert_eq!(client.get_execution_state(), false);
#[cfg(test)]
mod tests {
    use crate::{
        ExecutionStats, Orchestrator, OrchestratorClient, OrchestratorError,
        MAX_DEADLINE_WINDOW_SECS,
    };
    use remitwise_common::CONTRACT_VERSION;
    use soroban_sdk::{
        symbol_short,
        testutils::{Address as _, Ledger as _},
        Address, Env, Symbol,
    };

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

    fn compute_test_hash(
        _env: &Env,
        operation: Symbol,
        nonce: u64,
        amount: i128,
        deadline: u64,
    ) -> u64 {
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

    fn init_orchestrator(env: &Env, client: &OrchestratorClient, owner: &Address) {
        let fw = Address::generate(env);
        let rs = Address::generate(env);
        let sg = Address::generate(env);
        let bp = Address::generate(env);
        let ins = Address::generate(env);

        client.init(owner, &fw, &rs, &sg, &bp, &ins);
    }

    #[test]
    fn test_init_success() {
        let (env, owner) = setup_test();
        let client = register_orchestrator(&env);
        let fw = Address::generate(&env);
        let rs = Address::generate(&env);
        let sg = Address::generate(&env);
        let bp = Address::generate(&env);
        let ins = Address::generate(&env);

        let result = client.try_init(&owner, &fw, &rs, &sg, &bp, &ins);

        assert_eq!(result, Ok(Ok(true)));
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
    fn test_get_nonce() {
        let (env, _owner) = setup_test();
        let client = register_orchestrator(&env);
        let user = Address::generate(&env);
        assert_eq!(client.get_nonce(&user), 0);
    }

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
        let result = client.try_set_version(&non_owner, &2);
        assert_eq!(result, Err(Ok(OrchestratorError::Unauthorized)));
    }

    #[test]
    fn test_execute_flow_invalid_amount() {
        let (env, owner) = setup_test();
        let client = register_orchestrator(&env);
        init_orchestrator(&env, &client, &owner);

        let executor = Address::generate(&env);
        let deadline = env.ledger().timestamp() + 1000;

        let hash = compute_test_hash(
            &env,
            symbol_short!("flow"),
            0,
            0, // Invalid amount
            deadline,
        );

        let result = client.try_execute_remittance_flow(
            &executor, &0, // amount 0
            &0, &deadline, &hash,
        );

        assert_eq!(result, Err(Ok(OrchestratorError::InvalidAmount)));
    }

    #[test]
    fn test_execute_flow_expired_deadline() {
        let (env, owner) = setup_test();
        let client = register_orchestrator(&env);
        init_orchestrator(&env, &client, &owner);

        let executor = Address::generate(&env);
        let deadline = env.ledger().timestamp() - 100; // Expired

        let hash = compute_test_hash(&env, symbol_short!("flow"), 0, 1000, deadline);

        let result = client.try_execute_remittance_flow(&executor, &1000, &0, &deadline, &hash);

        assert_eq!(result, Err(Ok(OrchestratorError::DeadlineExpired)));
    }

    #[test]
    fn test_execute_flow_deadline_too_far() {
        let (env, owner) = setup_test();
        let client = register_orchestrator(&env);
        init_orchestrator(&env, &client, &owner);

        let executor = Address::generate(&env);
        let deadline = env.ledger().timestamp() + MAX_DEADLINE_WINDOW_SECS + 1000;

        let hash = compute_test_hash(&env, symbol_short!("flow"), 0, 1000, deadline);

        let result = client.try_execute_remittance_flow(&executor, &1000, &0, &deadline, &hash);

        assert_eq!(result, Err(Ok(OrchestratorError::DeadlineExpired)));
    }

    #[test]
    fn test_execute_flow_invalid_hash() {
        let (env, owner) = setup_test();
        let client = register_orchestrator(&env);
        init_orchestrator(&env, &client, &owner);

        let executor = Address::generate(&env);
        let deadline = env.ledger().timestamp() + 1000;

        let bad_hash = 12345u64;

        let result = client.try_execute_remittance_flow(&executor, &1000, &0, &deadline, &bad_hash);

        assert_eq!(result, Err(Ok(OrchestratorError::InvalidNonce)));
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
            })
        );
    }

    #[test]
    fn test_reentrancy_lock() {
        let (env, owner) = setup_test();
        let client = register_orchestrator(&env);
        init_orchestrator(&env, &client, &owner);

        // Manually set execution lock (simulating reentrancy)
        env.as_contract(&client.address, || {
            env.storage()
                .instance()
                .set(&symbol_short!("EXEC_LOCK"), &true);
        });

        let executor = Address::generate(&env);
        let deadline = env.ledger().timestamp() + 1000;
        let hash = compute_test_hash(&env, symbol_short!("flow"), 0, 1000, deadline);

        let result = client.try_execute_remittance_flow(&executor, &1000, &0, &deadline, &hash);

        assert_eq!(result, Err(Ok(OrchestratorError::ExecutionLocked)));
    }

    // ============================================================================
    // Nonce Replay Protection Tests (Issue #648)
    // ============================================================================
    // Verify that the NONCES map and USED_N set prevent replay attacks

    #[test]
    fn test_nonce_starts_at_zero() {
        let (env, owner) = setup_test();
        let client = register_orchestrator(&env);
        init_orchestrator(&env, &client, &owner);

        let executor = Address::generate(&env);
        let nonce = client.get_nonce(&executor);
        assert_eq!(nonce, 0, "New address should start with nonce 0");
    }

    #[test]
    fn test_nonce_increments_after_successful_execution() {
        let (env, owner) = setup_test();
        let client = register_orchestrator(&env);
        init_orchestrator(&env, &client, &owner);

        let executor = Address::generate(&env);
        env.mock_all_auths();

        // First execution with nonce 0
        let deadline = env.ledger().timestamp() + 1000;
        let hash = compute_test_hash(&env, symbol_short!("flow"), 0, 1000, deadline);
        let result = client.try_execute_remittance_flow(&executor, &1000, &0, &deadline, &hash);
        assert!(result.is_ok(), "First execution should succeed");

        // Verify nonce incremented to 1
        let new_nonce = client.get_nonce(&executor);
        assert_eq!(new_nonce, 1, "Nonce should increment after successful execution");
    }

    #[test]
    fn test_replay_same_nonce_fails() {
        let (env, owner) = setup_test();
        let client = register_orchestrator(&env);
        init_orchestrator(&env, &client, &owner);

        let executor = Address::generate(&env);
        env.mock_all_auths();

        let deadline = env.ledger().timestamp() + 1000;
        let hash = compute_test_hash(&env, symbol_short!("flow"), 0, 1000, deadline);

        // First execution with nonce 0
        let result1 = client.try_execute_remittance_flow(&executor, &1000, &0, &deadline, &hash);
        assert!(result1.is_ok(), "First execution should succeed");

        // Attempt replay with same nonce 0 (nonce should now be 1)
        let result2 = client.try_execute_remittance_flow(&executor, &1000, &0, &deadline, &hash);
        assert_eq!(
            result2,
            Err(Ok(OrchestratorError::InvalidNonce)),
            "Replay of same nonce should fail (nonce mismatch)"
        );
    }

    #[test]
    fn test_out_of_order_nonce_fails() {
        let (env, owner) = setup_test();
        let client = register_orchestrator(&env);
        init_orchestrator(&env, &client, &owner);

        let executor = Address::generate(&env);
        env.mock_all_auths();

        let deadline = env.ledger().timestamp() + 1000;

        // Attempt to execute with nonce 5 when current nonce is 0
        let hash = compute_test_hash(&env, symbol_short!("flow"), 5, 1000, deadline);
        let result = client.try_execute_remittance_flow(&executor, &1000, &5, &deadline, &hash);

        assert_eq!(
            result,
            Err(Ok(OrchestratorError::InvalidNonce)),
            "Out-of-order nonce should fail (must equal current nonce)"
        );
    }

    #[test]
    fn test_skipped_nonce_prevents_reuse() {
        let (env, owner) = setup_test();
        let client = register_orchestrator(&env);
        init_orchestrator(&env, &client, &owner);

        let executor = Address::generate(&env);
        env.mock_all_auths();

        let deadline = env.ledger().timestamp() + 1000;

        // Execute with nonce 0
        let hash0 = compute_test_hash(&env, symbol_short!("flow"), 0, 1000, deadline);
        let result0 = client.try_execute_remittance_flow(&executor, &1000, &0, &deadline, &hash0);
        assert!(result0.is_ok());

        // Execute with nonce 1
        let hash1 = compute_test_hash(&env, symbol_short!("flow"), 1, 2000, deadline);
        let result1 = client.try_execute_remittance_flow(&executor, &2000, &1, &deadline, &hash1);
        assert!(result1.is_ok());

        // Nonce should now be 2
        let current_nonce = client.get_nonce(&executor);
        assert_eq!(current_nonce, 2);

        // Now try to reuse nonce 0 (should fail because it's in USED_N)
        let hash_old = compute_test_hash(&env, symbol_short!("flow"), 0, 1000, deadline);
        let result_replay = client.try_execute_remittance_flow(&executor, &1000, &0, &deadline, &hash_old);
        assert_eq!(
            result_replay,
            Err(Ok(OrchestratorError::InvalidNonce)),
            "Reused nonce should fail even if counter was advanced"
        );
    }

    #[test]
    fn test_multiple_addresses_independent_nonces() {
        let (env, owner) = setup_test();
        let client = register_orchestrator(&env);
        init_orchestrator(&env, &client, &owner);

        let executor1 = Address::generate(&env);
        let executor2 = Address::generate(&env);
        env.mock_all_auths();

        let deadline = env.ledger().timestamp() + 1000;

        // Executor1 starts with nonce 0
        let nonce1_before = client.get_nonce(&executor1);
        assert_eq!(nonce1_before, 0);

        // Executor2 starts with nonce 0
        let nonce2_before = client.get_nonce(&executor2);
        assert_eq!(nonce2_before, 0);

        // Execute for executor1 with nonce 0
        let hash1 = compute_test_hash(&env, symbol_short!("flow"), 0, 1000, deadline);
        let result1 = client.try_execute_remittance_flow(&executor1, &1000, &0, &deadline, &hash1);
        assert!(result1.is_ok());

        // Executor1 nonce should be 1
        assert_eq!(client.get_nonce(&executor1), 1);

        // Executor2 nonce should still be 0 (independent)
        assert_eq!(client.get_nonce(&executor2), 0);

        // Executor2 can execute with nonce 0
        let hash2 = compute_test_hash(&env, symbol_short!("flow"), 0, 500, deadline);
        let result2 = client.try_execute_remittance_flow(&executor2, &500, &0, &deadline, &hash2);
        assert!(result2.is_ok(), "Executor2 should execute with nonce 0");
    }

    #[test]
    fn test_request_hash_binding_prevents_parameter_swap() {
        let (env, owner) = setup_test();
        let client = register_orchestrator(&env);
        init_orchestrator(&env, &client, &owner);

        let executor = Address::generate(&env);
        env.mock_all_auths();

        let deadline = env.ledger().timestamp() + 1000;

        // Compute hash for amount 1000
        let hash_1000 = compute_test_hash(&env, symbol_short!("flow"), 0, 1000, deadline);

        // Try to execute with different amount but using hash from 1000
        let result = client.try_execute_remittance_flow(&executor, &5000, &0, &deadline, &hash_1000);

        assert_eq!(
            result,
            Err(Ok(OrchestratorError::InvalidNonce)),
            "Parameter swap attempt should fail (hash mismatch)"
        );
    }

    #[test]
    fn test_deadline_window_prevents_old_requests() {
        let (env, owner) = setup_test();
        let client = register_orchestrator(&env);
        init_orchestrator(&env, &client, &owner);

        let executor = Address::generate(&env);
        env.mock_all_auths();

        // Create a request with a deadline far in the future
        let current_time = env.ledger().timestamp();
        let far_deadline = current_time + 366 * 86400; // 1 year in future (exceeds MAX_DEADLINE_WINDOW_SECS)

        let hash = compute_test_hash(&env, symbol_short!("flow"), 0, 1000, far_deadline);
        let result = client.try_execute_remittance_flow(&executor, &1000, &0, &far_deadline, &hash);

        assert_eq!(
            result,
            Err(Ok(OrchestratorError::DeadlineExpired)),
            "Request with deadline too far in future should fail"
        );
    }
}
