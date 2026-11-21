# KARIS-KY Escrow Repository Exploration Findings

## Executive Summary

This document details findings from a comprehensive exploration of the KARIS-KY escrow smart contract codebase, answering four specific research questions about core health metrics, investor cap operations, funding deadline functionality, and event validation testing.

---

## Question 1: `check_escrow_health` Return Type and `time_to_maturity_secs` Semantics

### Location
- **Public Interface**: `/workspaces/KARIS-KY/escrow/src/contract.rs` (line: `pub fn check_escrow_health(env: Env) -> (u32, i64, i64)`)
- **Implementation**: `/workspaces/KARIS-KY/escrow/src/admin.rs` (line 568: `pub(crate) fn check_escrow_health_impl(env: Env) -> (u32, i64, i64)`)
- **Also exported from**: `/workspaces/KARIS-KY/escrow/src/lib.rs` (via `#[contractimpl]`)

### Return Type
```rust
(u32, i64, i64)
```
Returns a 3-tuple:
1. **`warning_type` (u32)**: Health warning category (0 = no warning, 4001–4004 = specific conditions)
2. **`funded_ratio_bps` (i64)**: Basis points ratio (0–10,000+, clamped to `i64::MAX` on overflow)
3. **`time_to_maturity_secs` (i64)**: Seconds until maturity

### Documentation (from admin.rs, lines 540-566)
```
Read-only health check: compute and return the current health metrics without state mutation.
Returns `(warning_type, funded_ratio_bps, time_to_maturity_secs)`.

# Returns
- `warning_type`: 0 = no warning, 4001–4004 = specific warning condition.
- `funded_ratio_bps`: basis points ratio (0–10_000+, clamped to i64::MAX on overflow).
- `time_to_maturity_secs`: seconds until maturity (negative if past, i64::MAX if no maturity).

# Authorization
None — pure read; no auth required.
```

### `time_to_maturity_secs` Semantics

**YES, it IS NEGATIVE when past maturity.** Implementation (lines 583-590 in admin.rs):

```rust
let time_to_maturity_secs: i64 = if escrow.maturity > 0 {
    let maturity_i64 = escrow.maturity as i64;
    let now_i64 = now as i64;
    maturity_i64.saturating_sub(now_i64)  // Can be negative if now > maturity
} else {
    i64::MAX  // No maturity constraint
};
```

**Key behaviors:**
- **Positive**: Seconds remaining until maturity (normal case when `escrow.maturity > now`)
- **Negative**: Escrow is past maturity (when `escrow.maturity < now`). Uses saturating subtraction, so underflow is prevented.
- **i64::MAX**: Escrow has no maturity lock (`escrow.maturity == 0`)

### Warning Type Logic

Warning code determination (lines 593–608 in admin.rs):

```rust
let warning_type = if time_to_maturity_secs < 0
    && escrow.status == 0
    && escrow.funded_amount < escrow.funding_target
{
    4003  // OverMaturity: past maturity, still open, and unfunded.
} else if time_to_maturity_secs >= 0 && time_to_maturity_secs < 86400 {
    // Close to maturity (< 1 day away).
    if funded_ratio_bps < 5000 {
        4001  // LowFundingRatio near maturity.
    } else {
        4002  // CloseToMaturity with healthy funding.
    }
} else if funded_ratio_bps < 5000 && escrow.status == 0 {
    4001  // LowFundingRatio (open, no immediate time pressure).
} else {
    0  // No warning.
};
```

---

## Question 2: `lower_max_unique_investors` Event Emission and Documentation

### Location
- **Public Interface**: `/workspaces/KARIS-KY/escrow/src/contract.rs` (pub fn `lower_max_unique_investors`)
- **Implementation**: `/workspaces/KARIS-KY/escrow/src/funding.rs` (line 852: `pub(crate) fn lower_max_unique_investors_impl`)

### Event Emission

**YES, `MaxUniqueInvestorsCapLowered` is already emitted.** Implementation (lines 852–892 in funding.rs):

