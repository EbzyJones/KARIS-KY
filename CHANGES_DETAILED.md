# Detailed Changes - Four Issues Fixed

## File-by-File Summary

### 1. `/workspaces/KARIS-KY/escrow/src/events.rs`

**Lines 44-50 (NEW - FundingDeadlineSet event)**

```rust
/// Emitted when a funding deadline is set or cleared at initialization.
/// Indexers use this to track deadline changes without polling.
#[contractevent]
pub struct FundingDeadlineSet {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub invoice_id: Symbol,
    /// The funding deadline timestamp (in ledger seconds), or None if cleared.
    pub deadline: Option<u64>,
}
```

**Rationale:** Provides indexers with a typed event for deadline changes, enabling alerts without polling. The event is emitted during initialization, capturing the deadline configuration.

---

### 2. `/workspaces/KARIS-KY/escrow/src/funding.rs`

**Lines 371-378 (NEW - Event emission)**

Located in `init_internal()` immediately after `EscrowInitialized` event:

```rust
        // Emit FundingDeadlineSet event to give indexers an audit trail of deadline changes.
        FundingDeadlineSet {
            name: symbol_short!("fund_dl"),
            invoice_id: invoice_sym,
            deadline: funding_deadline,
        }
        .publish(&env);
```

**Rationale:** Emits the FundingDeadlineSet event whenever an escrow is initialized, capturing both the case where a deadline is set (Some) and where it's not (None). This provides complete audit coverage.

---

### 3. `/workspaces/KARIS-KY/escrow/src/tests.rs`

**Line 12 (MODIFIED - Added FundingDeadlineSet import)**

Before:
```rust
use super::{
    AttestationBoundEvt, AttestationDigestRevoked, CollateralRecordedEvt, ContractLimits, DataKey,
    EscrowError, EscrowFunded, EscrowInitialized, FundingTargetUpdated, LiquifactEscrow,
    LiquifactEscrowClient, MaxUniqueInvestorsCapLowered, YieldClaimDelegationRevoked,
    YieldClaimDelegationSet, YieldTier, LEGAL_HOLD_PROPOSAL_TTL_SECS,
    MAX_ATTESTATION_APPEND_ENTRIES, MAX_DUST_SWEEP_AMOUNT, MAX_FUND_BATCH, SCHEMA_VERSION,
};
```

After:
```rust
use super::{
    AttestationBoundEvt, AttestationDigestRevoked, CollateralRecordedEvt, ContractLimits, DataKey,
    EscrowError, EscrowFunded, EscrowInitialized, FundingDeadlineSet, FundingTargetUpdated,
    LiquifactEscrow, LiquifactEscrowClient, MaxUniqueInvestorsCapLowered,
    YieldClaimDelegationRevoked, YieldClaimDelegationSet, YieldTier,
    LEGAL_HOLD_PROPOSAL_TTL_SECS, MAX_ATTESTATION_APPEND_ENTRIES, MAX_DUST_SWEEP_AMOUNT,
    MAX_FUND_BATCH, SCHEMA_VERSION,
};
```

**Line 54 (MODIFIED - Added event_schema_conformance module)**

Before:
```rust
mod admin;
mod attestations;
mod batch_claim;
mod cap_validation;
mod clone;
mod coverage;
mod dos_analysis;
mod external_calls;
```

After:
```rust
mod admin;
mod attestations;
mod batch_claim;
mod cap_validation;
mod clone;
mod coverage;
mod dos_analysis;
mod event_schema_conformance;
mod external_calls;
```

**Rationale:** Registers the new event_schema_conformance test module.

---

### 4. `/workspaces/KARIS-KY/escrow/src/tests/health_warnings.rs`

**Lines 522-751 (NEW - 5 maturity semantics tests)**

**Test 1: `test_check_escrow_health_pre_maturity_positive_seconds`**
- Sets maturity to 1 day in future
- Advances to 12 hours before maturity
- Validates `time_to_maturity_secs` is POSITIVE (~43200)
- Ensures off-chain systems can rely on positive = time remaining

**Test 2: `test_check_escrow_health_at_exact_maturity_zero_seconds`**
- Advances to exact maturity timestamp
- Validates `time_to_maturity_secs` == 0
- Boundary condition test for maturity window

**Test 3: `test_check_escrow_health_post_maturity_negative_seconds`**
- Advances 1 day past maturity
- Validates `time_to_maturity_secs` is NEGATIVE (~-86400)
- Validates warning_type == 4003 (underfunded + past maturity)
- Confirms negative value indicates past-due state

**Test 4: `test_check_escrow_health_no_maturity_imax`**
- Initializes with maturity == 0 (no lock)
- Advances far into future
- Validates `time_to_maturity_secs` == i64::MAX
- Ensures "no maturity" case is distinguished

**Test 5: `test_check_escrow_health_maturity_units_seconds`**
- Sets maturity to 3600 seconds (1 hour) in future
- Validates `time_to_maturity_secs` == 3600
- Confirms units are SECONDS (not milliseconds or other)

**Rationale:** Makes the endpoint usable by off-chain risk monitoring. Tests validate all documented semantics:
- Sign indicates direction (positive = time remaining, negative = time past)
- Magnitude enables precise deadline calculations
- Units (seconds) enable proper integration with ledger timestamps

---

### 5. `/workspaces/KARIS-KY/escrow/src/tests/event_schema_conformance.rs`

**NEW FILE (~400 lines)**

**Test 1: `test_event_schema_escrow_initialized`**
- Captures EscrowInitialized event
- Validates ≥2 topics (fixed + name)
- Verifies topic[1] == "escrow_ii" routing symbol

