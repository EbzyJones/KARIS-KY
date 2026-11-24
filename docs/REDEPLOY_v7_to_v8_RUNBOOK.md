# v7 → v8 Redeploy Runbook

**Status:** Schema v8 requires redeploy (XDR struct layout changed).  
**Release version:** See `/workspaces/KARIS-KY/escrow/src/lib.rs:SCHEMA_VERSION`  
**Last updated:** When this runbook is first created  

---

## What Changed in v8

### InvoiceEscrow Struct (XDR shape modified)

**New field added:**
```rust
pub guardian: Option<Address>
```

**Purpose:** Two-step legal-hold activation
- `admin` activates (proposed) legal hold via `set_legal_hold(true)`
- `guardian` confirms the hold (second approval step)
- Either party can clear the hold when it's no longer needed

**Backward compatibility:**
- Instances deployed without a guardian use `guardian = None`
- Behavior is unchanged for guardian-free deployments
- New deployments can optionally include a guardian address

### Why Redeploy is Required

The `InvoiceEscrow` struct layout changed (new field). Old instances store the struct in XDR format incompatible with the new layout:

- **Old v7 WASM:** Cannot decode v8's struct on read (XDR mismatch → panic)
- **New v8 WASM:** Cannot decode v7's struct on read (missing field → XDR error)

**Solution:** Deploy new instances; archive old instances.

---

## Timeline & Phases

```
Phase 1 (1 week)    : Testnet validation & sign-off
Phase 2 (3-5 days)  : Production planning & rehearsal
Phase 3 (1 day)     : Staging execution (final checks)
Phase 4 (1 day)     : Production rollout (monitored)
Phase 5 (72 hours)  : Monitoring & reconciliation
```

---

## Phase 1: Testnet Validation (1 week)

### Step 1.1: Deploy v8 WASM to Testnet

1. Build the v8 escrow contract:
   ```bash
   cd escrow
   cargo build --release --target wasm32-unknown-unknown
   ```

2. Upload WASM to Soroban testnet:
   ```bash
   stellar contract invoke \
     --network testnet \
     --source <ADMIN_KEY> \
     --account <ADMIN_ACCOUNT> \
     -- upload-contract \
     --wasm <PATH>/escrow.wasm
   ```
   Record the WASM hash (`<WASM_HASH_v8>`).

### Step 1.2: Initialize New v8 Instance

1. Create a new escrow instance with v8 WASM:
   ```bash
   stellar contract invoke \
     --network testnet \
     --source <ADMIN_KEY> \
     --account <ADMIN_ACCOUNT> \
     --id <NEW_CONTRACT_ID> \
     -- init \
     --admin <ADMIN_ADDRESS> \
     --invoice_id "TEST-V8-001" \
     --sme_address <SME_ADDRESS> \
     --amount 100000 \
     --yield_bps 800 \
     --maturity 0 \
     --funding_token <STABLE_COIN_ADDRESS> \
     --treasury <TREASURY_ADDRESS> \
     --guardian <OPTIONAL_GUARDIAN_ADDRESS>
   ```

2. Verify initialization succeeded:
   ```bash
   stellar contract invoke \
     --network testnet \
     --source <ADMIN_KEY> \
     --id <NEW_CONTRACT_ID> \
     -- get_escrow
   ```
   Confirm:
   - ✅ `guardian` field is present (populated or None)
   - ✅ `version` == 8
   - ✅ `schema_version` == 8

### Step 1.3: Execute Representative Operations on v8

Execute fund → settle → claim workflow:

1. **Fund (testnet investor):**
   ```bash
   stellar contract invoke \
     --network testnet \
     --source <INVESTOR_KEY> \
     --id <NEW_CONTRACT_ID> \
     -- fund --investor <INVESTOR_ADDRESS> --amount 50000
   ```

2. **Settle (when funded):**
   ```bash
   stellar contract invoke \
     --network testnet \
     --source <SME_KEY> \
     --id <NEW_CONTRACT_ID> \
     -- settle
   ```