```rust
pub(crate) fn lower_max_unique_investors_impl(env: Env, new_cap: u32) -> u32 {
    ensure(&env, !Self::escrow_paused_active(&env), EscrowError::EscrowIsPaused);
    let escrow = Self::load_escrow_require_admin(&env);
    ensure(&env, escrow.status == 0, EscrowError::CapLowerNotOpen);
    
    let old_cap: Option<u32> = env.storage().instance().get(&DataKey::MaxUniqueInvestorsCap);
    ensure(&env, old_cap.is_some(), EscrowError::NoInvestorCapConfigured);
    let old_cap = old_cap.unwrap();
    let unique_count = Self::get_unique_funder_count_impl(env.clone());
    
    ensure(&env, new_cap < old_cap, EscrowError::NewCapNotLower);
    ensure(&env, new_cap >= unique_count, EscrowError::NewCapBelowCurrentFunderCount);
    
    env.storage().instance().set(&DataKey::MaxUniqueInvestorsCap, &new_cap);
    
    MaxUniqueInvestorsCapLowered {
        name: symbol_short!("inv_cap"),
        invoice_id: escrow.invoice_id.clone(),
        old_cap,
        new_cap,
    }
    .publish(&env);  // <-- EVENT EMITTED HERE
    
    new_cap
}
```

### Event Structure
```rust
#[contractevent]
pub struct MaxUniqueInvestorsCapLowered {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub invoice_id: Symbol,
    pub old_cap: u32,
    pub new_cap: u32,
}
```

### Documentation in EVENT_SCHEMA.md

**YES, fully documented.** From `/workspaces/KARIS-KY/docs/EVENT_SCHEMA.md`, lines 27–57:

```markdown
### `MaxUniqueInvestorsCapLowered`

Emitted after successful `lower_max_unique_investors`.

Topics:

| Index | Field | Type | Value |
|---:|---|---|---|
| 0 | fixed event topic | `Symbol` | `max_unique_investors_cap_lowered` |
| 1 | `name` | `Symbol` | `inv_cap` |
| 2 | `invoice_id` | `Symbol` | Escrow invoice id |

Data:

| Field | Type |
|---|---|
| `old_cap` | `u32` |
| `new_cap` | `u32` |
```

Also referenced in Event Catalog table (line 12):

```markdown
| `MaxUniqueInvestorsCapLowered` | `inv_cap` | `lower_max_unique_investors` |
```

### Test Coverage

Event is tested in `/workspaces/KARIS-KY/escrow/src/tests/cap_validation.rs`, function `test_lower_cap_emits_event()` (lines 865–897):

```rust
#[test]
fn test_lower_cap_emits_event() {
    use soroban_sdk::testutils::Events as _;

    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let client = deploy(&env);
    let contract_id = client.address.clone();

    client.init(...);
    client.fund(&Address::generate(&env), &10_000_000_000i128);
    client.lower_max_unique_investors(&3u32);

    assert_eq!(
        env.events().all(),
        std::vec![MaxUniqueInvestorsCapLowered {
            name: symbol_short!("inv_cap"),
            invoice_id: client.get_escrow().invoice_id,
            old_cap: 5,
            new_cap: 3,
        }
        .to_xdr(&env, &contract_id)]
    );
}
```

This test validates the event structure by converting to XDR, confirming the implementation emits the correct event with correct fields.

---

## Question 3: `FundingDeadlineSet` Event and `DataKey::FundingDeadline` Operations

### Existence of `FundingDeadlineSet` Event

**NO, `FundingDeadlineSet` event DOES NOT EXIST.** Grep search across events.rs returns no match. The funding deadline is set silently during initialization without emitting a dedicated event.

### `DataKey::FundingDeadline` Definition

Location: `/workspaces/KARIS-KY/escrow/src/storage.rs` (line 87–88)

```rust
/// Optional funding deadline (ledger timestamp); after it passes, new funds are rejected.
FundingDeadline,
```

**Storage class**: Instance storage (shared TTL with contract instance).  
**Mutability**: Set once during `init`; no update or clear mechanism exists.  
**Default**: Absent (None) ⇒ no deadline enforced.

### Operations Interacting with `DataKey::FundingDeadline`

**1. SET — During Initialization** (`/workspaces/KARIS-KY/escrow/src/funding.rs`, lines 205–216)

