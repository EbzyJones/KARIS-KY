# CRITICAL BUG: Missing VersionHistory DataKey Variant

## Summary

The code references `DataKey::VersionHistory` in multiple places but the variant is **never declared** in the `DataKey` enum. This causes a **compile error**.

### Impact
- **Severity**: CRITICAL (build failure)
- **Affected files**: 
  - escrow/src/lib.rs (uses VersionHistory but doesn't define it)
  - escrow/src/funding.rs (writes VersionHistory)
  - escrow/src/admin.rs (reads/writes VersionHistory)
- **Status**: Likely not discovered in CI because the code path may not be tested

---

## Evidence

### 1. VersionHistory is USED (but never defined)

**escrow/src/funding.rs:246-247** (in `init_internal()`):
```rust
let mut vh: Vec<(u32, u64)> = Vec::new(&env);
vh.push_back((SCHEMA_VERSION, now_ts));
env.storage().instance().set(&DataKey::VersionHistory, &vh);
```

**escrow/src/admin.rs:1082** (in `get_version_history_impl()`):
```rust
env.storage()
    .instance()
    .get(&DataKey::VersionHistory)
    .unwrap_or_else(|| Vec::new(&env))
```

**escrow/src/admin.rs:979-981** (in `migrate_impl()` comment):
```rust
// When a migration path IS implemented, also record the version change:
//   let mut vh: Vec<(u32, u64)> = env.storage().instance()
//       .get(&DataKey::VersionHistory).unwrap_or_else(|| Vec::new(&env));
```

**escrow/src/lib.rs:4458, 4463, 4642, 4673** (in various places):
```rust
.get(&DataKey::VersionHistory)
.set(&DataKey::VersionHistory, &version_history)
```

### 2. VersionHistory is NOT defined in DataKey enum

**escrow/src/lib.rs:822-953** (DataKey enum):
The enum includes many variants like:
- `Version` (line 828)
- `Escrow` (line 823)
- `InvestorContribution` (line 831)
- `LegalHold` (line 835)
- ... (many more)
- `MigrationExecutionLog(u32, u32)` (line 945)
- `MigrationCompletedAt(u32, u32)` (line 952)

**But NO VersionHistory variant.**

### 3. Compile Error Expected

Running `cargo check` or `cargo build` should fail with an error like:
```
error[E0425]: cannot find value `VersionHistory` in module `DataKey`
  --> escrow/src/funding.rs:247:44
   |
247 |         env.storage().instance().set(&DataKey::VersionHistory, &vh);
    |                                              ^^^^^^^^^^^^^^^ not found in this scope
```

---

## Root Cause

One of the following:

1. **Incomplete refactoring**: DataKey was reorganized, and VersionHistory was moved or removed from the enum definition but not from all usage sites.

2. **Feature branch not merged**: Code to use VersionHistory exists, but the enum variant definition is in a different branch/commit.

3. **Typo or oversight**: The variant was intended but accidentally skipped during enum definition.

---

## Fix Required

### Step 1: Define the Missing Variant

Add to the `DataKey` enum in **escrow/src/lib.rs** (after line 835 or before the closing brace):

```rust
/// Append-only version history audit trail: `Vec<(version, ledger_timestamp)>`.
/// Records when schema versions were written to this instance on init and migrate.
/// One entry per version change. Absent ⇒ not initialized (empty vector default).
/// Helps operators track deployment lineage and audit upgrade events.
/// Read via [`LiquifactEscrow::get_version_history`].
VersionHistory,
```

Or, if preferring a different location in the enum (e.g., after `LegalHoldClearDelay`):

```rust
/// Append-only version history audit trail for this instance.
/// Records: `Vec<(schema_version, ledger_timestamp)>`.
/// Written on init with first version; appended on each successful migrate.
/// Absence ⇒ empty (defaults to empty vector on read).
VersionHistory,
```

### Step 2: Verify Compilation

```bash
cd /workspaces/KARIS-KY
cargo check
# or
cargo build --release
```

Expected result: ✅ No error about missing `DataKey::VersionHistory`.

### Step 3: Verify Tests Pass

```bash
cargo test --lib
```

Expected: All existing tests pass (no behavior change, just fixing the enum).

---

## Why This Matters

1. **Blocks Issue 1 implementation**: Cannot write tests for version history if the DataKey variant doesn't exist
2. **Blocks redeploy/upgrade workflows**: The feature for tracking version changes is incomplete
3. **Operator visibility**: Once fixed, operators can query `get_version_history()` to audit schema upgrades

---

## Code Locations

| File | Line(s) | Action |
|------|---------|--------|
| escrow/src/lib.rs | 822-953 | Add VersionHistory variant here |
| escrow/src/funding.rs | 246-247 | Uses VersionHistory (no change needed) |
| escrow/src/admin.rs | 1082 | Uses VersionHistory (no change needed) |
| escrow/src/admin.rs | 979-981 | Comment shows usage (no change needed) |

---

## Testing After Fix

Once the variant is added, you should be able to:

1. ✅ Compile the escrow contract
2. ✅ Initialize an escrow and verify `get_version_history()` returns `[(8, timestamp)]`
3. ✅ Export snapshot and verify `version_history` field is populated
4. ✅ Prepare tests for Issue 1 (version history test coverage)

---

## Estimate to Fix

- **Time to add variant**: 2 minutes
- **Time to verify compilation**: 2 minutes
- **Time to verify tests**: 5 minutes
- **Total**: ~10 minutes (quick fix)

---

## Follow-up Tasks

After fixing this bug:

1. **Issue 1**: Add tests for `get_version_history()` (depends on this fix)
2. **Issue 3**: When implementing v8→v9 migration, append to version history
3. **CI**: Ensure `cargo check/build` is in the CI pipeline to catch similar issues

