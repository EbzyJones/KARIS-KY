# Schema Version 8: Guardian Field Addition Analysis

## Overview

Schema version 8 introduces an optional `guardian` field to the `InvoiceEscrow` struct to support two-step legal-hold activation. This change requires a **redeploy** because it modifies the XDR-encoded struct layout.

---

## Struct Change Details

### InvoiceEscrow (escrow/src/types.rs:13-24)

**v7 fields:**
```rust
pub invoice_id: Symbol,
pub admin: Address,
pub sme_address: Address,
pub amount: i128,
pub funding_target: i128,
pub funded_amount: i128,
pub yield_bps: i64,
pub maturity: u32,
pub status: u32,
```

**v8 fields (ADDED):**
```rust
pub guardian: Option<Address>,
```

### Why This Requires Redeploy

1. **XDR encoding changes**: When Soroban serializes the struct to persistent storage, the presence of a new field changes the byte layout
2. **Old WASM cannot read v8 data**: If you upgrade WASM to v8 without a migrate() path, old instances cannot decode their stored InvoiceEscrow
3. **Consequence**: `get_escrow()` panics with XDR decode error; escrow becomes read-only and broken

### Mitigation: No In-Place Migration Possible

Unlike additive `DataKey` variants (which can default to `None`), changes to stored struct definitions require:
- Option A: Redeploy (create new instance, migrate investor data offline)
- Option B: Implement a migrate() path that:
  - Reads v7 struct as bytes
  - Reconstructs with new guardian field set to `None`
  - Rewrites atomically
  - But: **This is not feasible on Soroban** because there's no per-field serialization control

**Decision**: **Redeploy required.**

---

## Guardian Usage & Legal Hold Two-Step Activation

### What Guardian Enables

**Before v8** (contract.rs:743):
```rust
pub fn set_legal_hold(env: Env, active: bool, reason: String) {
    // Only admin can activate/deactivate legal hold
}
```

**In v8** (contract.rs:73):
```rust
pub fn init_with_guardian(
    env: Env,
    admin: Address,
    guardian: Address,  // NEW
    ...
) -> InvoiceEscrow
```

### Two-Step Activation

1. **Admin proposes** → `propose_legal_hold(guardian: Address)` (contract.rs:749)
   - Admin initiates legal hold request
   - Proposal stored in `DataKey::LegalHoldProposalExpiresAt`
   - Expires after 1 hour (LEGAL_HOLD_PROPOSAL_TTL_SECS = 3600)

2. **Guardian confirms** → `confirm_legal_hold(guardian: Address)` (contract.rs:754)
   - Guardian address validates and activates the hold
   - Prevents accidental activation by admin alone

3. **Single-step activation (legacy)** → `set_legal_hold(active: true)` still works
   - When guardian is None, admin can activate directly
   - Maintains backward compatibility

### Operational Impact

| Scenario | v7 behavior | v8 behavior | Decision |
|----------|------------|------------|----------|
| init without guardian | ✅ Works | ✅ Works (guardian = None) | No change needed |
| init with guardian | N/A | ✅ New | Operators can opt in |
| set_legal_hold(true) with no guardian | ✅ Admin-only | ✅ Admin-only | Unchanged |
| set_legal_hold(true) with guardian | N/A | ❌ Blocked; must use propose + confirm | New behavior |
| Admin loses key | Funds stranded forever | Funds still stranded (guardian does not override) | No improvement |

---

## RedeplOy Procedure for v7→v8

### Operator Runbook

#### Phase 1: Preparation (on testnet)

1. Deploy new v8 WASM to testnet
2. Initialize new test instances with guardian address
3. Verify `get_escrow()` returns guardian field correctly
4. Test two-step legal hold: `propose_legal_hold()` → `confirm_legal_hold()`
5. Test legacy flow: `set_legal_hold(false)` on instance with guardian=None

#### Phase 2: Production Planning

**Before redeploy:**
- [ ] Identify all v7 instances needing upgrade
- [ ] Prepare new v8 WASM for mainnet deployment
- [ ] Build migration plan:
  - [ ] Extract investor data from v7 instances (query indexer)
  - [ ] Prepare init parameters for v8 instances
  - [ ] Document old → new contract ID mappings
  - [ ] Notify all integrators of coming contract ID change

**Investor communication:**
- [ ] Announce maintenance window (if applicable)
- [ ] Explain: old contract ID will be retired, new ID is active
- [ ] Provide new contract ID to SDKs/UIs/APIs

#### Phase 3: Execution

1. **Activate legal hold on all v7 instances**:
   ```bash
   for instance_id in $(cat instances.txt); do
     stellar contract invoke --id $instance_id -- \
       set_legal_hold --active true --reason "Maintenance: v7→v8 redeploy"
   done
   ```

2. **Deploy new v8 instances**:
   ```bash
   for row in $(cat migration_manifest.csv); do
     instance_id=$(echo $row | cut -d, -f1)
     init_params=$(echo $row | cut -d, -f2-)
     
     stellar contract invoke -- init $init_params
     # Optional: set guardian if known
   done
   ```

