# Implementation Checklists for Sequential Issues

Use these checklists to track progress on each issue.

---

## Issue 1: Get Version History Test Coverage

**Blocker**: Fix CRITICAL_BUG_VERSIONHISTORY first (add VersionHistory DataKey variant)

### Phase 1A: Fix Critical Bug

- [ ] Add `VersionHistory` variant to DataKey enum in escrow/src/lib.rs
- [ ] Run `cargo check` — verify no compile errors
- [ ] Commit: "fix: add missing VersionHistory DataKey variant"

### Phase 1B: Understanding Current Implementation

- [ ] Read escrow/src/funding.rs:240-247 (how init writes history)
- [ ] Read escrow/src/admin.rs:1082 (how get_version_history_impl works)
- [ ] Read escrow/src/admin.rs:947-984 (how migrate_impl should append history)
- [ ] Understand: history is `Vec<(u32, u64)>` = `Vec<(version, timestamp)>`
- [ ] Document in your notes: "Init always writes exactly one entry"

### Phase 1C: Write Fresh Instance Tests

Create new test file or add to escrow/src/tests.rs:

- [ ] `test_get_version_history_fresh_instance_has_single_entry()`
  - [ ] Initialize escrow
  - [ ] Call `get_version_history()`
  - [ ] Assert: result.len() == 1
  - [ ] Assert: result[0].0 == SCHEMA_VERSION (8)
  - [ ] Assert: result[0].1 is a valid ledger timestamp (> 0)

- [ ] `test_get_version_history_not_yet_initialized()`
  - [ ] Create environment without initializing escrow
  - [ ] Call `get_version_history()`
  - [ ] Assert: returns empty vector (default)

- [ ] `test_get_version_history_timestamp_matches_ledger()`
  - [ ] Initialize escrow with known timestamp
  - [ ] Query ledger timestamp before and after init
  - [ ] Verify history timestamp falls within that window

- [ ] `test_get_version_history_multiple_reads_consistent()`
  - [ ] Initialize escrow
  - [ ] Call `get_version_history()` 3 times
  - [ ] Assert: all three calls return identical data (no mutations)

### Phase 1D: Prepare Migration Test Scaffold

(For Phase 1E, which depends on Issue 3 implementation)

- [ ] Create `test_get_version_history_after_migration_appends_entry()`
  - [ ] Initialize escrow (creates history with version 8)
  - [ ] Implement a v8→v9 migration branch in `migrate_impl()` (Issue 3)
  - [ ] Call `migrate(8)`
  - [ ] Query `get_version_history()`
  - [ ] Assert: now has 2 entries
  - [ ] Assert: new entry is `(9, new_timestamp)`
  - [ ] Assert: first entry still exists and unchanged

- [ ] Create `test_get_version_history_export_snapshot_includes_history()`
  - [ ] Initialize escrow
  - [ ] Call `export_escrow_snapshot()`
  - [ ] Verify snapshot.version_history matches `get_version_history()`

### Phase 1E: Run & Verify

- [ ] `cargo test test_get_version_history` — all pass
- [ ] `cargo test --lib` — no regressions
- [ ] Run on testnet (if applicable)
- [ ] Document: "Version history test coverage complete"

**Acceptance Criteria**:
- ✅ Fresh instance returns single-entry history
- ✅ History is immutable across reads
- ✅ Timestamp is from ledger
- ✅ Export snapshot includes history
- ✅ Ready for migration scenario testing

---

## Issue 2: Consolidate Migrate Decision Docs

### Phase 2A: Review Existing Docs

- [ ] Read docs/UPGRADE_DECISION_TREES.md in full
- [ ] Read ADR-007 (find path, likely docs/adr/ADR-007*.md)
- [ ] Read escrow/src/lib.rs:308-356 (schema changelog)
- [ ] Read escrow/src/contract.rs:946-976 (migrate() docstring)
- [ ] List gaps in a working doc