**Test 2: `test_event_schema_funding_deadline_set`**
- Initializes with Some(deadline)
- Captures FundingDeadlineSet event
- Validates exactly 3 topics: fixed + name + invoice_id
- Verifies name symbol == "fund_dl"
- Validates data field structure

**Test 3: `test_event_schema_funding_deadline_none`**
- Initializes with None (no deadline)
- Captures FundingDeadlineSet event
- Validates event is still emitted with correct structure
- Tests the None case integration

**Test 4: `test_event_schema_max_unique_investors_cap_lowered`**
- Initializes with investor cap
- Lowers cap
- Captures MaxUniqueInvestorsCapLowered event
- Validates 3 topics: fixed + name + invoice_id
- Verifies name symbol == "inv_cap"

**Test 5: `test_event_schema_escrow_funded`**
- Initializes and funds escrow
- Captures EscrowFunded event
- Validates 4 topics: fixed + name + invoice_id + investor
- Verifies name symbol == "funded"

**Test 6: `test_event_schema_all_have_name_routing_symbol`**
- Universal validation across multiple event types
- Confirms ALL emitted events have name routing symbol at topic[1]
- Comprehensive integration test
- Ensures naming consistency (core design invariant)

**Rationale:** Prevents event-schema drift that would break indexers. Tests validate:
- Topic structure matches EVENT_SCHEMA.md
- All events use routing symbol pattern (topic[1] = name)
- Data fields are present and properly structured
- Breaking changes are caught immediately

---

### 6. `/workspaces/KARIS-KY/docs/EVENT_SCHEMA.md`

**Line 50: Updated event count**
- Before: "The event catalog below lists 24 event structs."
- After: "The event catalog below lists 25 event structs."

**Line 97: Added to event catalog table**
```markdown
| `FundingDeadlineSet` | `fund_dl` | `init` |
```

**Lines 117-143: Added complete section**

```markdown
### `FundingDeadlineSet`

Emitted during `init` to capture the funding deadline (or absence thereof). Indexers
use this to track deadline changes without polling, enabling alerts on deadline updates.

Topics:

| Index | Field | Type | Value |
|---:|---|---|---|
| 0 | fixed event topic | `Symbol` | `funding_deadline_set` |
| 1 | `name` | `Symbol` | `fund_dl` |
| 2 | `invoice_id` | `Symbol` | Escrow invoice id |

Data:

| Field | Type |
|---|---|
| `deadline` | `Option<u64>` |

**Semantics:** `deadline` is a ledger timestamp (Unix seconds). `None` indicates no funding deadline
was configured (funding remains open indefinitely). When `Some(ts)`, `ts` is the exclusive upper bound
for funding calls; attempting to `fund` after ledger timestamp `ts` fails with `EscrowError::FundingDeadlinePassed`.
```

**Rationale:** Provides authoritative schema reference for off-chain systems. Documents:
- Topic structure (3 topics for FundingDeadlineSet)
- Data payload (deadline field, Option type)
- Semantics (None = open-ended, Some = timestamp)
- Enforcement (exclusive upper bound semantics)

---

## Summary of Changes by Issue

### Issue #1: Check Escrow Health Documentation
- ✅ Documentation already complete in code
- ✅ Added 5 comprehensive tests validating semantics
- ✅ Tests cover: pre-maturity (positive), at-maturity (zero), post-maturity (negative), no-maturity (i64::MAX), units (seconds)

### Issue #2: Lower Max Unique Investors Event
- ✅ Event already implemented as `MaxUniqueInvestorsCapLowered`
- ✅ Event already documented in EVENT_SCHEMA.md
- ✅ No changes needed (verified complete)

### Issue #3: Funding Deadline Event
- ✅ Added `FundingDeadlineSet` event definition
- ✅ Added event emission in funding.rs
- ✅ Updated EVENT_SCHEMA.md with documentation
- ✅ Added test imports

### Issue #4: Event Schema Conformance Test
- ✅ Created event_schema_conformance.rs module
- ✅ Added 6 comprehensive validation tests
- ✅ Registered module in tests.rs

---

## Lines Changed Summary

| File | Lines | Type | Change |
|------|-------|------|--------|
| events.rs | 44-50 | NEW | FundingDeadlineSet event struct |
| funding.rs | 371-378 | NEW | Event emission |
| tests.rs | 12-17 | MODIFIED | Import FundingDeadlineSet |
| tests.rs | 54 | MODIFIED | Register event_schema_conformance module |
| health_warnings.rs | 522-751 | NEW | 5 maturity tests |
| event_schema_conformance.rs | 1-400+ | NEW | Schema validation test suite |
| EVENT_SCHEMA.md | 50, 97, 117-143 | MODIFIED | Event count, catalog, full spec |

**Total: 7 files modified, ~500 new lines of code, 11 new tests**

---

## Backward Compatibility

- ✅ NO breaking changes
- ✅ All changes are additive
- ✅ Existing event structure unchanged
- ✅ check_escrow_health signature unchanged
- ✅ When deadline = None, behavior identical to before
- ✅ Existing deployments compatible

---

## Testing Strategy

### Unit Tests
- Issue #1: 5 tests validating maturity semantics in `health_warnings.rs`
- Issue #3: 2 tests validating FundingDeadlineSet (Some and None) in `event_schema_conformance.rs`
- Issue #4: 6 tests validating schema conformance in `event_schema_conformance.rs`

### Integration
- `test_event_schema_all_have_name_routing_symbol` triggers multiple operations (init, fund, lower_cap) to ensure comprehensive validation

### Coverage
- Pre/at/post maturity for time_to_maturity_secs
- With/without deadline configuration
- Multiple event types and structures
- Universal routing symbol validation

