#![no_std]
use remitwise_common::{
    CoverageType, DEFAULT_PAGE_LIMIT, INSTANCE_BUMP_AMOUNT, INSTANCE_LIFETIME_THRESHOLD,
    MAX_BATCH_SIZE, MAX_PAGE_LIMIT,
};
use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, Address, Env, String, Vec,
};

// ─────────────────────────────────────────────────────────────────────────────
// Constants
// ─────────────────────────────────────────────────────────────────────────────

const THIRTY_DAYS_SECS: u64 = 30 * 24 * 60 * 60;
const MAX_NAME_LEN: u32 = 64;
const MAX_EXT_REF_LEN: u32 = 128;
const MAX_POLICIES: u32 = 1_000;

// ─────────────────────────────────────────────────────────────────────────────
// Error Codes
// ─────────────────────────────────────────────────────────────────────────────

#[contracterror]
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
#[repr(u32)]
pub enum InsuranceError {
    Unauthorized = 1,
    AlreadyInitialized = 2,
    NotInitialized = 3,
    PolicyNotFound = 4,
    PolicyInactive = 5,
    InvalidName = 6,
    InvalidPremium = 7,
    InvalidCoverageAmount = 8,
    UnsupportedCombination = 9,
    InvalidExternalRef = 10,
    MaxPoliciesReached = 11,
}

// ─────────────────────────────────────────────────────────────────────────────
// Data Types
// ─────────────────────────────────────────────────────────────────────────────

/// Per-type premium and coverage constraints (all values in stroops).
struct TypeConstraints {
    min_premium: i128,
    max_premium: i128,
    min_coverage: i128,
    max_coverage: i128,
}

impl TypeConstraints {
    fn for_type(t: &CoverageType) -> Self {
        match t {
            CoverageType::Health => Self {
                min_premium: 1,
                max_premium: 500_000_000_000,
                min_coverage: 1,
                max_coverage: 100_000_000_000_000,
            },
            CoverageType::Life => Self {
                min_premium: 1,
                max_premium: 1_000_000_000_000,
                min_coverage: 1,
                max_coverage: 500_000_000_000_000,
            },
            CoverageType::Property => Self {
                min_premium: 1,
                max_premium: 2_000_000_000_000,
                min_coverage: 1,
                max_coverage: 1_000_000_000_000_000,
            },
            CoverageType::Auto => Self {
                min_premium: 1,
                max_premium: 750_000_000_000,
                min_coverage: 1,
                max_coverage: 200_000_000_000_000,
            },
            CoverageType::Liability => Self {
                min_premium: 1,
                max_premium: 400_000_000_000,
                min_coverage: 1,
                max_coverage: 50_000_000_000_000,
            },
        }
    }
}

#[contracttype]
#[derive(Clone)]
pub struct Policy {
    pub id: u32,
    pub owner: Address,
    pub name: String,
    pub coverage_type: CoverageType,
    pub monthly_premium: i128,
    pub coverage_amount: i128,
    pub external_ref: core::option::Option<String>,
    pub active: bool,
    pub created_at: u64,
    pub last_payment_at: u64,
    pub next_payment_date: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct PolicyPage {
    pub items: Vec<u32>,
    pub next_cursor: u32,
    pub count: u32,
}

#[contracttype]
#[derive(Clone)]
pub struct PolicyCreatedEvent {
    pub policy_id: u32,
    pub name: String,
    pub coverage_type: CoverageType,
    pub monthly_premium: i128,
    pub coverage_amount: i128,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct PremiumPaidEvent {
    pub policy_id: u32,
    pub name: String,
    pub amount: i128,
    pub next_payment_date: u64,
    pub timestamp: u64,
}

#[contracttype]
#[derive(Clone)]
pub struct PolicyDeactivatedEvent {
    pub policy_id: u32,
    pub name: String,
    pub timestamp: u64,
}

#[contracttype]
pub enum DataKey {
    Owner,
    PolicyCount,
    Policy(u32),
    ActivePolicies,
    OwnerPolicies(Address),
    Initialized,
}

// ─────────────────────────────────────────────────────────────────────────────
// Contract
// ─────────────────────────────────────────────────────────────────────────────

#[contract]
pub struct Insurance;

#[contractimpl]
impl Insurance {
    // ── Initialization ───────────────────────────────────────────────────────

    pub fn init(env: Env, owner: Address) {
        if env.storage().instance().has(&DataKey::Initialized) {
            panic!("already initialized");
        }
        env.storage().instance().set(&DataKey::Initialized, &true);
        env.storage().instance().set(&DataKey::Owner, &owner);
        env.storage().instance().set(&DataKey::PolicyCount, &0u32);
        env.storage()
            .instance()
            .set(&DataKey::ActivePolicies, &Vec::<u32>::new(&env));
        Self::extend_instance_ttl(&env);
    }