3. **Verify new instances**:
   ```bash
   for new_id in $(cat new_instances.txt); do
     stellar contract invoke --id $new_id -- get_version
     # Expected: 8
   done
   ```

4. **Migrate investor data** (offline indexer operation):
   - Query v7 instances for investor contributions
   - Restore to v8 instances via bulk `fund()` calls

5. **Update integrations**:
   - Indexer: redirect queries to new contract IDs
   - API: return new contract ID in discovery endpoints
   - UI: clear cache, force refresh

6. **Archive v7 instances**:
   - Mark old instances with reason annotation
   - Keep legal hold active for safety
   - Document deprecation date

#### Phase 4: Rollback (if critical bug found)

**Not possible via upgrade entrypoint.** Options:
1. Keep v7 instances live; route new funding there (but ID mismatch with integrations)
2. Deploy v8 patch immediately (test thoroughly first)
3. Manual governance recovery (outside contract scope)

**Prevention**: Extra testing on testnet before mainnet rollout.

---

## Migration Data: What Transfers, What Doesn't

### Transferred from v7 to v8 Instances (offline)

These are **per-investor persistent storage** keys that can be queried and restored:
- `DataKey::InvestorContribution` → funding amount
- `DataKey::InvestorEffectiveYield` → tier-selected yield
- `DataKey::InvestorClaimNotBefore` → claim lock timestamp
- `DataKey::InvestorClaimed` → claim status (if already claimed)
- `DataKey::InvestorAllowlisted` → allowlist status

### Instance Storage (NOT transferred; reinitialized in v8)

These are set once at init:
- `DataKey::Escrow` → full struct (with new guardian field)
- `DataKey::Version` → 8 (new schema version)
- `DataKey::VersionHistory` → [(8, timestamp)]
- `DataKey::FundingToken` → same token address
- `DataKey::Treasury` → same treasury address
- `DataKey::YieldTierTable` → copy tier config if applicable
- All other init-time constants

### Immutable After Init (lost if not reinitialized)

- `invoice_id` — **Must be provided again** at init; v8 instance is a new contract with same business semantics but different contract ID
- `admin` — **Must be specified** at init (likely same admin as v7)
- `sme_address` → copy from v7
- `amount` (original target) → query and specify
- `funding_target` → copy from v7
- `yield_bps` → copy from v7
- `maturity` → copy from v7

---

## Example: Redeploy Migration Script

```bash
#!/bin/bash
# Pseudo-code: migrate v7 instance data to v8

set -e

OLD_INSTANCE_ID="CAAAA..."
NEW_INSTANCE_ID="CBBBBB..."
INDEXER_URL="https://indexer.example.com"

# Step 1: Extract investor data from indexer (query v7 instance)
echo "Fetching investor contributions from $OLD_INSTANCE_ID..."
INVESTORS_JSON=$(curl -s "$INDEXER_URL/escrow/$OLD_INSTANCE_ID/investors")

# Step 2: For each investor, call fund() on new instance
echo "Restoring investors to $NEW_INSTANCE_ID..."
while read -r row; do
  INVESTOR_ADDR=$(echo $row | jq -r '.address')
  CONTRIBUTION=$(echo $row | jq -r '.contribution')
  
  stellar contract invoke --id $NEW_INSTANCE_ID -- \
    fund --investor "$INVESTOR_ADDR" --amount "$CONTRIBUTION"
done < <(echo "$INVESTORS_JSON" | jq -c '.[]')

# Step 3: Verify state matches
echo "Verifying state restoration..."
OLD_FUNDED=$(stellar contract invoke --id $OLD_INSTANCE_ID -- \
  get_escrow | jq '.funded_amount')
NEW_FUNDED=$(stellar contract invoke --id $NEW_INSTANCE_ID -- \
  get_escrow | jq '.funded_amount')

if [ "$OLD_FUNDED" -eq "$NEW_FUNDED" ]; then
  echo "✅ Redeploy successful: $OLD_FUNDED tokens restored"
else
  echo "❌ Mismatch: old=$OLD_FUNDED, new=$NEW_FUNDED"
  exit 1
fi

# Step 4: Update contract ID in integrations
echo "Updating integrations to use $NEW_INSTANCE_ID..."
# Send notifications to indexer, API, UI teams

echo "Redeploy complete. Old instance: $OLD_INSTANCE_ID (deprecated)"
```

---

## Testing Checklist for v8 Redeploy

### Unit Tests (new code)

- [ ] `test_init_with_guardian_sets_field()` — guardian is stored and retrievable
- [ ] `test_init_without_guardian_defaults_none()` — backward compat: guardian = None
- [ ] `test_propose_legal_hold_only_with_guardian()` — propose fails if guardian is None
- [ ] `test_confirm_legal_hold_requires_guardian_auth()` — guardian address must call confirm
- [ ] `test_propose_expires_after_ttl()` — proposal times out after 1 hour
- [ ] `test_set_legal_hold_true_bypasses_proposal_if_no_guardian()` — legacy flow still works