### Phase 2B: Gap Analysis

- [ ] Document: "What's missing from the decision trees?"
  - [ ] Cross-reference to ADR-007 weak? (yes/no)
  - [ ] Version history guidance absent? (yes/no)
  - [ ] Redeploy notification checklist missing? (yes/no)

- [ ] Document: "What's unclear about 'when migrate() is needed'?"
  - [ ] Is the flow obvious to operators? (get feedback)
  - [ ] Does it reference code examples? (yes/no)

### Phase 2C: Updates to docs/UPGRADE_DECISION_TREES.md

- [ ] In § "Decision Tree 2": Add comment linking to ADR-007
  ```markdown
  See [ADR-007](../adr/ADR-007-additive-key-policy.md) for persistent-storage 
  migration rules.
  ```

- [ ] In § "Decision Tree 2": After "Did you implement a new migration branch?":
  ```markdown
  (Hint: Check `admin.rs` line 947+. If migrate_impl() only has error returns, 
   you haven't implemented a migration path yet.)
  ```

- [ ] In § "Success Criteria" → "Additive upgrade success":
  - [ ] Add bullet: "✓ `get_version_history()` returns single entry (migrate() not called)"

- [ ] In § "Success Criteria" → "Redeploy success":
  - [ ] Add bullet: "✓ All integrators notified of new contract ID"
  - [ ] Add sub-bullet: "✓ Indexer updated with new ID"
  - [ ] Add sub-bullet: "✓ API discovery endpoint returns new ID"
  - [ ] Add sub-bullet: "✓ UI cache cleared; new ID live"

- [ ] In § "Common Pitfalls": Update Pitfall #1:
  ```markdown
  ### Pitfall 1 (UPDATED): Calling migrate() during an additive upgrade
  
  **When version history helps:** After upgrade, verify:
  - `get_version_history()` returns exactly one entry
  - If it has two entries, you accidentally called migrate()
  ```

- [ ] Add new pitfall (or expand existing): Redeploy notification
  ```markdown
  ### Pitfall 4 (EXPANDED): Redeploy without integrator notification
  
  Check:
  - [ ] Indexer team notified 24h before
  - [ ] API team has new contract ID mapped
  - [ ] UI team cleared cache and deployed
  - [ ] Investors notified (if applicable)
  ```

### Phase 2D: Update lib.rs module docs (if needed)

- [ ] Add to escrow/src/lib.rs § "## Schema version":
  ```markdown
  Query [`LiquifactEscrow::get_version_history`] to audit which versions
  have been applied to an instance. Helpful for redeploy verification.
  ```

### Phase 2E: Verification

- [ ] Review updated docs
- [ ] Ask another team member: "Is it clear when to call migrate()?"
- [ ] Get thumbs-up or identify remaining gaps
- [ ] Commit: "docs: consolidate migrate decision guidance and add version-history refs"

**Acceptance Criteria**:
- ✅ ADR-007 cross-referenced in decision trees
- ✅ Version history verification in success criteria
- ✅ Redeploy integrator checklist added
- ✅ Code examples or line references provided
- ✅ Team feedback confirms clarity

---

## Issue 3: v7→v8 Migration Path

**Note**: v8 requires redeploy; no in-place migration path is needed.

### Phase 3A: Document v8 Changes

- [ ] Read escrow/src/types.rs:13-24 (InvoiceEscrow struct)
- [ ] Confirm: `guardian: Option<Address>` field added (yes)
- [ ] Document all struct changes (copy to working doc)
- [ ] Confirm: This requires redeploy (yes, XDR shape changed)

### Phase 3B: Create Redeploy Runbook

Use V8_SCHEMA_ANALYSIS.md as reference. Create operator runbook:

- [ ] Filename: docs/REDEPLOY_v7_to_v8_RUNBOOK.md
- [ ] Include:
  - [ ] § What changed (guardian field)
  - [ ] § Why redeploy is required (XDR encoding)
  - [ ] § Phase 1: Testnet validation
  - [ ] § Phase 2: Production planning (checklists)
  - [ ] § Phase 3: Execution (step-by-step)
  - [ ] § Phase 4: Rollback plan (manual recovery)
  - [ ] § Testing checklist
  - [ ] § FAQ

- [ ] Add to runbook:
  - [ ] Legal hold with two-step activation (new feature in v8)
  - [ ] Backward compatibility: guardian=None still works
  - [ ] Investor data migration procedure (offline)

### Phase 3C: Verify Replay Guard Pattern

- [ ] Read escrow/src/lib.rs:945-952 (MigrationExecutionLog, MigrationCompletedAt)
- [ ] Read escrow/src/admin.rs:947+ (migrate_impl usage)
- [ ] Document: "Replay guard prevents same migration from being applied twice"
- [ ] Confirm: implemented correctly (visual inspection)

### Phase 3D: Write Tests for Replay Guard

Create new test file or section in escrow/src/tests.rs:

- [ ] `test_migration_execution_log_prevents_double_application()`
  - [ ] Setup: Create mock migration from v8→v9
  - [ ] Call migrate(8)
  - [ ] Assert: succeeds, writes MigrationExecutionLog(8, 9)
  - [ ] Call migrate(8) again
  - [ ] Assert: fails with MigrationAlreadyApplied (or similar)

- [ ] `test_different_version_pairs_not_blocked_by_each_other()`
  - [ ] Setup: Mock migrations v8→v9 and v7→v8
  - [ ] Execute v8→v9 migration
  - [ ] Verify: v7→v8 is not blocked by the completed v8→v9
  - [ ] Execute v7→v8 (hypothetical)
  - [ ] Both logs exist independently

### Phase 3E: Document v8→v9 Migration Template

(For future schema changes) Create docs/MIGRATION_IMPLEMENTATION_TEMPLATE.md:

- [ ] When to use migrate() vs redeploy
- [ ] How to implement a migration branch
- [ ] Where to write: `if from_version == 8 && SCHEMA_VERSION == 9 { /* ... */ }`
- [ ] How to use replay guard: write MigrationExecutionLog, MigrationCompletedAt
- [ ] How to update version history
- [ ] Testing requirements

### Phase 3F: Verification

- [ ] Testnet: Deploy v8 WASM, init new instances, verify guardian works
- [ ] Testnet: Verify old v7 instances cannot be upgraded (XDR error or graceful fail)
- [ ] Replay guard tests pass
- [ ] Document complete: redeploy runbook + implementation template

**Acceptance Criteria**:
- ✅ v8 changes documented
- ✅ Redeploy procedure documented for operators
- ✅ Replay guard pattern tested
- ✅ Guardian feature tested (legal hold two-step)
- ✅ Future migration template provided

---

## Issue 4: Trace Spans for Admin Operations

### Phase 4A: Design Review

- [ ] Read FEATURE_224_TRACE_MODE_DESIGN.md in full
- [ ] Understand: two-tier tracing (event-based + buffer-based)
- [ ] Understand: verbosity levels (OFF, ERROR, WARN, INFO, DEBUG, TRACE)
- [ ] Confirm: feature is opt-in (`trace-mode` Cargo feature)

### Phase 4B: Identify Admin Operations to Instrument

From your requirements + spec, list operations to emit traces for:

- [ ] Legal hold:
  - [ ] `set_legal_hold(active=true)` → emit WARN-level state transition
  - [ ] `set_legal_hold(active=false)` → emit WARN-level state transition
  - [ ] `propose_legal_hold()` → emit INFO-level event
  - [ ] `confirm_legal_hold()` → emit WARN-level state transition

- [ ] Dispute pause:
  - [ ] `pause_dispute()` → emit state transition (ticket_id in message)
  - [ ] `resume_dispute()` → emit state transition