```rust
// Validate funding deadline
if let Some(deadline) = funding_deadline {
    ensure(
        &env,
        deadline > env.ledger().timestamp(),
        EscrowError::FundingDeadlinePassed,
    );
    env.storage()
        .instance()
        .set(&DataKey::FundingDeadline, &deadline);
}
```

Called from `init_impl` and `init_with_guardian_impl` in both `lib.rs` and `funding.rs`.

**2. READ — Get Deadline** (`/workspaces/KARIS-KY/escrow/src/funding.rs`, lines 481–485)

```rust
pub(crate) fn get_funding_deadline_impl(env: Env) -> Option<u64> {
    env.storage().instance().get(&DataKey::FundingDeadline)
}
```

Public read-only getter (exported from contract.rs).

**3. CHECK — Expiry Status** (`/workspaces/KARIS-KY/escrow/src/funding.rs`, lines 488–495)

```rust
pub(crate) fn is_funding_expired_impl(env: Env) -> bool {
    if let Some(deadline) = env.storage().instance().get(&DataKey::FundingDeadline) {
        env.ledger().timestamp() > deadline
    } else {
        false
    }
}
```

Public read-only status check.

**4. ENFORCE — Reject Expired Funding** (`/workspaces/KARIS-KY/escrow/src/funding.rs`, lines 1085–1093)

```rust
// Check funding deadline
if let Some(deadline) = env.storage().instance().get(&DataKey::FundingDeadline) {
    ensure(
        &env,
        env.ledger().timestamp() <= deadline,
        EscrowError::FundingDeadlinePassed,
    );
}
```

Embedded in `fund_impl` to block contributions after deadline expires.

### Summary

| Operation | Location | Behavior |
|---|---|---|
| **SET** | `init`, `init_with_guardian` | Write once; validates deadline > now before storing |
| **GET** | `get_funding_deadline_impl` | Returns `Option<u64>` (None if unset) |
| **CHECK** | `is_funding_expired_impl` | Returns `bool`: true if deadline set and now > deadline |
| **ENFORCE** | `fund_impl` | Rejects funding with `EscrowError::FundingDeadlinePassed` (code 153) if expired |
| **CLEAR** | N/A | **No clear mechanism** — deadline is immutable after init |
| **UPDATE** | N/A | **No update mechanism** — deadline is immutable after init |

---

## Question 4: EVENT_SCHEMA.md Location and Event Validation Tests

### Location of EVENT_SCHEMA.md

**File**: `/workspaces/KARIS-KY/docs/EVENT_SCHEMA.md`

**Size**: ~600 lines  
**Scope**: Complete authoritative reference for all contract events and their Soroban layout (topics and data fields).

### Test Files Validating Events Against Schema

**Yes, tests exist that validate emitted events.** The test suite includes:

#### Primary Event Validation Test

**File**: `/workspaces/KARIS-KY/escrow/src/tests/cap_validation.rs`  
**Function**: `test_lower_cap_emits_event()` (lines 865–897)

```rust
#[test]
fn test_lower_cap_emits_event() {
    use soroban_sdk::testutils::Events as _;

    let env = Env::default();
    env.mock_all_auths();
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let client = deploy(&env);
    let contract_id = client.address.clone();

    client.init(...);
    client.fund(&Address::generate(&env), &10_000_000_000i128);
    client.lower_max_unique_investors(&3u32);

    assert_eq!(
        env.events().all(),
        std::vec![MaxUniqueInvestorsCapLowered {
            name: symbol_short!("inv_cap"),
            invoice_id: client.get_escrow().invoice_id,
            old_cap: 5,
            new_cap: 3,
        }
        .to_xdr(&env, &contract_id)]
    );
}
```

**What it validates:**
1. Event struct fields (`name`, `invoice_id`, `old_cap`, `new_cap`)
2. Field types (Symbol, u32)
3. XDR serialization matches contract output
4. Event is emitted exactly once

#### Test Suite Coverage

The test suite includes multiple other event validation patterns (from `/workspaces/KARIS-KY/escrow/src/tests/`):