### Integration Tests (full redeploy flow)

- [ ] `test_redeploy_fund_restores_investor_data()` — investor contributions survive
- [ ] `test_redeploy_instance_resets_state()` — status becomes "open" on new instance
- [ ] `test_redeploy_version_is_8()` — `get_version()` returns 8
- [ ] `test_redeploy_version_history_single_entry()` — `get_version_history()` has one entry (timestamp from init)
- [ ] `test_redeploy_old_and_new_coexist()` — both instances can be queried during transition
- [ ] `test_old_instance_with_legal_hold_remains_readable()` — data not corrupted

### Manual Smoke Tests (on testnet)

- [ ] Deploy v8 WASM to testnet node
- [ ] Initialize one instance with guardian, one without
- [ ] Fund both instances with test investors
- [ ] Verify `get_escrow()` returns correct guardian value for each
- [ ] Activate legal hold on one instance using `propose` + `confirm`
- [ ] Activate legal hold on other using legacy `set_legal_hold(true)`
- [ ] Verify settlement blocked by legal hold on both
- [ ] Clear legal hold and verify settlement works

---

## Comparison: v7 vs v8 Features

| Feature | v7 | v8 | Notes |
|---------|----|----|-------|
| Legal hold (basic) | ✅ `set_legal_hold()` admin-only | ✅ `set_legal_hold()` still works | Unchanged if guardian = None |
| Legal hold (two-step) | ❌ N/A | ✅ `propose` + `confirm` | Opt-in: set guardian at init |
| Guardian field | ❌ N/A | ✅ `Option<Address>` | New in v8; optional |
| XDR compatibility | ✅ v7 struct | ❌ Different layout | **Redeploy required** |
| Migration path | ✅ N/A (no prior migrations) | ❌ No in-place migrate() | Use redeploy procedure |
| Replay guard | ✅ Exists for migrate() | ✅ Unchanged | Prevents double-application |

---

## FAQ

**Q: Do I have to use a guardian?**
A: No. Initialize with `guardian: None` (or use `init()` instead of `init_with_guardian()`) and legal hold works exactly like v7. Guardian is opt-in.

**Q: What if I have 1000 instances on v7?**
A: You must redeploy each to v8. Plan for:
- 1000 new contract IDs
- Offline data migration (indexer-based)
- 1000 integrator updates (via API layer, ideally)
- 1000 governance approvals if required

**Q: Can I test v8 on testnet while v7 runs on mainnet?**
A: Yes. Testnet and mainnet are separate ledgers. Deploy v8 WASM to testnet, validate, then upload the same WASM hash to mainnet and execute v8 init there.

**Q: Can I rollback from v8 to v7?**
A: No. Once you redeploy to v8, the v8 instance has a new contract ID. Old v7 instances remain on-chain but are archived. To go back, you'd have to migrate data back to v7 instances manually (not automated).

**Q: Does guardian prevent funds from being stranded?**
A: No. Guardian can only confirm legal hold activation. If both admin and guardian keys are lost, funds are still stranded. The purpose of guardian is to reduce the risk of **accidental** over-activation; it does not replace key management best practices or governance.

**Q: What happens to my migrated investor contributions after redeploy?**
A: They are restored via offline `fund()` calls. The new instance will have the same `funded_amount` and investor contributions. Historical data (timestamps, claim status) is lost unless you manually restore it.

---

## Timeline Estimate

| Phase | Activity | Testnet | Mainnet | Effort |
|-------|----------|---------|---------|--------|
| **1** | Build & deploy v8 WASM | 2 hours | — | Eng: 1-2h |
| **2** | Test two-step legal hold | 4 hours | — | QA: 2-4h |
| **3** | Plan redeploy for each instance | — | 8 hours | Ops: 4-8h |
| **4** | Notify integrators | — | 2 hours | Comm: 1-2h |
| **5** | Execute redeploy batch | — | 4 hours | Ops: 2-4h per 100 instances |
| **6** | Verify restoration | — | 4 hours | QA: 2-4h |
| **7** | Decommission v7 | — | 1 hour | Ops: 1h |
| **Total** | — | ~6-10h | ~15-25h | Depends on instance count |

---

## References

- `escrow/src/types.rs:13-24` — InvoiceEscrow struct with guardian field
- `escrow/src/contract.rs:73` — init_with_guardian() entrypoint
- `escrow/src/contract.rs:749` — propose_legal_hold() entrypoint
- `escrow/src/contract.rs:754` — confirm_legal_hold() entrypoint
- `escrow/src/lib.rs:311` — SCHEMA_VERSION = 8
- `escrow/src/lib.rs:338-339` — Changelog entry for v8
- `docs/UPGRADE_DECISION_TREES.md` — Redeploy vs. upgrade decision tree
- `docs/escrow-legal-hold.md` — (likely exists) Legal hold design
- `docs/adr/ADR-004-legal-hold.md` — (likely exists) Legal hold architectural decision
