# Issues Analysis Index

Complete analysis of 4 sequential escrow contract issues. Start here.

---

## Quick Navigation

| Document | Purpose | Read Time | Action |
|----------|---------|-----------|--------|
| **ANALYSIS_COMPLETE.md** | Executive summary & next steps | 5 min | Start here first |
| **CRITICAL_BUG_VERSIONHISTORY.md** | Blocking bug in version history | 5 min | Fix immediately (2 min) |
| **ISSUES_ANALYSIS_SUMMARY.md** | Deep analysis of all 4 issues | 15 min | Reference for details |
| **V8_SCHEMA_ANALYSIS.md** | v7→v8 redeploy & guardian feature | 20 min | Ops reference |
| **IMPLEMENTATION_CHECKLISTS.md** | Task breakdowns with test cases | 30 min | Use while coding |

---

## The 4 Issues at a Glance

### Issue 1: Get Version History Test Coverage

**What it is**: Add tests to verify `get_version_history()` returns audit trail of schema versions

**Current state**: Function exists but **blocked by critical bug** (missing DataKey variant)

**Effort**: 2 min (fix bug) + 1-2 hours (write tests) = **1-2 hours** after bug fix

**Priority**: HIGH (bug blocks this)

**Document**: CRITICAL_BUG_VERSIONHISTORY.md, ISSUES_ANALYSIS_SUMMARY.md § Issue 1

---

### Issue 2: Consolidate Migrate Decision Docs

**What it is**: Clarify and improve upgrade decision documentation (when migrate() is needed)

**Current state**: Docs are good; gaps are minor (ADR-007 ref weak, version history missing)

**Effort**: 1-2 hours (consolidate + clarify)

**Priority**: MEDIUM (documentation quality improvement)

**Document**: ISSUES_ANALYSIS_SUMMARY.md § Issue 2

---

### Issue 3: v7→v8 Migration Path

**What it is**: Understand v8 schema change (guardian field) and redeploy procedure

**Current state**: v8 is released; it requires redeploy (XDR shape changed)

**Effort**: 2-3 hours (create runbook + replay guard tests)

**Priority**: MEDIUM (needed for v8 rollout, but no urgent work)

**Document**: V8_SCHEMA_ANALYSIS.md, ISSUES_ANALYSIS_SUMMARY.md § Issue 3

---

### Issue 4: Trace Spans for Admin Operations

**What it is**: Implement optional trace mode for debugging storage operations

**Current state**: Spec complete; zero implementation

**Effort**: 6-8 hours (full implementation with tests)

**Priority**: LOW (post-launch feature, unless required for v8)

**Document**: FEATURE_224_TRACE_MODE_DESIGN.md (spec), ISSUES_ANALYSIS_SUMMARY.md § Issue 4

---

## The Critical Bug

**File**: escrow/src/lib.rs (DataKey enum, line 822+)

**Problem**: Code references `DataKey::VersionHistory` but variant is never declared

**Impact**: Compile error when version history code path is tested

**Fix**: Add 3 lines to enum definition (2 minutes)

**Read**: CRITICAL_BUG_VERSIONHISTORY.md

---

## Implementation Path (Recommended)

```
1. Read ANALYSIS_COMPLETE.md (5 min) ─ understand overview
                ↓
2. Read CRITICAL_BUG_VERSIONHISTORY.md (5 min) ─ understand blocking bug
                ↓
3. Fix bug (2 min) ─ add VersionHistory to DataKey enum
                ↓
4. Implement Issue 1 tests (1-2h) using IMPLEMENTATION_CHECKLISTS.md
                ↓
5. Consolidate Issue 2 docs (1-2h) using ISSUES_ANALYSIS_SUMMARY.md § Issue 2
                ↓
6. Create Issue 3 runbook (2-3h) using V8_SCHEMA_ANALYSIS.md
                ↓
7. Implement Issue 4 trace mode (6-8h) using IMPLEMENTATION_CHECKLISTS.md § Issue 4
```

**Total time**: 13-18 hours

---

## Where to Start Right Now

1. **If you have 5 minutes**: Read ANALYSIS_COMPLETE.md
2. **If you have 10 minutes**: Read ANALYSIS_COMPLETE.md + CRITICAL_BUG_VERSIONHISTORY.md
3. **If you have 30 minutes**: Add the above + ISSUES_ANALYSIS_SUMMARY.md
4. **If you're ready to code**: Jump to IMPLEMENTATION_CHECKLISTS.md (Issue 1)

---

## Key Findings Summary

| Issue | Key Finding | Impact |
|-------|-------------|--------|
| **1** | VersionHistory DataKey variant missing | **Compile error (critical bug)** |
| **1** | get_version_history() function works | Tests can be written once bug fixed |
| **2** | UPGRADE_DECISION_TREES.md is comprehensive | Minor clarifications needed; no overhaul |
| **2** | ADR-007 reference weak | Add cross-link to decision tree |
| **2** | Version history verification missing | Add to success criteria |
| **3** | v8 requires redeploy (not migration) | Correct architecture; create runbook |
| **3** | Guardian field added to InvoiceEscrow | Enables two-step legal hold (feature) |
| **3** | Replay guard pattern exists | Use for future migrations (v8→v9+) |
| **4** | Trace mode spec complete | Zero implementation; substantial work |
| **4** | Admin operations identified | legal hold, dispute pause, attestation |

---

## Document Relationships