3. **Claim (after settlement):**
   ```bash
   stellar contract invoke \
     --network testnet \
     --source <INVESTOR_KEY> \
     --id <NEW_CONTRACT_ID> \
     -- claim_investor_payout --investor <INVESTOR_ADDRESS>
   ```

4. Verify all operations succeeded and emitted expected events.

### Step 1.4: Test Legal Hold with Guardian

1. Initialize new v8 instance **with** a guardian:
   ```bash
   stellar contract invoke \
     --network testnet \
     --source <ADMIN_KEY> \
     --id <NEW_CONTRACT_ID_WITH_GUARDIAN> \
     -- init_with_guardian \
     --admin <ADMIN_ADDRESS> \
     --guardian <GUARDIAN_ADDRESS> \
     ... (other params)
   ```

2. Verify guardian field is set:
   ```bash
   stellar contract invoke \
     --network testnet \
     --source <ADMIN_KEY> \
     --id <NEW_CONTRACT_ID_WITH_GUARDIAN> \
     -- get_escrow
   ```

3. Test legal hold activation flow (if implemented in v8):
   ```bash
   # Admin proposes hold
   stellar contract invoke \
     --network testnet \
     --source <ADMIN_KEY> \
     --id <NEW_CONTRACT_ID_WITH_GUARDIAN> \
     -- set_legal_hold --active true --reason "Testing guardian flow"
   
   # Verify hold is active
   stellar contract invoke \
     --network testnet \
     --source <ADMIN_KEY> \
     --id <NEW_CONTRACT_ID_WITH_GUARDIAN> \
     -- get_legal_hold
   ```

4. Guardian and admin can both clear the hold (depending on implementation).

### Step 1.5: Version History Validation

1. Query `get_version()` to confirm schema version:
   ```bash
   stellar contract invoke \
     --network testnet \
     --source <ADMIN_KEY> \
     --id <NEW_CONTRACT_ID> \
     -- get_version
   # Expected: 8
   ```

2. Query `get_version_history()` to confirm no migration occurred:
   ```bash
   stellar contract invoke \
     --network testnet \
     --source <ADMIN_KEY> \
     --id <NEW_CONTRACT_ID> \
     -- get_version_history
   # Expected: [(8, <timestamp>)] — single entry only
   ```

### Step 1.6: Testnet Sign-Off

- [ ] All operations succeeded without errors
- [ ] Guardian field behaves as expected
- [ ] Version history shows single entry (no migration called)
- [ ] Events emitted correctly
- [ ] Security team reviewed XDR changes
- [ ] **Sign-off:** Team lead confirms readiness for staging

---

## Phase 2: Production Planning & Rehearsal (3–5 days)

### Step 2.1: Identify All Production Instances

1. Query your instance registry / indexer for all live escrows:
   ```bash
   SELECT contract_id, invoice_id, status, created_at 
   FROM escrows 
   WHERE network = 'mainnet' 
   ORDER BY created_at DESC;
   ```

2. Categorize instances:
   - **Funded & settleable** (highest priority for legal hold)
   - **Open & funding** (medium priority; may be impacted)
   - **Settled/withdrawn/cancelled** (low priority; can redeploy anytime)

3. Document the list and share with stakeholders.

### Step 2.2: Plan Investor Communication

If required by your business:

1. **Draft notification email:**
   - Explain: why redeploy is necessary (XDR struct change)
   - Confirm: no investor funds are affected
   - State: new contract address will be available on [DATE]
   - Provide: old address (archived) + new address (active)

2. **Timing:** Send notification 48–72 hours before production rollout.

3. **FAQ:** Prepare responses for common questions
   - "Will my funds be lost?" → No, investor data is migrated offline.
   - "Do I need to do anything?" → You'll be notified of new address for claims.
   - "When can I claim my payout?" → After settlement, on the new contract.

### Step 2.3: Prepare Integrator Handoff

1. **Indexer team:**
   - Provide mapping: old contract IDs → new contract IDs
   - Verify indexer can switch queries to new IDs
   - Plan: how will old IDs be handled in queries (archived vs inactive)?

2. **API team:**
   - Update discovery endpoint to return new contract IDs
   - Deprecate old IDs (keep for backward compat, mark as archived)
   - Prepare fallback if new contract ID discovery fails