- [ ] Attestation:
  - [ ] `bind_primary_attestation_hash()` → emit INFO-level event
  - [ ] `append_attestation_digest()` → emit INFO-level event
  - [ ] `revoke_attestation_digest()` → emit state transition

### Phase 4C: Implement Trace Infrastructure

- [ ] Add `trace-mode` feature to Cargo.toml:
  ```toml
  [features]
  trace-mode = []
  ```

- [ ] Add types to escrow/src/lib.rs:
  - [ ] `TraceEvent` struct (contractevent)
  - [ ] `TraceBuffer` struct (contracttype)
  - [ ] `TraceEntry` struct (contracttype)
  - [ ] Constants: TRACE_LEVEL_OFF, ERROR, WARN, INFO, DEBUG, TRACE

- [ ] Add DataKey variants to escrow/src/lib.rs:
  - [ ] `TraceLevel` (stores current level: u32)
  - [ ] `TraceBuffer` (stores circular buffer: Vec<TraceEntry>)

- [ ] Add helper functions to escrow/src/admin.rs or new module:
  - [ ] `get_trace_level(env: &Env) → Option<u32>`
  - [ ] `emit_trace_event(level, operation, key_name, value_hash, message)`
  - [ ] `append_to_trace_buffer(env, entry)`

### Phase 4D: Implement Trace Entrypoints

In escrow/src/contract.rs, add:

- [ ] `enable_tracing(env: Env, level: u32)` — admin-only
  - [ ] Require admin auth
  - [ ] Validate: 0 <= level <= TRACE_LEVEL
  - [ ] Store in DataKey::TraceLevel
  - [ ] Emit TraceConfigured event

- [ ] `disable_tracing(env: Env)` — admin-only
  - [ ] Require admin auth
  - [ ] Remove DataKey::TraceLevel
  - [ ] Emit TraceDisabled event

- [ ] `get_trace_buffer(env: Env) → Vec<TraceEntry>` — permissionless read
  - [ ] Read from DataKey::TraceBuffer
  - [ ] Return entries (or empty if not enabled)

- [ ] `clear_trace_buffer(env: Env)` — admin-only
  - [ ] Require admin auth
  - [ ] Remove DataKey::TraceBuffer
  - [ ] Emit TraceBufferCleared event

### Phase 4E: Instrument Admin Operations

For each operation in Phase 4B:

- [ ] In `set_legal_hold()` (contract.rs:743+):
  - [ ] Get current hold status before mutation
  - [ ] After mutation, emit trace:
    ```rust
    #[cfg(feature = "trace-mode")]
    {
        emit_trace_event(
            env,
            WARN_LEVEL,
            "legal_hold_change",
            "legal_hold",
            format!("{} → {}", old_active, active)
        );
    }
    ```

- [ ] In `pause_dispute()` (contract.rs:1587+):
  - [ ] Emit trace with ticket_id:
    ```rust
    emit_trace_event(WARN_LEVEL, "dispute_pause", "escrow_paused", ticket_id)
    ```

- [ ] In `bind_primary_attestation_hash()` (contract.rs:503+):
  - [ ] Emit trace:
    ```rust
    emit_trace_event(INFO_LEVEL, "attestation_bound", "primary_hash", digest_hash)
    ```

### Phase 4F: Conditional Compilation

- [ ] Wrap all trace logic in `#[cfg(feature = "trace-mode")]`
- [ ] Provide no-op versions for non-trace builds:
  ```rust
  #[cfg(not(feature = "trace-mode"))]
  fn emit_trace_event(_, _, _, _) { /* no-op */ }
  ```

### Phase 4G: Write Tests

In escrow/src/tests.rs:

- [ ] `test_trace_mode_legal_hold_emits_event()`
  - [ ] Enable tracing at WARN level
  - [ ] Call set_legal_hold(true)
  - [ ] Verify trace buffer contains entry with operation="legal_hold_change"

