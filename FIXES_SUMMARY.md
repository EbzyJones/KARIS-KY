# Four Issues Fixed - Summary

This document summarizes the fixes for the four issues identified in KARIS-KY escrow contract.

## Issue 1: Document `check_escrow_health` semantics and add tests

**Status:** ✅ COMPLETE

### Changes
- **File:** `/workspaces/KARIS-KY/escrow/src/tests/health_warnings.rs`
- **Action:** Added 5 comprehensive tests validating the semantics of `time_to_maturity_secs`:

1. **`test_check_escrow_health_pre_maturity_positive_seconds`**
   - Validates that `time_to_maturity_secs` is POSITIVE before maturity
   - Tests at 12 hours before maturity, expects ~43200 seconds

2. **`test_check_escrow_health_at_exact_maturity_zero_seconds`**
   - Validates that `time_to_maturity_secs` is exactly 0 at maturity timestamp

3. **`test_check_escrow_health_post_maturity_negative_seconds`**
   - Validates that `time_to_maturity_secs` is NEGATIVE after maturity
   - Tests 1 day past maturity, expects ~-86400 seconds
   - Validates warning_type = 4003 for underfunded + past maturity

4. **`test_check_escrow_health_no_maturity_imax`**
   - Validates that `time_to_maturity_secs` is `i64::MAX` when no maturity lock (maturity == 0)

5. **`test_check_escrow_health_maturity_units_seconds`**
   - Validates that units are in SECONDS (not milliseconds or other)
   - Sets maturity to 3600 seconds, verifies exact return

### Documentation
- The return type documentation in both `/escrow/src/admin.rs` (line 571) and `/escrow/src/lib.rs` (line 2781) already correctly documented:
  - `time_to_maturity_secs`: seconds until maturity (negative if past, i64::MAX if no maturity)
  - This makes the endpoint usable by off-chain risk monitoring

---

## Issue 2: Lower cap operator changes with CapLowered event

**Status:** ✅ COMPLETE (Already Implemented)

### Findings
- **Event:** `MaxUniqueInvestorsCapLowered` already exists and emits properly
- **Location:** `/workspaces/KARIS-KY/escrow/src/events.rs` (lines 35-42)
- **Implementation:** `/workspaces/KARIS-KY/escrow/src/funding.rs` (line 880)
- **Documentation:** `/workspaces/KARIS-KY/docs/EVENT_SCHEMA.md` (lines 89-103)
- **Testing:** `/workspaces/KARIS-KY/escrow/src/tests/cap_validation.rs` (`test_lower_cap_emits_event()`)

### Event Details
```rust
#[contractevent]
pub struct MaxUniqueInvestorsCapLowered {
    #[topic]
    pub name: Symbol,              // "inv_cap"
    #[topic]
    pub invoice_id: Symbol,        // Escrow invoice id
    pub old_cap: u32,              // Previous cap
    pub new_cap: u32,              // New cap
}
```

The event captures both old and new cap values, giving indexers a complete audit trail of cap changes.

---

## Issue 3: Add FundingDeadlineSet event for deadline changes

**Status:** ✅ COMPLETE

### Changes

#### 1. Event Definition
- **File:** `/workspaces/KARIS-KY/escrow/src/events.rs`
- **Lines:** 44-50
- **Added:**
```rust
/// Emitted when a funding deadline is set or cleared at initialization.
/// Indexers use this to track deadline changes without polling.
#[contractevent]
pub struct FundingDeadlineSet {
    #[topic]
    pub name: Symbol,              // "fund_dl"
    #[topic]
    pub invoice_id: Symbol,        // Escrow invoice id
    /// The funding deadline timestamp (in ledger seconds), or None if cleared.
    pub deadline: Option<u64>,
}
```

#### 2. Event Emission
- **File:** `/workspaces/KARIS-KY/escrow/src/funding.rs`
- **Lines:** 371-378
- **Location:** Immediately after `EscrowInitialized` event in `init_internal()`
- **Action:** Emits `FundingDeadlineSet` with the funding_deadline parameter (None if not configured)

#### 3. Schema Documentation
- **File:** `/workspaces/KARIS-KY/docs/EVENT_SCHEMA.md`
- **Lines:** 50, 97, 117-143
- **Updates:**
  - Updated event catalog count from 24 to 25 events
  - Added `FundingDeadlineSet` to the event catalog table with symbol `fund_dl` and entrypoint `init`
  - Added full topic/data specification section documenting:
    - 3 topics: fixed + name + invoice_id
    - 1 data field: deadline (Option<u64>)
    - Semantics: deadline is Unix seconds, None = no deadline, Some(ts) = exclusive upper bound

#### 4. Test Imports
- **File:** `/workspaces/KARIS-KY/escrow/src/tests.rs`
- **Lines:** 12-17
- **Action:** Added `FundingDeadlineSet` to test module imports

