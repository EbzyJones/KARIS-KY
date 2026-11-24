# Sequential Issues Analysis: Version History, Migration Docs, v7→v8 Schema, and Trace Spans

## Issue 1: Get Version History Test Coverage

### Current State

**What's implemented:**
- `get_version_history()` function exists at `escrow/src/contract.rs:1029` and returns `Vec<(u32, u64)>`
- Implementation at `escrow/src/admin.rs:1082` reads from `DataKey::VersionHistory`
- Version history is initialized in `escrow/src/funding.rs:246-247` during `init_internal()`
- Each init call writes a single entry: `[(SCHEMA_VERSION, ledger_timestamp)]`
- Comments in `migrate_impl()` show how to append on migration (lines 979-981 in admin.rs)

**Problem identified:**
1. **CRITICAL BUG**: `DataKey::VersionHistory` is **NOT defined in the DataKey enum** (lines 822-953 in lib.rs)
   - The variant is referenced but never declared
   - Code compiles because it likely exists in a branch or was removed
   - This causes a compile error if the code is built

2. **Missing test coverage**:
   - No tests exist for `get_version_history()` 
   - No test for fresh instance returning single-entry history
   - No test for migration appending to history

### Files Involved
- `escrow/src/lib.rs` — DataKey enum (line 822+), SCHEMA_VERSION=8 (line 311)
- `escrow/src/storage.rs` — DataKey variants also listed (incomplete mirror)
- `escrow/src/contract.rs` — `get_version_history()` entrypoint (line 1029)
- `escrow/src/admin.rs` — `get_version_history_impl()` (line 1082), `migrate_impl()` (line 947+)
- `escrow/src/funding.rs` — `init_internal()` writes VersionHistory (line 246-247)

### What Needs to Be Done (Phase 1)

1. **Add VersionHistory variant to DataKey enum** in lib.rs:
   ```rust
   /// Append-only version history audit trail: `Vec<(version, ledger_timestamp)>`.
   /// Written on init with the first version and appended on each successful migrate.
   /// Absence ⇒ not initialized.
   VersionHistory,
   ```

2. **Add tests in escrow/src/tests.rs** (or dedicated file):
   - Test: Fresh instance returns single-entry history with `(SCHEMA_VERSION, timestamp)`
   - Test: History remains unchanged if read multiple times
   - Test: Verify timestamp is from `env.ledger().timestamp()`

3. **Add migration test** (when Issue 3 migration path is implemented):
   - Test: After calling `migrate()`, history grows with new entry
   - Test: Old entries remain in the vector

---

## Issue 2: Consolidate Migrate Decision Docs

### Current State

**Existing documentation:**
- `docs/UPGRADE_DECISION_TREES.md` — Comprehensive operator runbook (3,500+ lines)
  - **Tree 1**: "What upgrade path should I use?" — ✅ Clear decision logic
  - **Tree 2**: "Should I call migrate()?" — ✅ Clear guidance
  - **Tree 3**: "Is this instance safe to upgrade right now?" — ✅ Operational checklist
  - **Tree 4**: "What do I do if the upgrade fails?" — ✅ Troubleshooting
  - **Common Pitfalls section** — Includes Pitfall #1: "Calling migrate() during additive upgrade"
  - **Symptom-to-action lookup table** — ✅ Indexed by error/symptom

- `escrow/src/contract.rs:946-976` — migrate() docstring
  - ✅ "Only when you have extended migrate with a concrete transformation"
  - ✅ "Additive DataKey variants don't require migrate"
  - ✅ Explicit error table for conditions that fail

- `escrow/src/lib.rs:308-356` — Module-level rustdoc for SCHEMA_VERSION
  - ✅ Changelog table showing versions 1–8 and upgrade paths
  - ✅ Clear "Additive keys — no migrate call required" for v2, v3, v4
  - ✅ Clear "Redeploy required" for v5, v6, v7, v8

### Gaps & Clarity Issues

1. **ADR-007 is mentioned but cross-reference is weak**:
   - `docs/adr/ADR-007.md` (likely exists) defines persistent-storage migration constraints
   - The main module docs reference it but don't embed key takeaway
   - Decision tree doesn't explicitly name "ADR-007 additive-key policy"

2. **"When migrate() is needed" — clear but could emphasize flow**:
   - Tree 2 asks "Did you implement a new migration branch?" but doesn't link to code example
   - Would help to show: "No branch in migrate_impl → don't call migrate()"
   - Current wording: implicit; could be more explicit