3. **UI team:**
   - Deploy new frontend that queries new discovery endpoint
   - Test: navigating to old contract ID redirects to new ID or shows "archived"
   - Clear browser/CDN caches to avoid serving old ID

4. **Timing:** All integrators should be **deployment-ready** before Phase 3 begins.

### Step 2.4: Investor Data Migration Strategy

1. **For funded instances (needs investor history):**
   - Export snapshot from old v7 instance: `export_escrow_snapshot()`
   - Off-chain process: map investor contributions + yield + claims to new v8 instance
   - (Implementation depends on your indexer and data layer)

2. **For open/empty instances:**
   - No investor history to migrate
   - New instance initialized fresh
   - Old instance archived

3. **For settled instances:**
   - No further investor claims possible
   - Archive old contract, no migration needed

### Step 2.5: Rehearsal on Staging

1. **Deploy v8 WASM to staging:**
   ```bash
   stellar contract invoke \
     --network staging \
     ... (same as testnet Phase 1)
   ```

2. **Simulate redeploy for 1–2 instances:**
   - Deploy new instance
   - Restore investor data (if needed)
   - Run fund → settle → claim workflow

3. **Verify:**
   - All operations succeed
   - Events emitted correctly
   - Version history shows correct entries
   - Indexer/API picks up new ID

4. **Get sign-off:** Operations team confirms readiness.

---

## Phase 3: Staging Execution (1 day)

### Step 3.1: Legal Hold Activation (if instances are funded)

For each funded or open instance:

```bash
stellar contract invoke \
  --network mainnet \
  --source <ADMIN_KEY> \
  --id <OLD_CONTRACT_ID> \
  -- set_legal_hold --active true --reason "v7→v8 redeploy in progress"
```

**Purpose:** Block any state mutations during redeploy. Saves occur, but payouts/settlements/claims pause.

### Step 3.2: Deploy v8 WASM to Mainnet

1. Upload WASM:
   ```bash
   stellar contract invoke \
     --network mainnet \
     --source <ADMIN_KEY> \
     ... upload-contract --wasm <PATH>/escrow.wasm
   ```

2. Record WASM hash (`<WASM_HASH_v8_MAINNET>`).

3. Verify hash matches testnet/staging (security check):
   ```bash
   if <WASM_HASH_v8_MAINNET> == <WASM_HASH_v8_TESTNET>:
       echo "✅ Mainnet WASM hash matches testnet"
   else:
       echo "❌ MISMATCH - Do not proceed!"
       exit 1
   fi
   ```

### Step 3.3: Initialize New v8 Instances

For each old instance, create a new v8 instance:

```bash
stellar contract invoke \
  --network mainnet \
  --source <ADMIN_KEY> \
  --id <NEW_CONTRACT_ID> \
  -- init \
  --admin <ADMIN_ADDRESS> \
  --invoice_id <OLD_INVOICE_ID> \
  --sme_address <OLD_SME> \
  --amount <OLD_AMOUNT> \
  --yield_bps <OLD_YIELD> \
  ... (other params from old instance)
```

**Guidance:** Use `get_escrow()` on old instance to retrieve all init parameters.

### Step 3.4: Restore Investor Data (if needed)

For funded instances, restore investor contributions + claims:

```bash
# For each investor in the old instance:
stellar contract invoke \
  --network mainnet \
  --source <INVESTOR_KEY> \
  --id <NEW_CONTRACT_ID> \
  -- fund --investor <INVESTOR_ADDRESS> --amount <RESTORED_CONTRIBUTION>
```

**Note:** This is **off-chain coordinated** (your indexer provides the list + amounts).

### Step 3.5: Update Discovery Endpoints

Have API/indexer team update:

1. **Discovery API endpoint** to return new contract IDs:
   ```bash
   GET /api/escrows/{invoice_id}
   # Returns: { contract_id: <NEW_ID>, status: "active", version: 8 }
   ```

2. **Archive old IDs** (optional, for backward compat):
   ```bash
   GET /api/escrows/{invoice_id}/archived
   # Returns: [{ contract_id: <OLD_ID>, status: "archived", version: 7 }]
   ```