    // ── Internal helpers ─────────────────────────────────────────────────────

    fn require_initialized(env: &Env) {
        if !env.storage().instance().has(&DataKey::Initialized) {
            panic!("not initialized");
        }
    }

    fn extend_instance_ttl(env: &Env) {
        env.storage()
            .instance()
            .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
    }

    fn get_owner(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Owner)
            .unwrap_or_else(|| panic!("contract not initialized"))
    }

    fn load_policy(env: &Env, policy_id: u32) -> Policy {
        env.storage()
            .instance()
            .get(&DataKey::Policy(policy_id))
            .unwrap_or_else(|| panic!("policy not found"))
    }

    fn validate_ext_ref(ext_ref: &core::option::Option<String>) {
        if let Some(r) = ext_ref {
            if r.len() == 0 || r.len() > MAX_EXT_REF_LEN {
                panic!("external_ref length out of range");
            }
        }
    }

    // ── Public API ───────────────────────────────────────────────────────────

    pub fn create_policy(
        env: Env,
        caller: Address,
        name: String,
        coverage_type: CoverageType,
        monthly_premium: i128,
        coverage_amount: i128,
    ) -> u32 {
        Self::require_initialized(&env);
        caller.require_auth();

        if name.len() == 0 {
            panic!("name cannot be empty");
        }
        if name.len() > MAX_NAME_LEN {
            panic!("name too long");
        }
        if monthly_premium <= 0 {
            panic!("monthly_premium must be positive");
        }
        if coverage_amount <= 0 {
            panic!("coverage_amount must be positive");
        }

        let constraints = TypeConstraints::for_type(&coverage_type);
        if monthly_premium < constraints.min_premium || monthly_premium > constraints.max_premium {
            panic!("monthly_premium out of range for coverage type");
        }
        if coverage_amount < constraints.min_coverage || coverage_amount > constraints.max_coverage
        {
            panic!("coverage_amount out of range for coverage type");
        }

        let max_ratio = monthly_premium
            .checked_mul(12)
            .and_then(|v| v.checked_mul(500))
            .unwrap_or(i128::MAX);
        if coverage_amount > max_ratio {
            panic!("unsupported combination: coverage_amount too high relative to premium");
        }

        let mut active = env
            .storage()
            .instance()
            .get::<_, Vec<u32>>(&DataKey::ActivePolicies)
            .unwrap_or_else(|| panic!("contract not initialized"));
        if active.len() >= MAX_POLICIES {
            panic!("max policies reached");
        }

        let next_id = env
            .storage()
            .instance()
            .get::<_, u32>(&DataKey::PolicyCount)
            .unwrap_or(0)
            + 1;
        let now = env.ledger().timestamp();
        let policy = Policy {
            id: next_id,
            owner: caller.clone(),
            name: name.clone(),
            coverage_type: coverage_type.clone(),
            monthly_premium,
            coverage_amount,
            external_ref: core::option::Option::None,
            active: true,
            created_at: now,
            last_payment_at: 0,
            next_payment_date: now + THIRTY_DAYS_SECS,
        };

        env.storage()
            .instance()
            .set(&DataKey::Policy(next_id), &policy);
        env.storage()
            .instance()
            .set(&DataKey::PolicyCount, &next_id);
        active.push_back(next_id);
        env.storage()
            .instance()
            .set(&DataKey::ActivePolicies, &active);

        let mut owner_ids = env
            .storage()
            .instance()
            .get::<_, Vec<u32>>(&DataKey::OwnerPolicies(caller.clone()))
            .unwrap_or_else(|| Vec::new(&env));
        owner_ids.push_back(next_id);
        env.storage()
            .instance()
            .set(&DataKey::OwnerPolicies(caller), &owner_ids);

        Self::extend_instance_ttl(&env);
        env.events().publish(
            (symbol_short!("created"), symbol_short!("policy")),
            PolicyCreatedEvent {
                policy_id: next_id,
                name,
                coverage_type,
                monthly_premium,
                coverage_amount,
                timestamp: now,
            },
        );

        next_id
    }

    pub fn pay_premium(env: Env, caller: Address, policy_id: u32) -> bool {
        Self::require_initialized(&env);
        caller.require_auth();

        let mut policy = Self::load_policy(&env, policy_id);
        if !policy.active {
            panic!("policy inactive");
        }
        if caller != policy.owner {
            panic!("Only the policy owner can pay premiums");
        }

        let now = env.ledger().timestamp();
        policy.last_payment_at = now;
        policy.next_payment_date = now + THIRTY_DAYS_SECS;

        env.storage()
            .instance()
            .set(&DataKey::Policy(policy_id), &policy);
        Self::extend_instance_ttl(&env);

        env.events().publish(
            (symbol_short!("paid"), symbol_short!("premium")),
            PremiumPaidEvent {
                policy_id,
                name: policy.name,
                amount: policy.monthly_premium,
                next_payment_date: policy.next_payment_date,
                timestamp: now,
            },
        );

        true
    }

