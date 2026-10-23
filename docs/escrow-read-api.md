# Escrow Read API

Soroban read-only entry points on `LiquifactEscrow`. All functions take `env: Env` and are
view-only (no state mutation, no auth required).

---

## `get_funding_token() → Address`

**Storage key:** `DataKey::FundingToken`

Returns the SEP-41 token contract address bound to this escrow instance at `init`.

- **Immutable** — set once at `init`; cannot change after deploy.
- Panics with `"Funding token not set"` if called before `init`.
- This is the only token that `sweep_terminal_dust` may transfer to the treasury.

---

## `get_treasury() → Address`

**Storage key:** `DataKey::Treasury`

Returns the protocol treasury address that receives terminal dust sweeps.

- **Immutable** — set once at `init`; cannot change after deploy.
- Panics with `"Treasury not set"` if called before `init`.
- The treasury must authorize `sweep_terminal_dust`; the admin cannot sweep unless it is also the treasury.

---

## `get_registry_ref() → Option<Address>`

**Storage key:** `DataKey::RegistryRef`

Returns the optional registry contract address supplied at `init`, or `None` when absent.

### Non-authority model

`RegistryRef` is a **read-only discoverability hint** for off-chain indexers only.

- No on-chain logic in this contract reads or calls this address.
- Its presence **does not** prove registry membership; call the registry contract directly to
  verify any on-chain claim.
- `None` is a valid, fully operational state — registry integration is optional.
- The key is omitted from instance storage entirely when `registry = None` at `init`, so
  `get_registry_ref()` on an uninitialized contract also returns `None`.

---

## `get_escrow() → InvoiceEscrow`

**Storage key:** `DataKey::Escrow`

Returns the full escrow snapshot. Panics with `"Escrow not initialized"` before `init`.

---

## `get_version() → u32`

**Storage key:** `DataKey::Version`

Returns the current schema version (`SCHEMA_VERSION`). Returns `0` before `init`.

---

## `get_legal_hold_status() → bool`

**Storage key:** `DataKey::LegalHold`

Returns `true` when a compliance hold is active. Defaults to `false` when the key is absent.
This lightweight read-only entrypoint returns only the hold status, without loading the full
`InvoiceEscrow` snapshot. No authorization is required.

The legacy `get_legal_hold() → bool` entrypoint remains available and returns the same value.

---

## `has_maturity_lock() → bool`

Derived from `DataKey::Escrow.maturity`.

Returns `true` when `maturity > 0` and `settle()` is gated by
`Env::ledger().timestamp() >= maturity`. Returns `false` when `maturity == 0`,
which means there is no maturity time lock and a funded escrow can settle
immediately if SME auth, status, and legal-hold checks pass.

---

## `get_min_contribution_floor() → i128`

**Storage key:** `DataKey::MinContributionFloor`

Returns the per-call funding floor in token base units. `0` means no extra floor.

---

## `get_max_unique_investors_cap() → Option<u32>`

**Storage key:** `DataKey::MaxUniqueInvestorsCap`

Returns the optional cap on distinct investor addresses. `None` means unlimited.

---

## `get_max_per_investor_cap() → Option<i128>`

**Storage key:** `DataKey::MaxPerInvestorCap`

Returns the optional immutable cap on cumulative principal for a single investor. `None` means unlimited.

---

## `get_unique_funder_count() → u32`

**Storage key:** `DataKey::UniqueFunderCount`

Returns the count of distinct addresses that have contributed principal. Initialized to `0` at `init`.

---

## `get_investor_cap_status() → InvestorCapStatus`

Returns a self-explanatory, single-call answer to "can more investors contribute?" without requiring
client-side arithmetic on `get_max_unique_investors_cap()` and `get_unique_funder_count()`.

### Use case

Investor-facing UIs need to display whether an escrow has room for new contributions. This entrypoint
eliminates error-prone client-side computation.

### Return Type: `InvestorCapStatus`

A `#[contracttype]` struct containing:

- `max: u32` — Maximum number of distinct investors allowed. Returns `u32::MAX` (4,294,967,295) when no cap is set (unlimited).
- `current: u32` — Current number of distinct investors that have contributed.
- `remaining: u32` — Remaining capacity for new investors, computed as `max - current`.
- `is_full: bool` — `true` when `current == max` (escrow is at capacity for new investors).

### Behavior

- **No cap set:** When `get_max_unique_investors_cap()` returns `None`, `max` is set to `u32::MAX` and `is_full` is always `false`.
- **Cap exists:** When a cap is configured, `remaining = max - current`. When `current >= max`, `is_full = true`.
- **Existing investors:** An escrow at capacity (`is_full = true`) can still accept additional principal from existing investors; the cap applies to **distinct addresses**, not total principal per address.
- **Cap changes:** If the admin calls `lower_max_unique_investors`, the status reflects the new lower cap immediately.

### Examples

```rust
// Escrow with no cap (unlimited investors)
let status = get_investor_cap_status();
// status.max = u32::MAX
// status.current = 42
// status.remaining = u32::MAX - 42
// status.is_full = false

// Escrow with cap of 100, 95 investors so far
let status = get_investor_cap_status();
// status.max = 100
// status.current = 95
// status.remaining = 5
// status.is_full = false

// Escrow with cap of 50, exactly 50 investors
let status = get_investor_cap_status();
// status.max = 50
// status.current = 50
// status.remaining = 0
// status.is_full = true
```

---

## `get_investor_position(investor: Address) → InvestorPosition`

Aggregates every per-investor read into a single host invocation, so indexers and UIs can render
an investor's full position without issuing one call per field. View-only (no auth, no state writes).

### Return Type: `InvestorPosition`

A `#[contracttype]` struct containing:

- `contribution: i128` — Cumulative principal contributed by `investor`. **Storage key:** `DataKey::InvestorContribution(investor)`. `0` when absent.
- `effective_yield_bps: i64` — Effective annualized yield (bps) locked in at the investor's first deposit. **Storage key:** `DataKey::InvestorEffectiveYield(investor)`. Falls back to `InvoiceEscrow::yield_bps` when the key is absent (legacy positions).
- `claim_not_before: u64` — Earliest ledger timestamp at which the investor may call `claim_investor_payout`. **Storage key:** `DataKey::InvestorClaimNotBefore(investor)`. `0` means no extra gate beyond settled status.
- `claimed: bool` — Whether the investor has exercised `claim_investor_payout`. **Storage key:** `DataKey::InvestorClaimed(investor)`. Defaults to `false`.

### Behavior

- **Safe fallback** — each field matches its individual getter exactly, returning the same default when the optional key is absent.
- **No panic on unknown investor** — an address that has never contributed yields an all-default position (`contribution = 0`, `claimed = false`, etc.).
- **Panics only** when the escrow itself is uninitialized, consistent with `get_escrow_summary`.

### Relationship to individual getters

The individual entrypoints remain available and are the source of truth for each field:

- `get_contribution(investor)`
- `get_investor_yield_bps(investor)`
- `get_investor_claim_not_before(investor)`
- `is_investor_claimed(investor)`

`get_investor_position` is a convenience aggregate over these; it introduces no new storage keys.

---

## `get_contribution(investor: Address) → i128`

**Storage key:** `DataKey::InvestorContribution(investor)`

Returns the cumulative principal contributed by `investor`. `0` when absent.

---

## `get_funding_close_snapshot() → Option<FundingCloseSnapshot>`

**Storage key:** `DataKey::FundingCloseSnapshot`

Returns the pro-rata denominator snapshot captured when the escrow first became **funded** (status 1).
`None` until that transition. Immutable once written.

---

## `get_investor_yield_bps(investor: Address) → i64`

**Storage key:** `DataKey::InvestorEffectiveYield(investor)`

Returns the effective annualized yield (bps) locked in at the investor's first deposit.
Falls back to `InvoiceEscrow::yield_bps` when the key is absent (legacy positions).

---

## `get_investor_claim_not_before(investor: Address) → u64`

**Storage key:** `DataKey::InvestorClaimNotBefore(investor)`

Returns the earliest ledger timestamp at which the investor may call `claim_investor_payout`.
`0` means no extra gate beyond settled status.

---

## `get_sme_collateral_commitment() → Option<SmeCollateralCommitment>`

**Storage key:** `DataKey::SmeCollateralPledge`

Returns the SME collateral pledge metadata, or `None` when never recorded.

**Record-only:** this is not an enforced on-chain asset lock.

---

## `is_investor_claimed(investor: Address) → bool`

**Storage key:** `DataKey::InvestorClaimed(investor)`

Returns `true` when the investor has exercised `claim_investor_payout`. Defaults to `false`.

---

## `get_primary_attestation_hash() → Option<BytesN<32>>`

**Storage key:** `DataKey::PrimaryAttestationHash`

Returns the single-set 32-byte attestation digest, or `None` when unbound.

---

## `get_attestation_append_log() → Vec<BytesN<32>>`

**Storage key:** `DataKey::AttestationAppendLog`

Returns the append-only audit chain of digests. Returns an empty `Vec` when no entries exist.
Bounded by `MAX_ATTESTATION_APPEND_ENTRIES`.

---

## `get_escrow_summary() → EscrowSummary`

Bundles multiple read-only values in a single host invocation, optimizing read latency and gas efficiency for off-chain indexers and frontend rendering.

- **Pure Read** — view-only (no authorization required, no state writes).
- **Safe Fallback** — matches individual getters exactly, returning defaults when optional keys are absent, and does not panic unless the escrow itself is uninitialized.

### Return Type: `EscrowSummary`

A `#[contracttype]` struct containing:

- `escrow: InvoiceEscrow` — The full escrow snapshot.
- `has_maturity_lock: bool` — True when `escrow.maturity > 0`; false means `maturity == 0` and settlement has no maturity time lock.
- `legal_hold: bool` — True if a compliance hold is active.
- `funding_close_snapshot: EscrowCloseSnapshot` — Custom option-like representation of the captured pro-rata denominator snapshot (detailed below).
- `unique_funder_count: u32` — Distinct address count of contributors.
- `is_allowlist_active: bool` — True if the investor allowlist is active.
- `allowlist_active: bool` — True if the investor allowlist is active. Mirrors `is_allowlist_active`; provided as a convenience alias for consumers that read the summary field directly.
- `dispute_pause_active: bool` — True when a dispute pause is currently in effect. Reflects the **auto-expiry-aware** state: a pause whose expiry has already passed is reported as inactive even if it was never explicitly cleared.
- `dispute_pause_expiry: Option<u64>` — The ledger timestamp at which the active dispute pause expires, or `None` when no pause is active. Because the value is auto-expiry-aware, this is `None` whenever `dispute_pause_active` is `false`.
- `schema_version: u32` — The schema version of the contract state.
- `sme_collateral_commitment: Option<SmeCollateralCommitment>` — SME collateral pledge metadata, or `None` when never recorded.
- `has_primary_attestation: bool` — True when a primary attestation hash has been bound via `bind_primary_attestation_hash`.
- `attestation_log_length: u32` — Number of entries currently in the attestation append log.

### Sub-type: `EscrowCloseSnapshot`

A `#[contracttype]` enum representing the optional `FundingCloseSnapshot`:

- `None` — Escrow is not yet funded; no close snapshot exists.
- `Some(FundingCloseSnapshot)` — The pro-rata denominator snapshot captured when the escrow first transitioned to **funded**.