- `/workspaces/KARIS-KY/escrow/src/tests/cap_validation.rs`: Tests for `MaxUniqueInvestorsCapLowered`
- `/workspaces/KARIS-KY/escrow/src/tests/health_warnings.rs`: Tests that validate health warning emissions
- Other tests in the `tests/` directory use `env.events().all()` and `env.auths()` patterns

### EVENT_SCHEMA.md Coverage

The schema document catalogs 24+ events with complete layout information:

**Event Catalog** (partial, from EVENT_SCHEMA.md):

| Rust Event | `name` Symbol | Entrypoint(s) |
|---|---:|---|
| `EscrowInitialized` | `escrow_ii` | `init` |
| `MaxUniqueInvestorsCapLowered` | `inv_cap` | `lower_max_unique_investors` |
| `EscrowFunded` | `funded` | `fund`, `fund_with_commitment` |
| `EscrowSettled` | `escrow_sd` | `settle` |
| `MaturityUpdatedEvent` | `maturity` | `update_maturity` |
| ... (19 more events) |

**Layout Information per Event:**
- Topic indices and field types
- Data field types and ranges
- Validation rules
- Nested type definitions

### Test Framework Pattern

Tests use the Soroban SDK's test utilities:

```rust
use soroban_sdk::testutils::Events as _;

// Emit events through contract calls
client.lower_max_unique_investors(&3u32);

// Capture all emitted events
env.events().all()

// Compare against expected struct
assert_eq!(
    env.events().all(),
    std::vec![
        MaxUniqueInvestorsCapLowered { ... }.to_xdr(&env, &contract_id)
    ]
);
```

This pattern ensures:
1. Event structs match XDR serialization
2. Only expected events are emitted
3. No spurious or duplicate events
4. Field values are exactly as specified

---

## Summary Table

| Question | Answer | Location | Notes |
|---|---|---|---|
| **1. `check_escrow_health` return type** | `(u32, i64, i64)` | `admin.rs` line 568, `contract.rs`, `lib.rs` | warning_type, funded_ratio_bps, time_to_maturity_secs |
| **1. `time_to_maturity_secs` is negative past maturity** | **YES** | `admin.rs` lines 583–590 | Uses saturating subtraction; i64::MAX when no maturity lock |
| **2. `MaxUniqueInvestorsCapLowered` emitted** | **YES** | `funding.rs` line 880 `.publish(&env)` | Called after successful cap lowering |
| **2. Event in EVENT_SCHEMA.md** | **YES** | `EVENT_SCHEMA.md` lines 27–57 | Documented with full topic/data layout |
| **2. Event tested** | **YES** | `cap_validation.rs` line 865–897 | `test_lower_cap_emits_event()` validates XDR |
| **3. `FundingDeadlineSet` event exists** | **NO** | All of `events.rs` | No dedicated event; set silently at init |
| **3. `DataKey::FundingDeadline` operations** | SET (init), GET, CHECK, ENFORCE | `storage.rs` line 87, `funding.rs` multiple | No clear/update; write-once immutable |
| **4. EVENT_SCHEMA.md location** | `/workspaces/KARIS-KY/docs/EVENT_SCHEMA.md` | 600+ lines | Authoritative Soroban layout reference |
| **4. Event validation tests exist** | **YES** | `cap_validation.rs` + multiple test suites | Uses `env.events().all()` and `.to_xdr()` patterns |

---

## Code References Summary

### Key Files
- **Contract Interface**: `/workspaces/KARIS-KY/escrow/src/contract.rs`
- **Admin Functions**: `/workspaces/KARIS-KY/escrow/src/admin.rs`
- **Funding Operations**: `/workspaces/KARIS-KY/escrow/src/funding.rs`
- **Event Definitions**: `/workspaces/KARIS-KY/escrow/src/events.rs`
- **Storage Keys**: `/workspaces/KARIS-KY/escrow/src/storage.rs`
- **Schema Documentation**: `/workspaces/KARIS-KY/docs/EVENT_SCHEMA.md`
- **Cap Validation Tests**: `/workspaces/KARIS-KY/escrow/src/tests/cap_validation.rs`
- **Health Warning Tests**: `/workspaces/KARIS-KY/escrow/src/tests/health_warnings.rs`