- [ ] `test_trace_mode_dispute_pause_with_ticket()`
  - [ ] Enable tracing at WARN level
  - [ ] Call pause_dispute(ticket_id="TICKET-123", duration=3600)
  - [ ] Verify trace entry contains "TICKET-123"

- [ ] `test_trace_mode_attestation_binding()`
  - [ ] Enable tracing at INFO level
  - [ ] Call bind_primary_attestation_hash(digest)
  - [ ] Verify trace entry with operation="attestation_bound"

- [ ] `test_trace_disabled_no_overhead()`
  - [ ] Build without `trace-mode` feature
  - [ ] Execute fund/settle/claim operations
  - [ ] Verify no trace entries generated
  - [ ] Verify performance unaffected (optional timing check)

- [ ] `test_trace_unauthorized_enable_fails()`
  - [ ] Call enable_tracing() as non-admin
  - [ ] Expect authorization error

- [ ] `test_trace_buffer_bounded()`
  - [ ] Enable tracing
  - [ ] Emit many events (> MAX_TRACE_ENTRIES)
  - [ ] Verify buffer stays within bounds (circular)

### Phase 4H: Operator Documentation

Create docs/TRACE_MODE_OPERATIONS_GUIDE.md:

- [ ] Configuration:
  - [ ] How to enable/disable/clear tracing
  - [ ] Verbosity levels and recommended use

- [ ] Fields in trace entries:
  - [ ] operation (string)
  - [ ] key_name (symbol)
  - [ ] value_hash (not raw value; hashes only)
  - [ ] timestamp

- [ ] Recommended levels:
  - [ ] Production: OFF (default)
  - [ ] Staging: WARN
  - [ ] Development: DEBUG or TRACE

- [ ] Performance impact table:
  - [ ] OFF: 0% overhead
  - [ ] WARN: <1%
  - [ ] DEBUG: ~5%
  - [ ] TRACE: ~15%

### Phase 4I: Verification

- [ ] `cargo test --features trace-mode` — all tests pass
- [ ] `cargo test` (without feature) — all tests pass
- [ ] `cargo build --release --features trace-mode` — compiles cleanly
- [ ] `cargo build --release` (without feature) — compiles cleanly
- [ ] No performance regression in non-trace builds
- [ ] Documentation complete and reviewed

**Acceptance Criteria**:
- ✅ Trace mode is optional Cargo feature
- ✅ Admin operations emit structured WARN/INFO events
- ✅ Trace buffer bounded and clearable
- ✅ Legal hold, dispute pause, attestation traced
- ✅ Tests cover enabled, disabled, unauthorized, buffer-bounds cases
- ✅ Operator guide provided

---

## Final Verification Checklist

When all 4 issues are complete:

- [ ] **Issue 1**: Fresh instance test passes
- [ ] **Issue 1**: Version history appends on migration (when Issue 3 migration path exists)
- [ ] **Issue 2**: Decision trees updated with ADR-007 ref and version-history guidance
- [ ] **Issue 3**: Redeploy runbook documented; replay guard tested
- [ ] **Issue 4**: Trace mode complete with legal hold/dispute/attestation instrumentation
- [ ] All tests pass: `cargo test --lib` and `cargo test --features trace-mode`
- [ ] No compile errors: `cargo check` and `cargo check --features trace-mode`
- [ ] Cargo.toml has `trace-mode` feature
- [ ] Documentation complete: ISSUES_ANALYSIS_SUMMARY.md, V8_SCHEMA_ANALYSIS.md, CRITICAL_BUG_VERSIONHISTORY.md
- [ ] Operator docs updated: UPGRADE_DECISION_TREES.md, REDEPLOY_v7_to_v8_RUNBOOK.md, TRACE_MODE_OPERATIONS_GUIDE.md

**Sign-off**:
- [ ] Code review passed
- [ ] All acceptance criteria met
- [ ] Testnet validation complete
- [ ] Ready for staging/production rollout