### Step 3.6: Notify Investors

Send communication (if applicable):

> **Subject:** Karis Invoice Escrow Contract Upgrade Complete
>
> Dear Investor,
>
> Your escrow instance has been upgraded to contract version 8. Your funds and claims are safe and available at the new address [NEW_ID].
>
> **What changed:** Internal schema improvements for better governance and legal-hold management.
>
> **What you need to do:** Update your bookmarks / UI to use the new contract ID. Existing transactions remain valid.
>
> If you have questions, contact support@example.com.

---

## Phase 4: Production Rollout (1 day)

### Step 4.1: Activate All Prod Legal Holds

```bash
for CONTRACT_ID in <LIST_OF_ALL_PROD_IDS>:
    stellar contract invoke \
      --network mainnet \
      --source <ADMIN_KEY> \
      --id $CONTRACT_ID \
      -- set_legal_hold --active true --reason "v7→v8 redeploy"
done
```

### Step 4.2: Batch Deploy & Initialize New Instances

Use a deployment script to parallelize (if safe):

```bash
# Pseudocode: deploy_v8_batch.sh
for OLD_ID in <LIST>:
    NEW_ID=$(deploy_and_init_v8 $OLD_ID)
    echo "$OLD_ID → $NEW_ID" >> migration_log.txt
done
```

### Step 4.3: Verify All New Instances

```bash
for NEW_ID in <LIST_OF_NEW_IDS>:
    VERSION=$(stellar contract invoke --id $NEW_ID -- get_version)
    HISTORY=$(stellar contract invoke --id $NEW_ID -- get_version_history)
    if VERSION == 8 && HISTORY.len == 1:
        echo "✅ $NEW_ID"
    else:
        echo "❌ $NEW_ID — CHECK MANUALLY"
    fi
done
```

### Step 4.4: Switch Discovery Endpoints (Coordinated)

1. API team publishes new endpoint mapping.
2. UI team deploys client pointing to new endpoint.
3. Indexer team confirms queries route to new IDs.

**Timing:** All three should go live **within 5 minutes** of each other to avoid confusion.

### Step 4.5: Clear Legal Holds

Once verification passes:

```bash
for OLD_ID in <LIST_OF_OLD_IDS>:
    stellar contract invoke \
      --network mainnet \
      --source <ADMIN_KEY> \
      --id $OLD_ID \
      -- set_legal_hold --active false --reason "v7→v8 redeploy complete"
done
```

**Important:** Do not clear the hold until you've verified all new instances are live and integrators are routing correctly.

---

## Phase 5: Monitoring & Reconciliation (72 hours)

### Step 5.1: Monitor New Instances (24/7 for 72h)

1. **Fund operations:** Track success rate
2. **Settle operations:** Track success rate
3. **Claim operations:** Track success rate
4. **Event emissions:** Verify all business events are emitted
5. **Error logs:** Zero unexpected panics

**Dashboard:** Set up alerts for:
- Contract panics (XDR errors, initialization failures)
- Slow transactions (performance regression?)
- Migration issues (if any)

### Step 5.2: Reconciliation Checks

1. **Investor count:** Old instance vs new instance should match
   ```bash
   OLD_UNIQUE=$(stellar contract invoke --id <OLD> -- get_unique_funder_count)
   NEW_UNIQUE=$(stellar contract invoke --id <NEW> -- get_unique_funder_count)
   if OLD_UNIQUE == NEW_UNIQUE:
       echo "✅ Investor count matches"
   fi
   ```

2. **Principal total:** Should match (assuming all investors restored)
   ```bash
   OLD_FUNDED=$(stellar contract invoke --id <OLD> -- get_funded_amount)
   NEW_FUNDED=$(stellar contract invoke --id <NEW> -- get_funded_amount)
   if OLD_FUNDED == NEW_FUNDED:
       echo "✅ Funded amount matches"
   fi
   ```

3. **Version history:** Should show single entry (no unexpected migrations)
   ```bash
   HISTORY=$(stellar contract invoke --id <NEW> -- get_version_history)
   if HISTORY.len == 1 && HISTORY[0].version == 8:
       echo "✅ Version history correct"
   fi
   ```