### Semantics
- **Name symbol:** `fund_dl` (short code for "funding deadline")
- **Deadline value:** Unix timestamp (ledger seconds)
- **None case:** When no deadline is configured, funding remains open indefinitely
- **Some(ts) case:** Timestamp is the exclusive upper bound; funding calls fail with `EscrowError::FundingDeadlinePassed` if ledger.timestamp() >= deadline

### Off-Chain Usage
Indexers can now:
- Subscribe to `FundingDeadlineSet` events without polling
- Alert operators on deadline changes
- Track deadline metadata per escrow for risk monitoring dashboards

---

## Issue 4: Add event-schema conformance test

**Status:** ✅ COMPLETE

### Changes

#### 1. New Test Module
- **File:** `/workspaces/KARIS-KY/escrow/src/tests/event_schema_conformance.rs` (NEW)
- **Lines:** ~400 lines of comprehensive validation tests

#### 2. Test Coverage

##### `test_event_schema_escrow_initialized`
- Validates `EscrowInitialized` event has ≥2 topics
- Confirms topic[1] is the routing symbol `escrow_ii`

##### `test_event_schema_funding_deadline_set`
- Validates `FundingDeadlineSet` event when deadline is configured
- Confirms 3 topics: fixed + name (`fund_dl`) + invoice_id
- Validates data structure

##### `test_event_schema_funding_deadline_none`
- Validates `FundingDeadlineSet` event when deadline is NOT configured (None case)
- Ensures event still emitted with proper structure

##### `test_event_schema_max_unique_investors_cap_lowered`
- Validates `MaxUniqueInvestorsCapLowered` event
- Confirms 3 topics: fixed + name (`inv_cap`) + invoice_id
- Validates against EVENT_SCHEMA.md specification

##### `test_event_schema_escrow_funded`
- Validates `EscrowFunded` event structure
- Confirms 4 topics: fixed + name (`funded`) + invoice_id + investor
- Validates data fields are present

##### `test_event_schema_all_have_name_routing_symbol`
- Universal test: validates that ALL emitted events have name routing symbol at topic[1]
- This is a key design invariant: every karis-ky event has a `Symbol` name field for routing
- Comprehensive integration test that triggers init, fund, lower_cap, etc.

#### 3. Module Registration
- **File:** `/workspaces/KARIS-KY/escrow/src/tests.rs`
- **Line:** 54
- **Action:** Added `mod event_schema_conformance;` to test module declarations

### Test Patterns
- All tests use `env.events().all()` to capture emitted events
- Validate topic count, topic types (Symbol, Address), and routing symbols
- Cross-reference captured events with EVENT_SCHEMA.md documentation
- Prevent schema drift: if event structure changes, tests fail immediately

### Prevention of Schema Drift
The test suite ensures:
1. No silent schema changes that would break indexers
2. Event topic/data structure remains stable across releases
3. New events follow the same pattern: topic[1] is always the routing symbol
4. Data fields are correctly typed and present

---

## Verification Checklist

- [x] Issue #1: `check_escrow_health` semantics documented; 5 new tests for pre/at/post maturity
- [x] Issue #2: `MaxUniqueInvestorsCapLowered` event already implemented and documented
- [x] Issue #3: `FundingDeadlineSet` event added; emitted at init; documented in EVENT_SCHEMA.md
- [x] Issue #4: Event schema conformance test suite added; 6 test functions covering major events

## Files Modified

1. `/workspaces/KARIS-KY/escrow/src/events.rs` - Added FundingDeadlineSet event
2. `/workspaces/KARIS-KY/escrow/src/funding.rs` - Added event emission
3. `/workspaces/KARIS-KY/escrow/src/tests.rs` - Added module imports
4. `/workspaces/KARIS-KY/escrow/src/tests/health_warnings.rs` - Added 5 maturity tests
5. `/workspaces/KARIS-KY/escrow/src/tests/event_schema_conformance.rs` - NEW test file
6. `/workspaces/KARIS-KY/docs/EVENT_SCHEMA.md` - Updated catalog and documentation

---

## Implementation Notes

### Testing Strategy
- Tests use the standard Soroban test environment (`Env::default()`)
- Each test is independent and owns a fresh environment
- Tests validate both happy path (deadline set) and edge case (deadline = None)
- Integration test confirms all events follow the routing pattern

### Documentation Strategy
- EVENT_SCHEMA.md now provides a single source of truth
- Schema version management enables future migrations
- Clear semantics documented for off-chain consumers
- Indexer notes clarify how to use the events

### Backward Compatibility
- FundingDeadlineSet is additive (no schema breaking changes)
- When deadline is None, escrow operates as before (open-ended)
- Existing events unchanged
- No action needed for existing deployments