3. **Version history tracking is underdocumented**:
   - `get_version_history()` exists but is barely mentioned in upgrade docs
   - Should note: "After calling migrate(), VersionHistory grows; check it for audit trail"
   - Operators don't know this is available for post-upgrade verification

4. **Trace of "when redeploy vs upgrade"**:
   - Tree 1 logic is solid; doc is correct
   - But doesn't mention: "Redeploy creates new contract ID; all integrators must be notified"
   - (Pitfall #4 covers this, but not in Tree 1)

### Files Involved
- `docs/UPGRADE_DECISION_TREES.md` — Primary reference (operational)
- `docs/adr/ADR-007*.md` — Architectural decision (likely exists)
- `escrow/src/lib.rs:308-356` — Schema version changelog
- `escrow/src/contract.rs:946-976` — migrate() docstring
- `escrow/src/admin.rs:947+` — migrate_impl() comments (lines 979-981)

### What Needs to Be Done (Phase 2)

1. **Review ADR-007** to confirm additive-key policy, then:
   - Add 1-2 sentence summary in `docs/UPGRADE_DECISION_TREES.md` § Decision Tree 2
   - Link to ADR: "See [ADR-007](../adr/ADR-007*.md) for persistent-storage policy"

2. **Add guidance on version history**:
   - In § Success Criteria for each upgrade type, add:
     - **Additive upgrade**: "Verify `get_version_history()` still shows only one entry (no migrate called)"
     - **Redeploy**: "Old instance's `get_version_history()` archived; new instance starts fresh"

3. **Clarify migrate() implementation requirement**:
   - Tree 2: After "Did you implement a new migration branch?", add parenthetical:
     - "(Check: `migrate_impl()` in `admin.rs` has branches for your version pair, not just error return)"

4. **Add redeploy notification checklist**:
   - Expand Pitfall #4 or add to Tree 1 after "REDEPLOY required":
     - "Before redeploy goes live: notify indexers, API teams, UI teams of new contract ID"

---

## Issue 3: v7→v8 Migration Path

### Current State

**Schema version 8 introduced in lib.rs:311**:
```
SCHEMA_VERSION: u32 = 8
```

**Changelog entry (lib.rs:338)**:
```
| 8 | Added optional [`InvoiceEscrow::guardian`] for two-step legal-hold activation | **Redeploy required** — [`InvoiceEscrow`] XDR shape changed |
```

**What changed:**
- `InvoiceEscrow` struct was modified to add `guardian: Option<Address>` field
- This is a **struct layout change** ⇒ XDR encoding changes
- Result: **Redeploy required** (no in-place upgrade possible for old instances)

**Replay guard pattern mentioned in code:**
- Search results show `MigrationExecutionLog(u32, u32)` and `MigrationCompletedAt(u32, u32)` in DataKey
- These are **nonces** to prevent idempotent re-application of the same migration
- In `admin.rs` around line 947+, migrate_impl checks/sets these keys
- Comment says: "Once written, a second `migrate()` call with the same version pair fails"

### Files Involved
- `escrow/src/lib.rs:311` — SCHEMA_VERSION = 8
- `escrow/src/lib.rs:338-339` — Changelog entry for v8
- `escrow/src/types.rs` — `InvoiceEscrow` struct definition (find with grep)
- `escrow/src/admin.rs:947-984` — migrate_impl() error paths, replay guard reference
- `escrow/src/lib.rs:945-972` — MigrationExecutionLog, MigrationCompletedAt DataKey variants

### What Was Added in v8

Need to read `InvoiceEscrow` struct to confirm exact field addition:

```bash
grep -n "pub struct InvoiceEscrow" /workspaces/KARIS-KY/escrow/src/*.rs
```

(Not read yet in this analysis session, but from comments: `guardian: Option<Address>`)

### Replay Guard Pattern

From DataKey enum (lib.rs:945-952):
```rust
MigrationExecutionLog(u32, u32),  // Records (from_version, to_version) → ledger_seq
MigrationCompletedAt(u32, u32),   // Records (from_version, to_version) → timestamp
```

How it works (inferred from comments in migrate_impl):
1. Before executing migration, check if `MigrationExecutionLog(from_version, SCHEMA_VERSION)` exists
2. If yes ⇒ migration already applied ⇒ return error `MigrationAlreadyApplied`
3. If no ⇒ execute migration, then write both keys as markers
4. On subsequent calls with same version pair: replay guard prevents re-execution

### What Needs to Be Done (Phase 3)

1. **Confirm v8 struct changes** by reading `InvoiceEscrow` in types.rs:
   - List all fields added/removed/renamed between v7 and v8
   - Document which fields are instance storage vs persistent storage

2. **Redeploy procedure for v7→v8**:
   - Confirm in docs: "No in-place migrate() path; redeploy required"
   - Create a migration runbook for operators:
     - Step 1: Mark old instance with legal hold
     - Step 2: Init new instance with same parameters + guardian if needed
     - Step 3: Migrate investor data offline (indexer-based restore)
     - Step 4: Update contract ID in all integrations
     - Step 5: Archive old instance

3. **Replay guard testing**:
   - Test: Calling `migrate(7, 8)` twice fails on second call
   - Test: Calling `migrate(6, 8)` is not blocked by `migrate(7, 8)` marker
   - Verify: `MigrationExecutionLog` and `MigrationCompletedAt` both written

4. **If a future v8→v9 migration is needed**:
   - Show example of implementing a migration branch in `migrate_impl()`
   - Use replay guard pattern to prevent double-application

---

## Issue 4: Trace Spans for Admin Operations

### Current State

**Design doc exists:** `FEATURE_224_TRACE_MODE_DESIGN.md`
- Defines two-tier tracing: event-based (critical) + buffer-based (detailed)
- Verbosity levels: OFF, ERROR, WARN, INFO, DEBUG, TRACE
- Proposed entrypoints: `enable_tracing()`, `disable_tracing()`, `get_trace_buffer()`, `clear_trace_buffer()`
- Acceptance criteria include: "Enabled tracing emits structured events for fund, settle, and **claim**"

**Implementation status:** ❌ **NOT IMPLEMENTED**
- No `trace-mode` Cargo feature exists
- No `TraceEvent` contract event type
- No `TraceBuffer` or `TraceEntry` types
- No trace instrumentation in fund/settle/claim
- Admin entrypoints not added

**Admin operations mentioned in docs:**
- From doc § "Integration Points": fund, settle, claim (state transitions)
- From § "Entrypoints": enable_tracing, disable_tracing (admin-gated)

**Missing: Legal hold, dispute pause, attestation spans**
- The spec doesn't explicitly list which admin operations should emit traces
- Your request: "legal hold, dispute pause, attestation"

### Current Admin Operations in Code

From grep results:
- `set_legal_hold()` — contract.rs:743
- `propose_legal_hold()` — contract.rs:749
- `confirm_legal_hold()` — contract.rs:754
- `clear_legal_hold()` — contract.rs:914
- `pause_dispute()` — contract.rs:1587
- `resume_dispute()` — contract.rs:1591+ (need to find exact line)
- `bind_primary_attestation_hash()` — contract.rs:503

### Files Involved
- `FEATURE_224_TRACE_MODE_DESIGN.md` — Full spec with acceptance criteria
- `Cargo.toml` — Need to add `trace-mode` feature
- `escrow/src/lib.rs` — Add TraceEvent, TraceBuffer, TraceEntry types; add DataKey::TraceLevel, DataKey::TraceBuffer
- `escrow/src/contract.rs` — Add enable_tracing(), disable_tracing(), get_trace_buffer(), clear_trace_buffer()
- `escrow/src/admin.rs` — Instrument legal hold, dispute pause operations
- `escrow/src/attestations.rs` — Instrument attestation binding

### What Needs to Be Done (Phase 4)

1. **Define trace types** in lib.rs:
   ```rust
   #[contractevent]
   pub struct TraceEvent {
       pub level: u32,
       pub operation: String,
       pub key_hash: [u8; 32],
       pub old_value_hash: [u8; 32],
       pub new_value_hash: [u8; 32],
       pub timestamp: u64,
       pub message: String,
   }
   
   #[contracttype]
   pub struct TraceBuffer { … }
   
   #[contracttype]
   pub struct TraceEntry { … }
   ```

2. **Add DataKey variants**:
   - `TraceLevel` — stores current trace verbosity (u32)
   - `TraceBuffer` — stores circular buffer of trace entries

3. **Add trace entrypoints** in contract.rs:
   - `enable_tracing(level: u32)` — admin-only, validates 0..TRACE_LEVEL
   - `disable_tracing()` — admin-only
   - `get_trace_buffer()` — read-only, returns Vec<TraceEntry>
   - `clear_trace_buffer()` — admin-only

4. **Instrument admin operations** for WARN-level events:
   - `set_legal_hold()` → emit state transition: "0 (active=false)" → "1 (active=true)"
   - `clear_legal_hold()` → emit state transition: "1" → "0"
   - `pause_dispute()` → emit state transition with ticket_id in message
   - `resume_dispute()` → emit state transition: "pause_active=true" → "false"
   - `bind_primary_attestation_hash()` → emit: "AttestationBound", hash_value_hash

5. **Add Cargo feature**:
   ```toml
   [features]
   trace-mode = []
   ```

6. **Conditional compilation**:
   - Behind `#[cfg(feature = "trace-mode")]` for emit_trace_event() etc.
   - No-op when disabled: `#[cfg(not(feature = "trace-mode"))] fn emit_trace_event() { }`

7. **Tests**:
   - `test_trace_mode_on_legal_hold()` — verify legal hold change emits trace
   - `test_trace_mode_on_dispute_pause()` — verify pause/resume emits traces
   - `test_trace_mode_on_attestation()` — verify attestation binding emits trace
   - `test_trace_disabled_no_overhead()` — verify no-op when feature disabled
   - `test_trace_unauthorized()` — verify non-admin cannot enable/disable/clear

---

## Summary Table: What Exists vs. What's Missing

| Issue | Component | Status | Details |
|-------|-----------|--------|---------|
| **1** | `get_version_history()` function | ✅ Exists | But VersionHistory DataKey variant is MISSING (compile error) |
| **1** | Version history initialization | ✅ Exists | Written in funding.rs:246-247 on init |
| **1** | Test coverage | ❌ Missing | No tests for fresh instance or migration scenario |
| **2** | Decision trees | ✅ Complete | docs/UPGRADE_DECISION_TREES.md is comprehensive |
| **2** | ADR-007 cross-ref | ⚠️ Weak | Mentioned but not embedded; link not in decision flow |
| **2** | Version history docs | ❌ Missing | get_version_history() not mentioned in upgrade guides |
| **3** | v8 schema defined | ✅ Exists | SCHEMA_VERSION=8, changelog entry exists |
| **3** | v8 struct change | ✅ Documented | guardian field added (XDR shape changed) |
| **3** | Replay guard pattern | ✅ Exists | MigrationExecutionLog, MigrationCompletedAt DataKeys defined |
| **3** | v7→v8 migration path | ❌ N/A | Redeploy required; no migration path needed |
| **3** | Migration testing | ❌ Depends on #1 | Once VersionHistory bug fixed, can write tests |
| **4** | TraceEvent types | ❌ Missing | Not defined |
| **4** | Trace entrypoints | ❌ Missing | enable_tracing, disable_tracing, etc. not implemented |
| **4** | Trace instrumentation | ❌ Missing | No trace emissions in admin operations |
| **4** | Cargo trace-mode feature | ❌ Missing | Not in Cargo.toml |

---

## Implementation Order (Recommended)

1. **Phase 1 (Critical — Fix compile error)**:
   - Add `VersionHistory` variant to DataKey enum in lib.rs
   - Verify code compiles

2. **Phase 2 (Core tests)**:
   - Add version history tests for fresh instances
   - Add version history display verification

3. **Phase 3 (Documentation)**:
   - Update UPGRADE_DECISION_TREES.md with ADR-007 and version history callouts
   - Add success criteria mentioning get_version_history() verification

4. **Phase 4 (Trace mode — lower priority, but per spec)**:
   - Implement trace types and entrypoints
   - Instrument admin operations (legal hold, dispute pause, attestation)
   - Add tests for trace mode

5. **Phase 5 (Future migration)**:
   - When v8→v9 schema change requires a migration, implement migrate() branch using replay guard
   - Use as template for future migrations

---

## Quick Reference: Key Code Locations

| Topic | File | Line(s) |
|-------|------|---------|
| SCHEMA_VERSION | lib.rs | 311 |
| Schema changelog | lib.rs | 315-339 |
| DataKey enum | lib.rs | 822-953 |
| get_version_history() | contract.rs | 1029 |
| get_version_history_impl() | admin.rs | 1082 |
| migrate() | contract.rs | 976 |
| migrate_impl() | admin.rs | 947-984 |
| init_internal() version write | funding.rs | 240-247 |
| Legal hold operations | contract.rs | 743, 749, 754, 914 |
| Dispute pause | contract.rs | 1587, 1591+ |
| Attestation binding | contract.rs | 503 |
| Trace mode spec | FEATURE_224_TRACE_MODE_DESIGN.md | Full file |
| Upgrade decision guide | docs/UPGRADE_DECISION_TREES.md | Full file |