### Step 5.3: Investor Feedback

1. Monitor support tickets for redeploy-related issues
2. Respond to questions about old contract ID availability
3. Document any issues for future runbook refinement

### Step 5.4: Archive Old Instances

Once monitoring confirms stability:

1. Set legal hold on all old instances (permanent archive):
   ```bash
   for OLD_ID in <LIST>:
       stellar contract invoke \
         --network mainnet \
         --source <ADMIN_KEY> \
         --id $OLD_ID \
         -- set_legal_hold --active true --reason "v7 archived after v8 redeploy"
   done
   ```

2. Update instance registry: mark as `archived: true`

3. Keep old instances on-chain for audit trail (do not delete).

### Step 5.5: Post-Redeploy Sign-Off

- [ ] All new instances operational
- [ ] Zero critical errors in logs
- [ ] Investor count reconciled
- [ ] Principal totals reconciled
- [ ] Version history correct
- [ ] Integrators confirmed working
- [ ] Investors notified
- [ ] Old instances archived

---

## Rollback Plan

**Scenario:** A critical bug is discovered in v8 WASM post-redeploy.

**Options:**

1. **Pause new operations** (if bug is non-critical):
   - Activate legal hold on all new instances
   - Let existing operations complete
   - Deploy v8.1 hotfix and re-init new instances

2. **Emergency revert to v7** (if bug is critical):
   - Activate legal hold on all new v8 instances
   - Direct investor traffic back to old v7 instances (legal hold removed)
   - Do NOT restore investor data to v7 (too risky mid-flight)
   - Create incident report and plan v8.2 fix
   - Redeploy v8.2 when tested

**Note:** There is no on-chain "rollback" entrypoint. Recovery is manual (off-chain processes).

---

## Appendix A: Checklist

### Pre-Redeploy
- [ ] Testnet validation complete
- [ ] Staging rehearsal passed
- [ ] Security review of v8 changes
- [ ] Integrators (indexer, API, UI) deployment-ready
- [ ] Investor notification drafted
- [ ] Admin key(s) available and tested
- [ ] Legal hold entrypoint verified to work on mainnet
- [ ] WASM hash recorded: `<WASM_HASH_v8_MAINNET>`

### During Redeploy
- [ ] Phase 1 legal holds activated
- [ ] Phase 2 new instances deployed
- [ ] Phase 3 investor data restored
- [ ] Phase 4 discovery endpoints updated
- [ ] Phase 5 legal holds cleared
- [ ] All operations verified

### Post-Redeploy (72h)
- [ ] Monitoring dashboard green
- [ ] Reconciliation checks passed
- [ ] Zero critical errors
- [ ] Investor feedback addressed
- [ ] Old instances archived
- [ ] Sign-off obtained

---

## Appendix B: Common Issues & Troubleshooting

| Issue | Cause | Fix |
|-------|-------|-----|
| New instance init fails with XDR error | v8 WASM not on-chain | Verify WASM hash matches uploaded contract |
| `get_escrow` panics with decode error | WASM mismatch | Check network; ensure v8 WASM is active |
| Version history has 2+ entries after init | migrate() was accidentally called | Redeploy new instance (do not call migrate) |
| Investors report missing claims after restoration | Data not restored off-chain | Verify indexer has correct list + amounts |
| Old instance still accepting funds | Legal hold not activated | Call `set_legal_hold(true)` to block |
| Discovery endpoint returns old ID | API deployment didn't complete | Check with API team; re-deploy if needed |

---

## Appendix C: Reference Links

- **Schema changes:** escrow/src/lib.rs (SCHEMA_VERSION, InvoiceEscrow struct)
- **Migration template:** docs/MIGRATION_IMPLEMENTATION_TEMPLATE.md
- **Upgrade decision tree:** docs/UPGRADE_DECISION_TREES.md
- **ADR-007 (additive keys):** docs/adr/ADR-007-storage-key-evolution.md
- **Operator runbook:** docs/OPERATOR_RUNBOOK.md

