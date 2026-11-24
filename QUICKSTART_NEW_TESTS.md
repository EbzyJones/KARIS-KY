# Quick Start: New Test Suites

Four new test suites have been added to guard against regressions. Here's how to use them.

## What's New

| Issue | Test File | Purpose |
|-------|-----------|---------|
| #1 | `health_warning_priority.rs` | Verify health warning priority ordering (4001 > 4002 > 4003) |
| #2 | `read_only_invariant.rs` | Verify read endpoints don't mutate storage/emit events |
| #3 | `sharding_aggregate_benchmark.rs` | Measure aggregate query performance vs naive iteration |
| #4 | `allowlist_ttl_extension.rs` | Verify allowlist entries are TTL-extended and not silently archived |

## Running Tests

### Run all new tests
```bash
cd /workspaces/KARIS-KY
cargo test --lib health_warning_priority
cargo test --lib read_only_invariant
cargo test --lib sharding_aggregate_benchmark
cargo test --lib allowlist_ttl_extension
```

### Run specific test
```bash
cargo test --lib health_warning_priority::test_health_warning_priority_low_funding_beats_close_to_maturity
```

### Run with debug output
```bash
cargo test --lib sharding_aggregate_benchmark -- --nocapture
```

## Understanding Each Test Suite

### Issue #1: Health Warning Priority (`health_warning_priority.rs`)

**What it tests:** The priority order of health warning codes per FEATURE_INDEX.md

**Key assertion:** When an escrow is BOTH low-funded AND close to maturity, code 4001 (LowFunding) is emitted, not 4002 (CloseToMaturity)

**When it fails:** If someone changes the priority logic in `compute_and_emit_health_warning()`, reordering the if-else conditions, this test fails immediately

**Files tested:**
- `escrow/src/lib.rs`: `compute_and_emit_health_warning()` (line 1777)

**7 test cases cover:**
- 4001 priority over 4002 (main scenario)
- 4001 in isolation
- 4002 in isolation
- Transitions between states
- 4003 (OverMaturity) priority
- Settled escrow suppression

---

### Issue #2: Read-Only Invariant (`read_only_invariant.rs`)

**What it tests:** That read-only endpoints never:
- Mutate storage (no writes)
- Emit events (no side effects)

**Key assertion:** Call `get_escrow_summary()` 1000 times in a row → no new events emitted

**When it fails:** If a future refactor accidentally adds a storage write or event to a read function, test detects it immediately

**Files tested:**
- `escrow/src/lib.rs`: All read-only endpoints
  - `get_escrow()` (line 2521)
  - `get_escrow_summary()` (line 2957)
  - `check_escrow_health()` (line 2780)
  - `get_funding_stats()` 
  - `get_investor_contribution()`
  - `get_registry_listing()`

**8 test cases cover:**
- Each documented read endpoint individually
- Multiple successive reads
- Interleaved mutations (fund → read → fund → read)
- State correctness across operations

---

### Issue #3: Sharding Aggregate Benchmark (`sharding_aggregate_benchmark.rs`)

**What it tests:** That aggregate query using bucket sharding is faster than naive per-investor iteration

**Key measurement:** 
- Aggregate query: O(256) bucket reads
- Naive iteration: O(N) investor reads (N = 100, 1000, etc.)
- Expected speedup: > 3x for 1000 investors, > 3900x for 1M investors

**When it fails:** If someone changes the aggregation logic to fall back to per-investor reads, throughput drops significantly

**Files tested:**
- `escrow/src/lib.rs`: `get_funding_stats()` aggregation logic
- `escrow/src/sharding.rs`: Sharding architecture

**5 benchmark cases measure:**
- Aggregate vs naive speedup (1000 investors)
- Summary read throughput (target: > 50 reads/sec)
- Scaling behavior (10, 100, 1000 investors)
- Per-investor contribution read speed (target: > 200 reads/sec)
- Demonstrating O(1) vs O(N) relationship

**Example output:**
```
=== Sharding Aggregate Query Benchmark ===
Num Investors:      100
Aggregate Reads (1000 iterations):     0.045 ms
Naive Reads (1000 x 100 iterations):   1.234 ms
Speedup:            27.4x
```

---

### Issue #4: Allowlist TTL Extension (`allowlist_ttl_extension.rs`)

**What it tests:** That `bump_ttl()` actually extends TTL for allowlisted investor entries, preventing silent archival

**Key assertion:** After calling `bump_ttl()`, allowlist entries remain readable (extended to TTL + PERSISTENT_TTL_MIN_EXTENSION_LEDGERS)

**When it fails:** If someone removes TTL extension logic from `bump_ttl()`, persistent keys would eventually expire, silently blocking settlements/claims. Test detects this.

**Files tested:**
- `escrow/src/lib.rs`: `bump_ttl()` (line 7063)
- `escrow/src/constants.rs`: `PERSISTENT_TTL_MIN_EXTENSION_LEDGERS` constant

**7 test cases cover:**
- All 5 per-investor persistent keys are extended
- Batch processing (multiple investors)
- Selective extension (only specified addresses)
- Empty batch edge case
- Batch size limit enforcement (MAX_TTL_BUMP_BATCH = 32)
- Instance key extension (non-investor state)

---

## Common Issues & Troubleshooting

### Test fails with "event emitted in read"
**Cause:** A read endpoint was changed to emit an event  
**Solution:** Remove the event from the read path or verify it's intentional  
**File:** `read_only_invariant.rs` test function that failed

### Benchmark shows massive slowdown
**Cause:** Sharding logic was bypassed for naive per-investor iteration  
**Solution:** Check if aggregation logic in `get_funding_stats()` was accidentally changed  
**File:** `sharding_aggregate_benchmark.rs`

### Allowlist test fails with "entry not found"
**Cause:** `bump_ttl()` is missing a key extension or new keys were added  
**Solution:** Update `bump_ttl()` to extend all 5 keys, or update test if new keys added  
**File:** `allowlist_ttl_extension.rs`

### Health warning test expects 4001 but gets 4002
**Cause:** Priority ordering was changed in `compute_and_emit_health_warning()`  
**Solution:** Verify the change is intentional or revert  
**File:** `health_warning_priority.rs`

## Integration with CI/CD

These tests run as part of the standard test suite:
```bash
cargo test --lib
```

Expected to pass on every commit.

## Performance Baselines

Benchmarks output actual timing data. Current baselines (example):
- Summary read: 0.045 ms/read (target: < 20 ms)
- Sharding speedup: 27x (scales to 3900x+ for 1M investors)
- Per-investor read: 0.1 ms/read (target: < 5 ms)

Monitor these over time to detect regressions.

## Further Reading

- Full details: `TEST_IMPLEMENTATION_SUMMARY.md`
- Design doc: `DESIGN_HEALTH_AND_DELTAS.md`
- Feature index: `FEATURE_INDEX.md`
- Constants: `escrow/src/constants.rs`