```
┌─ ANALYSIS_COMPLETE.md (executive summary)
│  │
│  ├─→ CRITICAL_BUG_VERSIONHISTORY.md (2-min fix)
│  │
│  ├─→ ISSUES_ANALYSIS_SUMMARY.md (deep dive on all 4 issues)
│  │   │
│  │   ├─→ § Issue 1: points to IMPLEMENTATION_CHECKLISTS.md
│  │   ├─→ § Issue 2: points to docs/UPGRADE_DECISION_TREES.md
│  │   ├─→ § Issue 3: points to V8_SCHEMA_ANALYSIS.md
│  │   └─→ § Issue 4: points to FEATURE_224_TRACE_MODE_DESIGN.md & IMPLEMENTATION_CHECKLISTS.md
│  │
│  ├─→ V8_SCHEMA_ANALYSIS.md (v7→v8 redeploy details)
│  │   └─→ Redeploy procedure, guardian feature, operational runbook
│  │
│  └─→ IMPLEMENTATION_CHECKLISTS.md (task breakdowns)
│      ├─→ Phase 1A-E: Issue 1 (test coverage)
│      ├─→ Phase 2A-E: Issue 2 (docs consolidation)
│      ├─→ Phase 3A-F: Issue 3 (redeploy runbook)
│      └─→ Phase 4A-I: Issue 4 (trace mode implementation)
```

---

## File Structure in Workspace

```
/workspaces/KARIS-KY/
├── ANALYSIS_COMPLETE.md ...................... Executive summary
├── CRITICAL_BUG_VERSIONHISTORY.md ........... Blocking bug
├── ISSUES_ANALYSIS_SUMMARY.md .............. Deep analysis
├── V8_SCHEMA_ANALYSIS.md ................... Redeploy runbook
├── IMPLEMENTATION_CHECKLISTS.md ............ Task breakdowns
├── ISSUES_ANALYSIS_INDEX.md ............... This file
├── FEATURE_224_TRACE_MODE_DESIGN.md ....... Trace mode spec (existing)
├── docs/
│   └── UPGRADE_DECISION_TREES.md ......... Decision trees (existing)
└── escrow/
    ├── src/
    │   ├── lib.rs ........................ DataKey enum, SCHEMA_VERSION=8
    │   ├── types.rs ..................... InvoiceEscrow struct (guardian field)
    │   ├── contract.rs ................. Entrypoints (get_version_history, etc.)
    │   ├── admin.rs .................... Implementation (migrate, legal hold, etc.)
    │   ├── funding.rs .................. init_internal writes version history
    │   └── tests.rs .................... Add Issue 1 tests here
    └── Cargo.toml ...................... Add trace-mode feature for Issue 4
```

---

## Success Criteria

When complete:
- ✅ VersionHistory bug fixed (cargo check passes)
- ✅ Issue 1 tests written (version history audit trail)
- ✅ Issue 2 docs consolidated (ADR-007 refs, success criteria updated)
- ✅ Issue 3 runbook created (operator redeploy guide)
- ✅ Issue 4 trace mode implemented (admin operation instrumentation)
- ✅ All tests pass (cargo test --lib, cargo test --features trace-mode)
- ✅ Code review passed
- ✅ Testnet validation complete (if applicable)

---

## Quick Reference: Code Locations

**VersionHistory Bug (Issue 1)**:
- **Problem**: escrow/src/lib.rs line 822+ (DataKey enum)
- **Fix**: Add `VersionHistory` variant

**Version History Usage (Issue 1)**:
- **Read**: escrow/src/admin.rs:1082 (get_version_history_impl)
- **Write**: escrow/src/funding.rs:246-247 (init writes history)

**Migrate Docs (Issue 2)**:
- **Update**: docs/UPGRADE_DECISION_TREES.md

**v8 Schema (Issue 3)**:
- **Struct**: escrow/src/types.rs:13-24 (guardian field)
- **Version**: escrow/src/lib.rs:311 (SCHEMA_VERSION=8)
- **Changelog**: escrow/src/lib.rs:338-339

**Trace Mode (Issue 4)**:
- **Spec**: FEATURE_224_TRACE_MODE_DESIGN.md
- **Implement**: escrow/src/lib.rs (add types), escrow/src/contract.rs (add entrypoints)
- **Instrument**: escrow/src/contract.rs (legal hold, etc.)

---

## Estimated Timeline

| Phase | Issue | Time |
|-------|-------|------|
| 0 | Read docs | 1 hour |
| 1 | Fix VersionHistory bug | 2 min |
| 1 | Issue 1 tests | 1-2 hours |
| 2 | Issue 2 docs | 1-2 hours |
| 3 | Issue 3 runbook + tests | 2-3 hours |
| 4 | Issue 4 trace mode | 6-8 hours |
| **Total** | | **13-18 hours** |

---

## Need Help?

- **Questions about Issue 1?** → Read CRITICAL_BUG_VERSIONHISTORY.md + IMPLEMENTATION_CHECKLISTS.md § Issue 1
- **Questions about Issue 2?** → Read ISSUES_ANALYSIS_SUMMARY.md § Issue 2
- **Questions about Issue 3?** → Read V8_SCHEMA_ANALYSIS.md
- **Questions about Issue 4?** → Read FEATURE_224_TRACE_MODE_DESIGN.md + IMPLEMENTATION_CHECKLISTS.md § Issue 4

---

**Last updated**: Analysis complete  
**Status**: Ready for implementation  
**Next action**: Read ANALYSIS_COMPLETE.md (5 min)