    pub fn batch_pay_premiums(env: Env, caller: Address, ids: Vec<u32>) -> u32 {
        Self::require_initialized(&env);
        caller.require_auth();
        if ids.len() > MAX_BATCH_SIZE {
            panic!("batch too large");
        }

        let mut count = 0u32;
        for id in ids.iter() {
            let mut policy = Self::load_policy(&env, id);
            if policy.active && policy.owner == caller {
                let now = env.ledger().timestamp();
                policy.last_payment_at = now;
                policy.next_payment_date = now + THIRTY_DAYS_SECS;
                env.storage().instance().set(&DataKey::Policy(id), &policy);
                count += 1;
            }
        }
        Self::extend_instance_ttl(&env);
        count
    }

    pub fn set_external_ref(
        env: Env,
        caller: Address,
        policy_id: u32,
        ext_ref: core::option::Option<String>,
    ) -> bool {
        Self::require_initialized(&env);
        caller.require_auth();
        if caller != Self::get_owner(&env) {
            panic!("unauthorized");
        }

        let mut policy = Self::load_policy(&env, policy_id);
        Self::validate_ext_ref(&ext_ref);
        policy.external_ref = ext_ref;
        env.storage()
            .instance()
            .set(&DataKey::Policy(policy_id), &policy);
        true
    }

    pub fn deactivate_policy(env: Env, caller: Address, policy_id: u32) -> bool {
        Self::require_initialized(&env);
        caller.require_auth();
        let mut policy = Self::load_policy(&env, policy_id);
        if caller != policy.owner && caller != Self::get_owner(&env) {
            panic!("unauthorized");
        }
        if !policy.active {
            panic!("already inactive");
        }

        policy.active = false;
        env.storage()
            .instance()
            .set(&DataKey::Policy(policy_id), &policy);

        let mut active = env
            .storage()
            .instance()
            .get::<_, Vec<u32>>(&DataKey::ActivePolicies)
            .unwrap_or_else(|| panic!("contract not initialized"));
        let mut new_active = Vec::new(&env);
        for id in active.iter() {
            if id != policy_id {
                new_active.push_back(id);
            }
        }
        env.storage()
            .instance()
            .set(&DataKey::ActivePolicies, &new_active);

        env.events().publish(
            (symbol_short!("deactive"), symbol_short!("policy")),
            PolicyDeactivatedEvent {
                policy_id,
                name: policy.name,
                timestamp: env.ledger().timestamp(),
            },
        );
        true
    }

    pub fn get_active_policies(env: Env, owner: Address, cursor: u32, limit: u32) -> PolicyPage {
        Self::require_initialized(&env);
        let owner_ids = env
            .storage()
            .instance()
            .get::<_, Vec<u32>>(&DataKey::OwnerPolicies(owner))
            .unwrap_or_else(|| Vec::new(&env));
        let mut items = Vec::new(&env);
        let mut next_cursor = 0u32;
        let lim = if limit == 0 {
            DEFAULT_PAGE_LIMIT
        } else if limit > MAX_PAGE_LIMIT {
            MAX_PAGE_LIMIT
        } else {
            limit
        };

        for id in owner_ids.iter() {
            if id > cursor {
                if let Some(p) = env
                    .storage()
                    .instance()
                    .get::<_, Policy>(&DataKey::Policy(id))
                {
                    if p.active {
                        if items.len() < lim {
                            items.push_back(id);
                        } else {
                            next_cursor = id;
                            break;
                        }
                    }
                }
            }
        }
        let count = items.len();
        PolicyPage {
            items,
            next_cursor,
            count,
        }
    }

    pub fn get_policy(env: Env, policy_id: u32) -> core::option::Option<Policy> {
        Self::require_initialized(&env);
        env.storage().instance().get(&DataKey::Policy(policy_id))
    }

    pub fn get_total_monthly_premium(env: Env, owner: Address) -> i128 {
        Self::require_initialized(&env);
        let owner_ids = env
            .storage()
            .instance()
            .get::<_, Vec<u32>>(&DataKey::OwnerPolicies(owner))
            .unwrap_or_else(|| Vec::new(&env));
        let mut total: i128 = 0;
        for id in owner_ids.iter() {
            if let Some(p) = env
                .storage()
                .instance()
                .get::<_, Policy>(&DataKey::Policy(id))
            {
                if p.active {
                    total = total.saturating_add(p.monthly_premium);
                }
            }
        }
        total
    }
}
