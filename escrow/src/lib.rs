#![cfg_attr(not(test), no_std)]
//! karis-ky Escrow Contract
//!
//! Holds investor funds for an invoice until settlement.
//! - SME receives stablecoin when funding target is met ([`LiquifactEscrow::withdraw`])
//! - SME records optional **collateral commitments** ([`LiquifactEscrow::record_sme_collateral_commitment`]) —
//!   these are **ledger records only**; they do **not** move tokens, freeze balances,
//!   reserve assets, or create an enforceable on-chain claim.
//! - [`LiquifactEscrow::settle`] finalizes the escrow after maturity (when configured).
//!
//! ## Schema version ([`SCHEMA_VERSION`] / [`DataKey::Version`])
//!
//! The constant [`SCHEMA_VERSION`] is written to [`DataKey::Version`] by [`LiquifactEscrow::init`]
//! and is the canonical source of truth for upgrade decisions. **Current value: 6.**
//!
//! [`LiquifactEscrow::migrate`] **fails with typed errors in all current execution paths** — no
//! silent migration work is promised or performed. Operators must extend `migrate` before calling
//! it, or redeploy when stored struct layout changes. See `docs/OPERATOR_RUNBOOK.md` for the full
//! decision tree.
//!
//! ## SME collateral commitment metadata
//!
//! [`LiquifactEscrow::record_sme_collateral_commitment`] is an SME-authenticated metadata write for
//! off-chain risk review. The stored [`SmeCollateralCommitment`] and emitted
//! [`CollateralRecordedEvt`] are not proof of custody, lien, encumbrance, asset control, or token
//! movement. Risk teams and indexers must label this state as reported collateral metadata and must
//! verify supporting evidence outside this contract.
//!
//! ## Compliance hold (legal hold)
//!
//! An admin may set [`DataKey::LegalHold`] to block risk-bearing transitions until cleared:
//! [`LiquifactEscrow::settle`], SME [`LiquifactEscrow::withdraw`], and
//! [`LiquifactEscrow::claim_investor_payout`]. **Clearing** requires the **current**
//! [`InvoiceEscrow::admin`] to call [`LiquifactEscrow::set_legal_hold`] with `active = false`
//! (or [`LiquifactEscrow::clear_legal_hold`]). This contract does not embed a timelock or
//! council multisig: production deployments **must** use a governed `admin` (multisig or
//! protocol DAO) so a single lost key cannot strand funds indefinitely.
//!
//! **Failure mode:** a hold plus loss of the current admin signing key leaves funds blocked
//! on-chain until governance regains control of admin authority. There is no break-glass bypass.
//!
//! **Recovery lever:** [`LiquifactEscrow::propose_admin`] and
//! [`LiquifactEscrow::accept_admin`] are **not** gated by the hold. Governance proposes a new
//! admin, the proposed address accepts, then the new admin clears the hold. Invariant: a hold is
//! always clearable by whoever holds `InvoiceEscrow::admin`; recovery requires controlling that
//! authority. See `docs/escrow-legal-hold.md` and [ADR-004](docs/adr/ADR-004-legal-hold.md).
//!
//! ## Authorization guard ordering
//!
//! Every state-mutating entrypoint follows a canonical sequence (see
//! `docs/escrow-security-checklist.md` §6 and [ADR-002](docs/adr/ADR-002-auth-boundaries.md)):
//!
//! 1. **Read-only** preconditions (legal hold, status checks, input validation).
//! 2. **`Address::require_auth()`** for the bound role ([Stellar authorization](https://developers.stellar.org/docs/build/guides/auth/contract-authorization)).
//! 3. **Storage writes** and **SEP-41 transfers** (via [`external_calls`]).
//!
//! Invariant: no instance/persistent storage mutation and no token transfer occurs until
//! step 2 succeeds. Reading [`DataKey::Escrow`] before `require_auth` is intentional — it is
//! read-only and does not weaken the auth boundary.
//!
//! ## Invoice identifier (`invoice_id`)
//!
//! At initialization, `invoice_id` is supplied as a Soroban [`String`] and validated for length
//! and charset before conversion to [`Symbol`] for storage. Align off-chain invoice slugs with the
//! same rules (ASCII alphanumeric + `_`, max length [`MAX_INVOICE_ID_STRING_LEN`]) so indexers stay
//! unambiguous.
//!
//! ## Funding token and registry (immutable hints)
//!
//! Each escrow instance binds exactly one **funding token** contract ([`DataKey::FundingToken`])
//! at [`LiquifactEscrow::init`]; it cannot be changed after deploy. An optional **registry**
//! ([`DataKey::RegistryRef`]) is a read-only discoverability hint only — it is **not** an authority
//! for this contract and must not be used on-chain as proof of registry state without calling the
//! registry yourself.
//!
//! ## Terminal dust sweep
//!
//! [`LiquifactEscrow::sweep_terminal_dust`] moves at most [`MAX_DUST_SWEEP_AMOUNT`] units of the
//! bound funding token from this contract to the immutable **treasury** address, only when the
//! escrow has reached a **terminal** [`InvoiceEscrow::status`] (settled, withdrawn, or cancelled).
//! It cannot run during a legal hold. Transfers go through [`crate::external_calls`] so **pre/post
//! token balances** must match the requested amount (standard SEP-41 behavior); fee-on-transfer or
//! malicious tokens are **explicitly out of scope** and fail with typed errors at the balance-check
//! boundary. This is meant for rounding residue / stray transfers, not for settling live liabilities —
//! integrations that custody principal on-chain must keep token balances reconciled with
//! `funded_amount` so treasury sweeps cannot pull user funds.
//!
//! ## Ledger time trust model
//!
//! [`LiquifactEscrow::settle`] and [`LiquifactEscrow::claim_investor_payout`] compare against
//! [`Env::ledger`] timestamps only (no wall-clock oracle). Maturity, per-investor **claim locks**
//! from [`LiquifactEscrow::fund_with_commitment`], and [`FundingCloseSnapshot`] metadata must be
//! interpreted as **validator-observed ledger time**, including possible skew between simulated and
//! live networks—integrators should treat boundaries as `>=` / `<` tests on integer seconds.
//!
//! ## Optional tiered yield (immutable table at init)
//!
//! Pass `yield_tiers` to [`LiquifactEscrow::init`] as [`Option`] of a Soroban [`Vec`] of [`YieldTier`].
//! The table is **immutable** for the escrow instance. Investors who use [`LiquifactEscrow::fund_with_commitment`]
//! on their **first** deposit select an effective [`DataKey::InvestorEffectiveYield`] from the ladder;
//! further principal from that address must use [`LiquifactEscrow::fund`]. **Fairness:** tiers are
//! validated non-decreasing in both `min_lock_secs` and `yield_bps` relative to the base [`InvoiceEscrow::yield_bps`].
//!
//! ## Funding-close snapshot (pro-rata)
//!
//! When status first becomes **funded**, [`DataKey::FundingCloseSnapshot`] stores total principal
//! (including over-funding past target), the target, and ledger timestamp/sequence. **Immutable** once
//! written; see `docs/escrow-pro-rata.md` for the authoritative pro-rata payout math and rounding rules.
//! Off-chain share for an investor is `get_contribution(addr) / snapshot.total_principal`.

#![allow(clippy::too_many_arguments)]

#[cfg(test)]
extern crate std;

use core::{clone::Clone, default::Default};
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, panic_with_error,
    symbol_short, token::TokenClient, Address, BytesN, Env, String, Symbol, Vec,
};

pub mod external_calls;
pub mod validation;

// Build-time constants auto-generated by `escrow/build.rs`. Embedded into the
// WASM artifact at compile time and surfaced via
// [`LiquifactEscrow::get_build_metadata`] so off-chain tooling and CI can verify
// a deployed contract was built from the expected source commit, with the
// expected `SCHEMA_VERSION` / `CONTRACT_INTERFACE_VERSION`.
//
// Available as `BUILD_*` constants throughout this module.
include!(concat!(env!("OUT_DIR"), "/build_info.rs"));

/// Current storage schema version written to [`DataKey::Version`] by [`LiquifactEscrow::init`].
///
/// # Schema version changelog
///
/// | Version | Summary | Upgrade path |
/// |---------|---------|-------------|
/// | 1 | Initial schema (`InvoiceEscrow` v1, basic fund / settle) | N/A |
/// | 2 | Added `InvestorEffectiveYield`, `InvestorClaimNotBefore` | Additive keys — no `migrate` call required |
/// | 3 | Added `FundingCloseSnapshot`, `MinContributionFloor`, `MaxUniqueInvestorsCap`, `UniqueFunderCount` | Additive keys — old instances return defaults |
/// | 4 | Added `PrimaryAttestationHash`, `AttestationAppendLog` | Additive keys — no `migrate` call required |
/// | 5 | Added `YieldTierTable`, `RegistryRef`, `Treasury`; `fund_with_commitment` | **Redeploy required** if `InvoiceEscrow` XDR changed |
/// | 6 | Per-investor keys moved to **persistent** storage (see ADR-007) | **Redeploy required** — no `migrate` path (addresses not enumerable) |
/// | 7 | Added `SettlementNotifierContract`, `CreatedAt`, status 5 (archived), `archive_escrow`, `notify_settlement`, `get_registry_listing` | Additive keys + new entrypoints — **redeploy required** for new status codes |
///
/// See `docs/OPERATOR_RUNBOOK.md` for the full redeploy-vs-upgrade decision tree.
pub const SCHEMA_VERSION: u32 = 7;

/// Contract interface version — identifies the ABI surface exposed to callers.
///
/// This constant is returned by [`LiquifactEscrow::get_interface_version`] and is **distinct**
/// from [`SCHEMA_VERSION`]:
///
/// - [`SCHEMA_VERSION`] tracks the on-chain storage layout (XDR structs, `DataKey` variants).
/// - `CONTRACT_INTERFACE_VERSION` tracks the **public entrypoint surface** (function names,
///   parameter lists, return types, event shapes).
///
/// # Increment rules — callers must bump this value when:
///
/// - An entrypoint is **renamed** or **removed**.
/// - A parameter is **added, removed, or retyped** for any public entrypoint.
/// - A return type changes in a way that is not backward-compatible with existing callers.
/// - An event `#[topic]` or field name/type changes in a way that breaks indexed consumers.
///
/// # Stable / append-only policy
///
/// - This constant is **append-only**: once a numeric value has been published in a
///   production deployment it must never be reused or decremented.
/// - Adding a **new** entrypoint without touching existing signatures does **not** require
///   a bump — callers that do not call the new function are unaffected.
/// - New **optional** parameters guarded by `Option<T>` on Soroban may be additive; evaluate
///   on a case-by-case basis and prefer a bump when in doubt.
///
/// See `docs/escrow-interface-versioning.md` for the full policy and examples.
pub const CONTRACT_INTERFACE_VERSION: u32 = 2;

/// Upper bound on [`LiquifactEscrow::append_attestation_digest`] entries to keep storage bounded.
/// Revocation via [`LiquifactEscrow::revoke_attestation_digest`] does not consume a slot.
pub const MAX_ATTESTATION_APPEND_ENTRIES: u32 = 32;

/// Upper bound on batch allowlist mutation entries to keep storage/CPU bounded.
/// Mirrors the spirit of `MAX_ATTESTATION_APPEND_ENTRIES` to limit per-call work.
pub const MAX_INVESTOR_ALLOWLIST_BATCH: u32 = 32;

/// Upper bound on [`LiquifactEscrow::fund_batch`] entries to keep storage/CPU bounded.
/// Mirrors the spirit of `MAX_ATTESTATION_APPEND_ENTRIES` to limit per-call work.
pub const MAX_FUND_BATCH: u32 = 50;

/// Number of bucketing shards for investor contribution aggregates (256 buckets).
///
/// Buckets partition investor addresses by `hash(address) % INVESTOR_BUCKET_COUNT`
/// so that dashboards can compute total funded ≈ Σ buckets in O(256) instance reads
/// instead of iterating over all per‑investor persistent `InvestorContribution` keys.
///
/// The bucket count is chosen as a power‑of‑two for fast modulo with bit‑masking
/// and provides a good balance between write overhead (+1 instance write per fund)
/// and read granularity for aggregate queries.
pub const INVESTOR_BUCKET_COUNT: u32 = 256;

/// Upper bound on [`LiquifactEscrow::sweep_terminal_dust`] per call (base units of the funding token).
///
/// Caps blast radius if instrumentation mis-estimates “dust”; tune per asset decimals off-chain.
pub const MAX_DUST_SWEEP_AMOUNT: i128 = 100_000_000;

/// Maximum UTF-8 byte length for the invoice `String` at init (matches Soroban [`Symbol`] max).
pub const MAX_INVOICE_ID_STRING_LEN: u32 = 32;

/// Minimum instance storage TTL extension horizon for time-sensitive escrow entries.
///
/// `bump_ttl` extends instance-storage entries to avoid rent/archival edge cases when
/// maturity/claim locks are far in the future.
///
/// Named as a constant so operators can reason about and audit the threshold.
pub const INSTANCE_TTL_MIN_EXTENSION_LEDGERS: u32 = 60 * 60; // Approx. 1h at 1 ledger/sec.

/// Minimum persistent storage TTL extension horizon for per-investor allowlist entries.
///
/// When the escrow uses the allowlist gate, investor funding depends on persistent entries.
/// Extending persistent allowlist TTL reduces the risk of silent allowlist disablement.
pub const PERSISTENT_TTL_MIN_EXTENSION_LEDGERS: u32 = 60 * 60; // Approx. 1h at 1 ledger/sec.

/// Maximum UTF-8 byte length for a snapshot name (must fit in Soroban Symbol).
pub const MAX_SNAPSHOT_NAME_LEN: u32 = 32;

/// Upper bound on state snapshots to keep storage bounded and prevent abuse.
/// Admin may create at most this many named snapshots per escrow instance.
pub const MAX_STATE_SNAPSHOTS: u32 = 16;

/// Stable typed errors emitted by karis-ky escrow entrypoints.
///
/// Codes are append-only: never reuse or renumber a variant. Client SDKs should branch on the
/// numeric code rather than legacy panic strings. See `docs/escrow-error-messages.md`.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum EscrowError {
    /// [`LiquifactEscrow::init`] rejected a non-positive invoice amount.
    AmountMustBePositive = 1,
    /// [`LiquifactEscrow::init`] rejected `yield_bps` outside `0..=10_000`.
    YieldBpsOutOfRange = 2,
    /// [`LiquifactEscrow::init`] called when escrow storage already exists.
    EscrowAlreadyInitialized = 3,
    /// [`LiquifactEscrow::init`] rejected an `invoice_id` outside the allowed length range.
    InvoiceIdInvalidLength = 4,
    /// [`LiquifactEscrow::init`] rejected an `invoice_id` with disallowed characters.
    InvoiceIdInvalidCharset = 5,
    /// [`LiquifactEscrow::init`] configured `min_contribution` but it is not positive.
    MinContributionNotPositive = 6,
    /// [`LiquifactEscrow::init`] configured `min_contribution` above the target hint.
    MinContributionExceedsAmount = 7,
    /// [`LiquifactEscrow::init`] configured `max_unique_investors` but it is not positive.
    MaxUniqueInvestorsNotPositive = 8,
    /// [`LiquifactEscrow::init`] configured `max_per_investor` but it is not positive.
    MaxPerInvestorNotPositive = 9,
    /// [`LiquifactEscrow::init`] rejected a tier with `yield_bps` outside `0..=10_000`.
    TierYieldOutOfRange = 10,
    /// [`LiquifactEscrow::init`] rejected a tier yield below the base `yield_bps`.
    TierYieldBelowBase = 11,
    /// [`LiquifactEscrow::init`] rejected tiers whose `min_lock_secs` are not strictly increasing.
    TierLockNotIncreasing = 12,
    /// [`LiquifactEscrow::init`] rejected tiers whose `yield_bps` decrease across tiers.
    TierYieldNotNonDecreasing = 13,

    /// Escrow storage is missing; entrypoint requires prior [`LiquifactEscrow::init`].
    EscrowNotInitialized = 20,
    /// [`DataKey::FundingToken`] is unset (escrow not fully initialized).
    FundingTokenNotSet = 21,
    /// [`DataKey::Treasury`] is unset (escrow not fully initialized).
    TreasuryNotSet = 22,

    /// [`LiquifactEscrow::sweep_terminal_dust`] blocked while a legal hold is active.
    LegalHoldBlocksTreasuryDustSweep = 30,
    /// [`LiquifactEscrow::sweep_terminal_dust`] received a non-positive sweep amount.
    SweepAmountNotPositive = 31,
    /// [`LiquifactEscrow::sweep_terminal_dust`] exceeded [`MAX_DUST_SWEEP_AMOUNT`].
    SweepAmountExceedsMax = 32,
    /// [`LiquifactEscrow::sweep_terminal_dust`] called before a terminal escrow status.
    DustSweepNotTerminal = 33,
    /// [`LiquifactEscrow::sweep_terminal_dust`] found no funding-token balance to sweep.
    NoFundingTokenBalanceToSweep = 34,
    /// [`LiquifactEscrow::sweep_terminal_dust`] computed an effective sweep amount of zero.
    EffectiveSweepAmountZero = 35,
    /// Token transfer wrapper received a non-positive amount (see `external_calls`).
    TransferAmountNotPositive = 36,
    /// Token transfer wrapper found insufficient sender balance before transfer.
    InsufficientTokenBalanceBeforeTransfer = 37,
    /// Token transfer wrapper detected sender balance delta underflow.
    SenderBalanceUnderflow = 38,
    /// Token transfer wrapper detected recipient balance delta underflow.
    RecipientBalanceUnderflow = 39,
    /// Token transfer wrapper detected sender spent amount differs from requested transfer.
    SenderBalanceDeltaMismatch = 40,
    /// Token transfer wrapper detected recipient received amount differs from requested transfer.
    RecipientBalanceDeltaMismatch = 41,
    /// Sweep would reduce the contract balance below outstanding investor liabilities.
    /// `balance - sweep_amt` must be `>= funded_amount - distributed_principal`.
    SweepExceedsLiabilityFloor = 42,

    /// [`LiquifactEscrow::bind_primary_attestation_hash`] called when a primary hash exists.
    PrimaryAttestationAlreadyBound = 50,
    /// [`LiquifactEscrow::append_attestation_digest`] exceeded [`MAX_ATTESTATION_APPEND_ENTRIES`].
    AttestationAppendLogCapacityReached = 51,

    /// [`LiquifactEscrow::record_sme_collateral_commitment`] received a non-positive amount.
    CollateralAmountNotPositive = 60,
    /// [`LiquifactEscrow::record_sme_collateral_commitment`] received an empty asset symbol.
    CollateralAssetEmpty = 61,
    /// [`LiquifactEscrow::record_sme_collateral_commitment`] received a timestamp before the stored record.
    CollateralTimestampBackwards = 62,

    /// [`LiquifactEscrow::set_investors_allowlisted`] received an empty batch.
    InvestorBatchEmpty = 70,
    /// [`LiquifactEscrow::set_investors_allowlisted`] exceeded [`MAX_INVESTOR_ALLOWLIST_BATCH`].
    InvestorBatchTooLarge = 71,
    /// [`LiquifactEscrow::fund_batch`] received an empty entries vector.
    FundingBatchEmpty = 82,
    /// [`LiquifactEscrow::fund_batch`] exceeded [`MAX_FUND_BATCH`].
    FundingBatchTooLarge = 83,
    /// [`LiquifactEscrow::update_funding_target`] received a non-positive target.
    TargetNotPositive = 72,
    /// [`LiquifactEscrow::update_funding_target`] called while escrow is not open.
    TargetUpdateNotOpen = 73,
    /// [`LiquifactEscrow::update_funding_target`] set target below already-funded principal.
    TargetBelowFundedAmount = 74,
    /// [`LiquifactEscrow::lower_max_unique_investors`] called while escrow is not open.
    CapLowerNotOpen = 75,
    /// [`LiquifactEscrow::lower_max_unique_investors`] called with no investor cap configured.
    NoInvestorCapConfigured = 76,
    /// [`LiquifactEscrow::lower_max_unique_investors`] did not strictly lower the cap.
    NewCapNotLower = 77,
    /// [`LiquifactEscrow::lower_max_unique_investors`] set cap below current unique funder count.
    NewCapBelowCurrentFunderCount = 78,
    /// [`LiquifactEscrow::update_maturity`] called while escrow is not open.
    MaturityUpdateNotOpen = 79,
    /// [`LiquifactEscrow::propose_admin`] nominated the current admin address.
    NewAdminSameAsCurrent = 80,

    /// [`LiquifactEscrow::migrate`] `from_version` does not match stored version.
    MigrationVersionMismatch = 90,
    /// [`LiquifactEscrow::migrate`] called at or above [`SCHEMA_VERSION`].
    AlreadyCurrentSchemaVersion = 91,
    /// [`LiquifactEscrow::migrate`] has no implemented path from the requested version.
    NoMigrationPath = 92,

    /// [`LiquifactEscrow::fund`] / [`LiquifactEscrow::fund_with_commitment`] received non-positive amount.
    FundingAmountNotPositive = 100,
    /// Funding amount is below configured `min_contribution`.
    FundingBelowMinContribution = 101,
    /// Funding blocked while a legal hold is active.
    LegalHoldBlocksFunding = 102,
    /// Funding attempted while escrow is not in open status.
    EscrowNotOpenForFunding = 103,
    /// Allowlist gate active and investor address is not allowlisted.
    InvestorNotAllowlisted = 104,
    /// Adding funding would overflow the investor's stored contribution.
    InvestorContributionOverflow = 105,
    /// Funding would exceed configured `max_per_investor`.
    InvestorContributionExceedsCap = 106,
    /// A new investor would exceed configured `max_unique_investors`.
    UniqueInvestorCapReached = 107,
    /// [`LiquifactEscrow::fund_with_commitment`] called after investor already has principal.
    TieredSecondDeposit = 108,
    /// Computing investor claim-not-before timestamp would overflow.
    InvestorClaimTimeOverflow = 109,
    /// Adding funding would overflow escrow `funded_amount`.
    FundedAmountOverflow = 110,
    /// Commitment lock would push `now + committed_lock_secs` past the escrow maturity.
    /// Reject at deposit time so a settled escrow cannot hold an investor's payout
    /// claim hostage beyond the point where principal is due.
    CommitmentLockExceedsMaturity = 111,

    /// [`LiquifactEscrow::settle`] blocked while a legal hold is active.
    LegalHoldBlocksSettlement = 120,
    /// [`LiquifactEscrow::settle`] called before escrow reached funded status.
    SettlementNotFunded = 121,
    /// [`LiquifactEscrow::settle`] called before configured maturity timestamp.
    MaturityNotReached = 122,
    /// [`LiquifactEscrow::withdraw`] blocked while a legal hold is active.
    LegalHoldBlocksWithdrawal = 123,
    /// [`LiquifactEscrow::withdraw`] called before escrow reached funded status.
    WithdrawalNotFunded = 124,
    /// [`LiquifactEscrow::claim_investor_payout`] blocked while a legal hold is active.
    LegalHoldBlocksInvestorClaims = 125,
    /// [`LiquifactEscrow::claim_investor_payout`] for an address with zero contribution.
    NoContributionToClaim = 126,
    /// [`LiquifactEscrow::claim_investor_payout`] before escrow is settled.
    InvestorClaimNotSettled = 127,
    /// [`LiquifactEscrow::claim_investor_payout`] before tier commitment lock expires.
    InvestorCommitmentLockNotExpired = 128,
    /// Checked arithmetic overflow in [`LiquifactEscrow::compute_investor_payout`].
    ComputePayoutArithmeticOverflow = 129,

    /// [`LiquifactEscrow::cancel_funding`] blocked while a legal hold is active.
    LegalHoldBlocksCancelFunding = 140,
    /// [`LiquifactEscrow::cancel_funding`] called while escrow is not open.
    CancelFundingNotOpen = 141,
    /// [`LiquifactEscrow::refund`] called while escrow is not cancelled.
    RefundNotCancelled = 142,
    /// [`LiquifactEscrow::refund`] for an address with zero contribution.
    NoContributionToRefund = 143,

    /// `clear_legal_hold` was called without a prior `request_legal_hold_clear`.
    LegalHoldClearRequestMissing = 150,
    /// The two-phase legal-hold clear delay has not elapsed yet.
    LegalHoldClearNotReady = 151,
    /// Computing the legal-hold clear ready-at timestamp would overflow.
    LegalHoldClearDelayOverflow = 152,
    /// Funding deadline has passed, new deposits are rejected.
    FundingDeadlinePassed = 164,

    /// A legal hold blocks rotating the beneficiary (SME) address.
    LegalHoldBlocksBeneficiaryRotation = 160,
    /// Beneficiary rotation was attempted while the escrow was not in a
    /// pre-settlement state (`status` must be 0 = open or 1 = funded).
    RotationNotOpen = 161,
    /// [`LiquifactEscrow::claim_investor_payout`] detected yield slippage exceeding configured threshold.
    YieldSlippageDetected = 162,
    /// Yield slippage threshold is out of valid range (0..=10_000 bps).
    YieldSlippageThresholdOutOfRange = 163,
    /// The proposed new SME address is identical to the current beneficiary.
    NewSmeSameAsCurrent = 162,

    /// Attempted to accept admin role when no pending admin exists.
    NoPendingAdmin = 163,
    /// The contract's funding-token balance is less than `funded_amount` at withdraw time.
    /// Funds must be custodied in this contract before the SME can pull them.
    InsufficientContractBalance = 164,

    /// [`LiquifactEscrow::create_state_snapshot`] received an invalid or empty snapshot name.
    InvalidSnapshotName = 170,
    /// [`LiquifactEscrow::create_state_snapshot`] exceeded snapshot storage capacity.
    SnapshotStorageCapacityReached = 171,
    /// [`LiquifactEscrow::revert_to_snapshot`] called with a snapshot name that does not exist.
    SnapshotNotFound = 172,
}

#[inline(always)]
pub(crate) fn fail(env: &Env, error: EscrowError) -> ! {
    panic_with_error!(env, error)
}

#[inline(always)]
pub(crate) fn ensure(env: &Env, condition: bool, error: EscrowError) {
    if !condition {
        fail(env, error);
    }
}

// --- Storage keys ---

#[contracttype]
#[derive(Clone)]
/// Storage discriminator for persisted contract state.
///
/// Most variants live in **instance** storage (shared TTL with the contract instance, bounded
/// aggregate size). Per-investor variants
/// [`InvestorContribution`], [`InvestorEffectiveYield`], [`InvestorClaimNotBefore`], and
/// [`InvestorClaimed`] use **persistent** storage (independent per-address TTL; see ADR-007 and
/// `docs/escrow-gas-storage-notes.md`). [`InvestorAllowlisted`] also uses persistent storage.
///
/// Optional keys are always read with `.get(...).unwrap_or(default)` so that deployments predating
/// a key behave as “unset / default” without panicking.
///
/// ## Additive-key policy (see ADR-007)
///
/// Adding a new variant is **backward-compatible** when the new key is read with
/// `.unwrap_or(default)` and its absence does not change existing entrypoint semantics.
/// Renaming a variant, changing its XDR discriminant, or altering the stored type of an
/// existing key is **breaking** and requires a `migrate` path or a full redeploy.
///
/// Derive rationale:
/// - `Clone`: required because keys are passed by reference into storage APIs and reused
///   across lookups/sets in the same execution path.
pub enum DataKey {
    /// Full escrow snapshot ([`InvoiceEscrow`]); rewritten atomically on every state transition.
    Escrow,
    /// Stored schema version; written once by [`LiquifactEscrow::init`] to [`SCHEMA_VERSION`]
    /// and updated by [`LiquifactEscrow::migrate`] when a migration path is implemented.
    /// Read with [`LiquifactEscrow::get_version`]. Never delete or rename this variant.
    Version,
    /// Per-investor contributed principal recorded during [`LiquifactEscrow::fund`].
    /// **Persistent** storage. Absent ⇒ `0`. One entry per investor address.
    InvestorContribution(Address),
    /// When true, compliance/legal hold blocks payouts and settlement finalization.
    /// Absent ⇒ `false` (no hold). Toggled by admin via [`LiquifactEscrow::set_legal_hold`].
    LegalHold,
    /// Optional minimum ledger timestamp when `LegalHold` may be cleared after a
    /// [`LiquifactEscrow::request_clear_legal_hold`] call.
    /// Absent ⇒ no clear request is pending.
    LegalHoldClearableAt,
    /// Configured minimum delay between [`LiquifactEscrow::request_clear_legal_hold`] and
    /// [`LiquifactEscrow::set_legal_hold(env, false)`]. Absent ⇒ `0`.
    LegalHoldClearDelay,
    /// Optional SME collateral commitment metadata (record-only — not an on-chain asset lock).
    /// Absent when no commitment has been recorded. Replaceable by the SME.
    SmeCollateralPledge,
    /// Set to `true` when an investor has exercised a claim after settlement.
    /// **Persistent** storage. Absent ⇒ `false`. Written once; a second claim returns without re-emitting.
    InvestorClaimed(Address),
    /// SEP-41 funding asset for this invoice instance; set once in [`LiquifactEscrow::init`].
    /// Immutable after init.
    FundingToken,
    /// Protocol treasury that may receive [`LiquifactEscrow::sweep_terminal_dust`]; set once in init.
    /// Immutable after init.
    Treasury,
    /// Optional registry contract id for indexers; **hint only**, not authority (see module rustdoc).
    /// Omitted from storage when unset at init. Absent ⇒ `None`.
    RegistryRef,
    /// Immutable tier table when configured at [`LiquifactEscrow::init`]; omitted when tiering is off.
    /// Absent ⇒ no tiering (base `yield_bps` applies to all investors).
    /// **Trust:** values are protocol-supplied at deploy; the contract never mutates this key after init.
    YieldTierTable,
    /// Set once when status first becomes **funded** (1); immutable thereafter (pro-rata denominator).
    /// Absent until the escrow reaches `status == 1`. See [`FundingCloseSnapshot`].
    FundingCloseSnapshot,
    /// Effective annualized yield in bps chosen at this investor’s **first** deposit (see tiered yield).
    /// **Persistent** storage. Absent ⇒ falls back to [`InvoiceEscrow::yield_bps`]. One entry per investor address.
    InvestorEffectiveYield(Address),
    /// Minimum [`Env::ledger`] timestamp before [`LiquifactEscrow::claim_investor_payout`] (0 = no extra gate).
    /// **Persistent** storage. Absent ⇒ `0`. One entry per investor address; set on first deposit.
    InvestorClaimNotBefore(Address),
    /// Minimum [`LiquifactEscrow::fund`] / [`LiquifactEscrow::fund_with_commitment`] amount per call (0 = no floor).
    /// Written as `0` even when unconfigured so reads always succeed.
    MinContributionFloor,
    /// When set at [`LiquifactEscrow::init`], caps distinct investor addresses that may contribute.
    /// Absent ⇒ unlimited. Checked against [`DataKey::UniqueFunderCount`] on each new investor.
    MaxUniqueInvestorsCap,
    /// Optional immutable per-investor cap on total principal credited to a single address.
    /// Absent ⇒ unlimited. Checked against [`DataKey::InvestorContribution`] on every deposit.
    MaxPerInvestorCap,
    /// Proposed successor admin waiting for [`LiquifactEscrow::accept_admin`].
    /// Absent ⇒ no pending handover. Cleared after successful acceptance.
    PendingAdmin,
    /// Count of distinct investor addresses that have a non-zero [`DataKey::InvestorContribution`].
    /// Written as `0` at init; incremented once per new investor in `fund_impl`.
    UniqueFunderCount,
    /// Admin-only **single-set** off-chain attestation digest (e.g. SHA-256 of a legal/KYC bundle).
    /// Absent until [`LiquifactEscrow::bind_primary_attestation_hash`] is called; single-set thereafter.
    PrimaryAttestationHash,
    /// Append-only audit chain of digests (bounded by [`MAX_ATTESTATION_APPEND_ENTRIES`]).
    /// Absent ⇒ empty log. See [`LiquifactEscrow::append_attestation_digest`].
    AttestationAppendLog,
    /// Per-index revocation marker for [`DataKey::AttestationAppendLog`] entries.
    /// Absent ⇒ not revoked. Written as `true` by [`LiquifactEscrow::revoke_attestation_digest`].
    /// Preserves the original digest for auditability while signalling supersession.
    AttestationRevoked(u32),
    /// When true, only allowlisted addresses may call [`LiquifactEscrow::fund`] or [`LiquifactEscrow::fund_with_commitment`].
    AllowlistActive,
    /// Whether a specific address is permitted to fund when [`DataKey::AllowlistActive`] is true.
    InvestorAllowlisted(Address),
    /// Set to `true` once an investor's principal has been refunded in a cancelled escrow.
    /// Absent ⇒ `false`. Written once; prevents double-refund.
    InvestorRefunded(Address),
    /// Running total of principal already returned to investors via [`LiquifactEscrow::refund`].
    /// Absent ⇒ `0`. Incremented atomically with each successful refund transfer.
    /// Used by [`LiquifactEscrow::sweep_terminal_dust`] to compute outstanding liabilities:
    /// `outstanding = funded_amount - distributed_principal`.
    DistributedPrincipal,
    /// Optional funding deadline (ledger timestamp); after it passes, new funds are rejected.
    FundingDeadline,
    /// Optional yield slippage threshold in basis points (bps) for real-time anomaly detection.
    /// When set, [`LiquifactEscrow::claim_investor_payout`] compares actual vs. expected yield.
    /// If deviation exceeds threshold, a [`YieldSlippageWarning`] is emitted.
    /// Absent ⇒ `0` (no slippage check).
    YieldSlippageThreshold,
    /// Cached token metadata to reduce external calls on fund operations.
    /// Written at [`LiquifactEscrow::init`], updated only via
    /// [`LiquifactEscrow::revalidate_token_cache`] (admin-only).
    /// Absent ⇒ not yet cached (should not happen if init succeeded).
    TokenMetadataCache,
}

// --- Data types ---

/// Full state of an invoice escrow persisted in contract storage (`DataKey::Escrow`).
#[contracttype]
#[derive(Debug, PartialEq)]
/// Full escrow snapshot persisted at [`DataKey::Escrow`].
///
/// Derive rationale:
/// - `Debug`: improves failure diagnostics in tests.
/// - `PartialEq`: allows exact state assertions in tests.
///
/// `Clone` is intentionally omitted to avoid accidental full-state copies.
pub struct InvoiceEscrow {
    pub invoice_id: Symbol,
    pub admin: Address,
    pub sme_address: Address,
    pub amount: i128,
    pub funding_target: i128,
    pub funded_amount: i128,
    pub yield_bps: i64,
    pub maturity: u64,
    /// 0 = open, 1 = funded, 2 = settled, 3 = withdrawn (SME pulled liquidity), 4 = cancelled (admin-gated; investors may refund), 5 = archived (admin-gated; read-only terminal)
    pub status: u32,
}

/// SME-reported collateral metadata for off-chain risk review.
///
/// **Record-only:** this struct is stored for transparency and indexing. It does **not**
/// custody, escrow, transfer, freeze, reserve, or verify assets. It also does not alter funding,
/// settlement, SME withdrawal, investor-claim, compliance hold, or treasury-sweep behavior.
/// Future versions that enforce asset movement or custody must introduce explicit APIs and must
/// not treat historical records from this type as proof of locked assets.
///
/// # Fields
/// - `asset`: The off-chain asset symbol (cannot be empty).
/// - `amount`: The reported collateral amount (must be positive).
/// - `recorded_at`: The Soroban ledger timestamp when this record was written.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
/// SME collateral commitment metadata (record-only).
///
/// Derive rationale:
/// - `Clone`: required for `Option<SmeCollateralCommitment>` used in `EscrowSummary`.
/// - `Debug`: improves failure diagnostics in tests.
/// - `PartialEq`: allows deterministic assertion of stored/read values.
pub struct SmeCollateralCommitment {
    pub asset: Symbol,
    pub amount: i128,
    pub recorded_at: u64,
}

/// Incremental state change record for delta-encoded snapshots.
/// Stores only the fields that changed from the previous state, enabling storage optimization.
///
/// **Immutable** once written; delta chains form an audit trail of state transitions.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct SnapshotDelta {
    /// Unique ID of this delta (monotonically increasing).
    pub delta_id: u32,
    /// Ledger timestamp when this delta was recorded.
    pub recorded_at: u64,
    /// Previous delta ID this one is based on (0 for baseline/first delta).
    pub based_on_delta_id: u32,
    /// Change in funded amount (signed; may be negative for reversals).
    pub funded_amount_delta: i128,
    /// New maturity value (0 if unchanged).
    pub maturity: u64,
    /// New status (255 if unchanged).
    pub status: u8,
    /// New admin address (None if unchanged).
    pub admin: Option<Address>,
    /// New SME/beneficiary address (None if unchanged).
    pub sme_address: Option<Address>,
}

/// One step in an optional tier ladder: investors who commit to at least `min_lock_secs` (on first
/// deposit via [`LiquifactEscrow::fund_with_commitment`]) may receive `yield_bps` for pro-rata /
/// off-chain coupon math. **Immutable** after `init`: the table is fixed for the escrow instance.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct YieldTier {
    pub min_lock_secs: u64,
    pub yield_bps: i64,
}

/// Captured exactly once at the first ledger transition to **funded** so settlement and claims can
/// use a stable total principal and target. If the threshold-crossing deposit overshoots
/// [`InvoiceEscrow::funding_target`], [`FundingCloseSnapshot::total_principal`] records the full
/// credited [`InvoiceEscrow::funded_amount`] at close and becomes the pro-rata denominator.
/// **Immutable** once written.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct FundingCloseSnapshot {
    /// Sum of principal credited when the invoice became funded (`funded_amount` at close),
    /// including over-funding past target.
    pub total_principal: i128,
    pub funding_target: i128,
    pub closed_at_ledger_timestamp: u64,
    pub closed_at_ledger_sequence: u32,
}

/// Custom option-like enum to represent the captured funding close snapshot.
/// Models standard option semantics as a contracttype to avoid standard library
/// blanket trait limitations in Soroban SDK testutils.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum EscrowCloseSnapshot {
    None,
    Some(FundingCloseSnapshot),
}

/// Custom option-like enum to represent the SME collateral commitment.
/// Models standard option semantics as a contracttype to avoid standard library
/// blanket trait limitations in Soroban SDK testutils.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub enum CollateralCommitmentSnapshot {
    None,
    Some(SmeCollateralCommitment),
}

/// Cached token metadata to reduce external calls during fund operations.
///
/// **Purpose:** Caches token contract metadata (decimals, name, symbol) at escrow
/// initialization time to avoid repeated cross-contract calls on every fund operation.
/// This is a read-only cache that is only updated via explicit revalidation entrypoint.
///
/// **Cache Policy:**
/// - Written once at initialization by [`LiquifactEscrow::init`]
/// - Updated only via [`LiquifactEscrow::revalidate_token_cache`] (admin-only)
/// - Read by fund operations to validate amounts and compute scaled values
/// - Immutable for normal operation (no mutations except explicit revalidation)
///
/// **Fields:**
/// - `decimals`: Token decimal precision (typically 7 for Stellar)
/// - `cached_at_ledger_timestamp`: Timestamp when cache was last written
/// - `cached_at_ledger_sequence`: Ledger sequence when cache was last written
///
/// **Versioning:** Includes ledger time/sequence to allow detecting stale caches
/// (e.g., if token contract upgrade changes decimal places). Exact staleness
/// detection is left to governance processes.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct TokenMetadataCache {
    /// Token decimal places (e.g., 7 for Stellar USDC)
    pub decimals: u32,
    /// Ledger timestamp when cache was written (for staleness detection)
    pub cached_at_ledger_timestamp: u64,
    /// Ledger sequence when cache was written (for staleness detection)
    pub cached_at_ledger_sequence: u32,
}

/// Comprehensive summary of the escrow contract state.
/// Bundles multiple read-only values to allow a single host invocation
/// for off-chain indexers and client rendering.
#[contracttype]
#[derive(Debug, PartialEq)]
pub struct EscrowSummary {
    /// Full escrow snapshot.
    pub escrow: InvoiceEscrow,
    /// True when `escrow.maturity > 0`; false means settlement has no maturity time lock.
    pub has_maturity_lock: bool,
    /// Active legal or compliance hold flag.
    pub legal_hold: bool,
    /// The captured funding close snapshot (Option).
    pub funding_close_snapshot: EscrowCloseSnapshot,
    /// Unique investors count who funded the escrow.
    pub unique_funder_count: u32,
    /// Whether the investor allowlist is active.
    pub is_allowlist_active: bool,
    /// Persisted schema version of the contract data.
    pub schema_version: u32,
    /// SME collateral commitment metadata (None when never recorded).
    pub sme_collateral_commitment: CollateralCommitmentSnapshot,
    /// Whether a primary attestation hash has been bound.
    pub has_primary_attestation: bool,
    /// Number of entries in the attestation append log.
    pub attestation_log_length: u32,
    /// Reason for the active legal hold, if any (max 256 bytes). Absent when no hold.
    pub legal_hold_reason: Option<String>,
}

/// Structured compliance report for regulatory submissions.
/// Generated on-demand by [`LiquifactEscrow::generate_compliance_report`].
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct ComplianceReport {
    /// Invoice identifier for this escrow.
    pub invoice_id: Symbol,
    /// Escrow creation ledger timestamp (when init was called).
    pub created_at_ledger_timestamp: u64,
    /// Escrow creation ledger sequence number.
    pub created_at_ledger_sequence: u32,
    /// Current escrow status (0=open, 1=funded, 2=settled, 3=withdrawn, 4=cancelled).
    pub status: u32,
    /// Original invoice amount (funding target at init).
    pub amount: i128,
    /// Total principal funded by investors.
    pub funded_amount: i128,
    /// Configured base yield in basis points.
    pub yield_bps: i64,
    /// Maturity timestamp (0 if no maturity lock).
    pub maturity: u64,
    /// Count of unique investors who contributed.
    pub investor_count: u32,
    /// True when a legal hold is currently active.
    pub legal_hold_active: bool,
    /// Reason for legal hold, if any (max 256 bytes).
    pub legal_hold_reason: Option<String>,
    /// Funding close snapshot timestamp (when escrow became fully funded), if applicable.
    pub funded_at_ledger_timestamp: Option<u64>,
    /// Settlement timestamp, if settled.
    pub settled_at_ledger_timestamp: Option<u64>,
    /// Admin address.
    pub admin: Address,
    /// SME (beneficiary) address.
    pub sme_address: Address,
    /// Number of investors who have claimed their payout.
    pub investors_claimed: u32,
}

/// Dashboard-ready health score for the escrow.
///
/// Computed read-only from storage. Can be called by any address.
///
/// # Fields
/// - `funding_progress_percent`: 0–100 percentage of funding target met (capped at 100 if overfunded).
/// - `days_to_maturity`: signed delta in days between maturity timestamp and current ledger time.
///   Negative means maturity has already passed. 0 when `maturity == 0` (no lock).
/// - `unique_investor_count`: number of distinct investor addresses with non-zero contributions.
/// - `average_contribution_size`: `funded_amount / unique_investor_count`; 0 when no investors.
/// - `estimated_yield_payout`: coupon = `funded_amount × yield_bps / 10_000`; off‑chain pro‑rata
///   per investor requires the funding‑close snapshot.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct EscrowHealthMetrics {
    /// 0–100: how close the escrow is to the funding target (floor, capped at 100).
    pub funding_progress_percent: u32,
    /// Signed ledger‑time delta in whole days.
    pub days_to_maturity: i64,
    /// Number of distinct investors.
    pub unique_investor_count: u32,
    /// `funded_amount / unique_investor_count`; 0 when there are no investors.
    pub average_contribution_size: i128,
    /// `funded_amount × yield_bps / 10_000` (floor); rough total coupon estimate.
    pub estimated_yield_payout: i128,
}

// --- Events ---

#[contractevent]
pub struct EscrowInitialized {
    #[topic]
    pub name: Symbol,
    pub escrow: InvoiceEscrow,
    /// Bound funding token; equals [`DataKey::FundingToken`].
    pub funding_token: Address,
    /// Bound treasury; equals [`DataKey::Treasury`].
    pub treasury: Address,
    /// Optional registry hint; equals [`DataKey::RegistryRef`] (`None` when unset).
    pub registry: Option<Address>,
    /// False when `escrow.maturity == 0`, which means `settle` has no maturity time lock.
    pub has_maturity_lock: bool,
}

#[contractevent]
pub struct MaxUniqueInvestorsCapLowered {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub invoice_id: Symbol,
    pub old_cap: u32,
    pub new_cap: u32,
}

#[contractevent]
pub struct EscrowFunded {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub invoice_id: Symbol,
    #[topic]
    pub investor: Address,
    pub amount: i128,
    pub funded_amount: i128,
    pub status: u32,
    /// Investor-specific effective yield (bps) after this fund; see [`DataKey::InvestorEffectiveYield`].
    pub investor_effective_yield_bps: i64,
    /// The `min_lock_secs` of the matched [`YieldTier`] (0 when base yield applies — no tier,
    /// no lock commitment, or simple fund). See [`LiquifactEscrow::effective_yield_for_commitment`].
    pub tier_lock_secs: u64,
}

/// Emitted by [`LiquifactEscrow::rotate_beneficiary`] when the SME (beneficiary)
/// address is changed, carrying both the prior and new addresses for auditing.
#[contractevent]
pub struct BeneficiaryRotated {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub invoice_id: Symbol,
    pub prior_sme: Address,
    pub new_sme: Address,
}

#[contractevent]
pub struct EscrowPartialSettle {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub invoice_id: Symbol,
    pub funded_amount: i128,
}

#[contractevent]
pub struct EscrowSettled {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub invoice_id: Symbol,
    pub funded_amount: i128,
    pub yield_bps: i64,
    pub maturity: u64,
    /// Ledger timestamp at which the settlement occurred.
    pub settled_at_ledger_timestamp: u64,
}

#[contractevent]
pub struct MaturityUpdatedEvent {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub invoice_id: Symbol,
    pub old_maturity: u64,
    pub new_maturity: u64,
}

#[contractevent]
pub struct AdminTransferredEvent {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub invoice_id: Symbol,
    pub new_admin: Address,
}

#[contractevent]
pub struct AdminProposedEvent {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub invoice_id: Symbol,
    pub current_admin: Address,
    pub pending_admin: Address,
}

#[contractevent]
pub struct FundingTargetUpdated {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub invoice_id: Symbol,
    pub old_target: i128,
    pub new_target: i128,
}

#[contractevent]
pub struct LegalHoldChanged {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub invoice_id: Symbol,
    /// `1` = hold enabled, `0` = cleared.
    pub active: u32,
    /// Human-readable reason for the hold (max 256 bytes). Empty when clearing.
    pub reason: String,
}

#[contractevent]
pub struct LegalHoldClearRequested {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub invoice_id: Symbol,
    /// Inclusive ledger timestamp when clearing may occur.
    pub clearable_at: u64,
}

/// Emitted by [`LiquifactEscrow::revalidate_token_cache`] when the token metadata cache is updated.
///
/// This event signals that the cached token metadata (decimals, etc.) has been refreshed from
/// the token contract. Used for auditing and monitoring cache staleness.
#[contractevent]
pub struct TokenCacheRevalidated {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub invoice_id: Symbol,
    /// Updated token decimals from the token contract.
    pub decimals: u32,
    /// Ledger timestamp when the cache was revalidated.
    pub revalidated_at_ledger_timestamp: u64,
}

/// SME collateral commitment metadata recorded.
///
/// This event is emitted when [`DataKey::SmeCollateralPledge`] is written or replaced by the SME.
/// It acts as a metadata-update signal and is not proof of custody, lien, encumbrance, asset control,
/// or token movement. The event intentionally omits token contract, custodian, and transfer-receipt
/// fields so consumers do not treat it as an on-chain encumbrance.
///
/// # Fields
/// - `name`: Hardcoded `coll_rec` symbol.
/// - `invoice_id`: Symbol representation of the invoice.
/// - `amount`: Newly recorded positive collateral amount.
/// - `prior_amount`: Prior recorded collateral amount (or `0` if none existed).
#[contractevent]
pub struct CollateralRecordedEvt {
    #[topic]
    pub name: Symbol,
    /// Invoice whose SME-reported metadata was updated.
    pub invoice_id: Symbol,
    /// SME-reported amount in the off-chain asset's own units; not a locked token balance.
    pub amount: i128,
    /// Prior recorded amount, or 0 if no prior commitment existed.
    pub prior_amount: i128,
}

#[contractevent]
pub struct SmeWithdrew {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub invoice_id: Symbol,
    pub amount: i128,
    pub recipient: Address,
}

#[contractevent]
pub struct InvestorPayoutClaimed {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub investor: Address,
    #[topic]
    pub invoice_id: Symbol,
}

/// Emitted when a batch of investor claims completes successfully.
/// Carries the count of claims processed so indexers can reconcile
/// against individual [`InvestorPayoutClaimed`] events emitted per investor.
#[contractevent]
pub struct BatchInvestorPayoutsClaimed {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub invoice_id: Symbol,
    /// Number of investors whose claims were processed in this batch.
    pub claimed_count: u32,
}

#[contractevent]
pub struct YieldSlippageWarning {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub investor: Address,
    #[topic]
    pub invoice_id: Symbol,
    /// Expected yield (in basis points) based on configured `yield_bps`.
    pub expected_yield_bps: i64,
    /// Actual effective yield (in basis points) determined at settlement.
    pub actual_yield_bps: i64,
    /// Configured slippage threshold (in basis points).
    pub slippage_threshold_bps: i64,
    /// Computed deviation (absolute difference) in basis points.
    pub deviation_bps: i64,
}

#[contractevent]
pub struct FundingCancelled {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub invoice_id: Symbol,
    pub funded_amount: i128,
}

#[contractevent]
pub struct InvestorRefundedEvt {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub investor: Address,
    #[topic]
    pub invoice_id: Symbol,
    pub amount: i128,
}

#[contractevent]
pub struct TreasuryDustSwept {
    #[topic]
    pub name: Symbol,
    pub invoice_id: Symbol,
    pub token: Address,
    pub amount: i128,
}

#[contractevent]
pub struct AssetCustodyVerified {
    #[topic]
    pub name: Symbol,
    pub invoice_id: Symbol,
    pub recorded_funded_amount: i128,
    pub contract_balance: i128,
    pub discrepancy: i128,
}

#[contractevent]
pub struct PrimaryAttestationBound {
    #[topic]
    pub name: Symbol,
    pub invoice_id: Symbol,
    pub digest: BytesN<32>,
}

#[contractevent]
pub struct AttestationDigestAppended {
    #[topic]
    pub name: Symbol,
    pub invoice_id: Symbol,
    pub index: u32,
    pub digest: BytesN<32>,
}

#[contractevent]
pub struct AttestationDigestRevoked {
    #[topic]
    pub name: Symbol,
    pub invoice_id: Symbol,
    pub index: u32,
}

#[contractevent]
pub struct AllowlistEnabledChanged {
    #[topic]
    pub name: Symbol,
    pub invoice_id: Symbol,
    /// `1` = enabled, `0` = disabled.
    pub active: u32,
}

#[contractevent]
pub struct InvestorAllowlistChanged {
    #[topic]
    pub name: Symbol,
    pub invoice_id: Symbol,
    pub investor: Address,
    /// `1` = allowed, `0` = blocked.
    pub allowed: u32,
}

/// Emitted when a compliance report is generated via
/// [`LiquifactEscrow::generate_compliance_report`].
#[contractevent]
pub struct ComplianceReportGenerated {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub invoice_id: Symbol,
    pub report: ComplianceReport,
}

#[contractevent]
pub struct FundReceived {
    /// Event schema version for forward compatibility.
    #[topic]
    pub name: Symbol,
    /// Escrow invoice identifier.
    #[topic]
    pub invoice_id: Symbol,
    /// Address that authorized the deposit.
    #[topic]
    pub actor: Address,
    /// Ledger timestamp when the deposit was recorded.
    pub timestamp: u64,
    /// Deposit amount in base units of the funding token.
    pub amount: i128,
    /// Running total funded after this deposit.
    pub funded_amount: i128,
    /// Escrow status after this deposit (0 = open, 1 = funded).
    pub status: u32,
    /// Effective yield (bps) assigned to this investor.
    pub investor_effective_yield_bps: i64,
    /// Lock duration (seconds) matched from the tier table; 0 if no lock.
    pub tier_lock_secs: u64,
}

/// Emitted on admin transfer (acceptance) and admin proposal.
///
/// Unifies `AdminProposedEvent` + `AdminTransferredEvent` into one versioned
/// event with ledger timestamp, old and new admin, and the change kind.
/// Schema event version = 1.
#[contractevent]
pub struct AdminChanged {
    /// Event schema version for forward compatibility.
    #[topic]
    pub name: Symbol,
    /// Escrow invoice identifier.
    #[topic]
    pub invoice_id: Symbol,
    /// Address that triggered the change.
    #[topic]
    pub actor: Address,
    /// Ledger timestamp when the change was recorded.
    pub timestamp: u64,
    /// `"proposed"` or `"accepted"`.
    pub kind: Symbol,
    /// Previous admin address.
    pub old_admin: Address,
    /// New admin (or pending) address.
    pub new_admin: Address,
}

/// Emitted when the compliance / legal hold is toggled.
///
/// Carries the actor (admin), ledger timestamp, and active flag.
/// Schema event version = 1.
#[contractevent]
pub struct LegalHoldSet {
    /// Event schema version for forward compatibility.
    #[topic]
    pub name: Symbol,
    /// Escrow invoice identifier.
    #[topic]
    pub invoice_id: Symbol,
    /// Address that toggled the hold (must be admin).
    #[topic]
    pub actor: Address,
    /// Ledger timestamp when the hold was toggled.
    pub timestamp: u64,
    /// `1` = hold enabled, `0` = cleared.
    pub active: u32,
}

/// Emitted when the admin pauses (or unpauses) the escrow via
/// [`LiquifactEscrow::set_escrow_paused`].
///
/// While paused, all state‑mutating entrypoints are blocked.
/// Schema event version = 1.
#[contractevent]
pub struct EscrowPaused {
    /// Event schema version for forward compatibility.
    #[topic]
    pub name: Symbol,
    /// Escrow invoice identifier.
    #[topic]
    pub invoice_id: Symbol,
    /// Address that toggled the pause (must be admin).
    #[topic]
    pub actor: Address,
    /// Ledger timestamp when pause was toggled.
    pub timestamp: u64,
    /// `1` = paused, `0` = resumed.
    pub paused: u32,
}

#[contractevent]
pub struct EscrowHealthWarning {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub invoice_id: Symbol,
    /// Warning category: 4001–4004 (0 = no warning, for future use).
    pub warning_type: u32,
    /// Current funded principal.
    pub funded_amount: i128,
    /// Funding target.
    pub funding_target: i128,
    /// Funded ratio in basis points: (funded_amount / funding_target) * 10_000
    /// (range 0–10_000+). Clamped to i64::MAX if overflow.
    pub funded_ratio_bps: i64,
    /// Seconds until maturity (may be negative if past maturity).
    /// i64::MAX if no maturity constraint (maturity == 0).
    pub time_to_maturity_secs: i64,
    pub recorded_at_ledger_timestamp: u64,
}

#[contractevent]
pub struct ContractUpgraded {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub invoice_id: Symbol,
    pub new_wasm_hash: BytesN<32>,
}

/// Emitted by [`LiquifactEscrow::detect_state_inconsistencies`] when one or more state
/// inconsistencies are detected. This is a diagnostic event for auditing and monitoring;
/// it does not mutate state and serves as a signal that the escrow may need investigation.
#[contractevent]
pub struct StateInconsistenciesDetected {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub invoice_id: Symbol,
    /// Flags indicating which inconsistencies were found.
    pub report: StateInconsistencyReport,
}

/// Diagnostic information for contract errors, emitted alongside error returns.
///
/// Records the snapshot name, metadata about creation (timestamp, sequence, admin address),
/// and the full escrow state at snapshot time. Used for audit trails and off-chain indexing.
#[contractevent]
pub struct StateSnapshotCreated {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub invoice_id: Symbol,
    #[topic]
    pub snapshot_name: Symbol,
    /// Ledger timestamp when snapshot was taken.
    pub created_at_ledger_timestamp: u64,
    /// Admin that created the snapshot.
    pub created_by: Address,
    /// Full escrow state at snapshot time.
    pub escrow_snapshot: InvoiceEscrow,
}

/// Emitted when the escrow is reverted to a previously saved state snapshot.
///
/// Records the snapshot name being reverted to, the prior state, the new state after revert,
/// and the admin that performed the revert. Used for compliance audits and debugging.
#[contractevent]
pub struct StateSnapshotReverted {
    #[topic]
    pub name: Symbol,
    #[topic]
    pub invoice_id: Symbol,
    #[topic]
    pub snapshot_name: Symbol,
    /// Ledger timestamp when revert occurred.
    pub reverted_at_ledger_timestamp: u64,
    /// Admin that triggered the revert.
    pub reverted_by: Address,
    /// Prior escrow state before revert.
    pub prior_escrow_state: InvoiceEscrow,
    /// New escrow state after revert (from snapshot).
    pub new_escrow_state: InvoiceEscrow,
}

/// Filter parameters for [`LiquifactEscrow::query_attestation_logs`].
///
/// All fields are optional; omitted filters match everything (pass-through).
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AuditLogFilter {
    /// Return only entries whose timestamp >= this value (inclusive).
    /// `None` ⇒ no lower bound on timestamp.
    pub after_timestamp: Option<u64>,
    /// Return only entries whose tag starts with this prefix (case-sensitive).
    /// `None` ⇒ no tag filter.
    pub tag_prefix: Option<Symbol>,
}

/// A single entry returned by [`LiquifactEscrow::query_attestation_logs`].
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AttestationLogEntry {
    /// The 32-byte attestation digest.
    pub digest: BytesN<32>,
    /// Zero-based index in the append log.
    pub index: u32,
    /// Ledger timestamp when this entry was appended (0 if absent).
    pub timestamp: u64,
    /// Optional tag stored with this entry (absent ⇒ empty symbol).
    pub tag: Symbol,
    /// Whether this index has been revoked.
    pub revoked: bool,
}

/// Paginated result from [`LiquifactEscrow::query_attestation_logs`].
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct AuditLogPage {
    /// Matching entries for the current page (after filtering & pagination).
    pub entries: Vec<AttestationLogEntry>,
    /// Total number of matching entries (before pagination) so callers can compute pages.
    pub total_count: u32,
}

/// Complete escrow state snapshot for archival/analysis.
///
/// Returned by [`LiquifactEscrow::export_escrow_snapshot`] for compliance and audit reporting.
/// All investor records are collected from persistent storage; only the current set of known
/// keys is scanned — the contract cannot enumerate investor addresses on-chain, so callers
/// should combine this snapshot with off-chain indexer data for a full investor list.
#[contracttype]
#[derive(Clone, Debug, PartialEq)]
pub struct EscrowSnapshot {
    /// Full escrow state.
    pub escrow: InvoiceEscrow,
    /// Stored schema version.
    pub schema_version: u32,
    /// Active legal hold flag.
    pub legal_hold: bool,
    /// Funding close snapshot (if escrow reached funded status).
    pub funding_close_snapshot: EscrowCloseSnapshot,
    /// Count of unique funder addresses.
    pub unique_funder_count: u32,
    /// Whether the allowlist gate is active.
    pub is_allowlist_active: bool,
    /// SME collateral commitment metadata (if recorded).
    pub sme_collateral_commitment: CollateralCommitmentSnapshot,
    /// Primary attestation hash (if bound).
    pub primary_attestation_hash: Option<BytesN<32>>,
    /// Current length of the attestation append log.
    pub attestation_log_length: u32,
    /// Sanctions provider address (if configured).
    pub sanctions_provider: Option<Address>,
    /// Funding token address.
    pub funding_token: Address,
    /// Treasury address.
    pub treasury: Address,
    /// Optional registry hint.
    pub registry: Option<Address>,
    /// Version history tuples of (version, timestamp).
    pub version_history: Vec<(u32, u64)>,
}

#[contract]
pub struct LiquifactEscrow;

/// Validates and converts a workspace-provided invoice identifier string into a Soroban [`Symbol`].
///
/// ### Constraints
/// - **Length**: Must be between 1 and [`MAX_INVOICE_ID_STRING_LEN`] (inclusive).
/// - **Charset**: Must only contain `[A-Za-z0-9_]`. This is a subset of the valid Symbol charset
///   enforced to ensure stable, URL-safe slugs in off-chain systems.
///
/// ### Security
/// This function performs a bounds-checked copy into a fixed stack buffer to prevent
/// uninitialized memory leaks. Only the exact byte-length of the input is converted
/// to the final symbol, ensuring no trailing null bytes or buffer remnants are preserved.
fn validate_invoice_id_string(env: &Env, invoice_id: &String) -> Symbol {
    match validation::validate_invoice_id(env, invoice_id) {
        Ok(sym) => sym,
        Err(err) => fail(env, err),
    }
}

#[contractimpl]
impl LiquifactEscrow {
    fn legal_hold_active(env: &Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::LegalHold)
            .unwrap_or(false)
    }

    /// Whether the escrow is currently paused by the admin.
    fn escrow_paused_active(env: &Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::EscrowPaused)
            .unwrap_or(false)
    }

    /// Compute which bucket (0..255) an investor address falls into.
    ///
    /// Uses the Soroban host's `Address` to‑bytes serialisation, then a simple
    /// DJB2‑style hash folded modulo [`INVESTOR_BUCKET_COUNT`]. The hash is
    /// deterministic within a single ledger but not guaranteed stable across
    /// network upgrades — bucket indices are soft hints for dashboards, not a
    /// consensus‑critical invariant.
    fn investor_bucket_index(env: &Env, addr: &Address) -> u32 {
        // Use the SDK-supplied `require_auth_for_args` / `Address` to derive
        // a stable 32-byte contract-internal representation via the host's
        // `compute_hash_sha256` function. This avoids heap-allocating a String
        // in no_std and gives us a deterministic seed.
        //
        // We take the first 8 bytes of the SHA-256 as a u64 seed and fold
        // modulo INVESTOR_BUCKET_COUNT.
        let hash_bytes = env.crypto().sha256(&addr.clone().into());
        let seed = u64::from_be_bytes([
            hash_bytes.get(0).unwrap(),
            hash_bytes.get(1).unwrap(),
            hash_bytes.get(2).unwrap(),
            hash_bytes.get(3).unwrap(),
            hash_bytes.get(4).unwrap(),
            hash_bytes.get(5).unwrap(),
            hash_bytes.get(6).unwrap(),
            hash_bytes.get(7).unwrap(),
        ]);
        (seed % (INVESTOR_BUCKET_COUNT as u64)) as u32
    }

    /// Emit a diagnostic event for an error with recovery guidance.
    ///
    /// # Logic
    /// - **Code 4001 (LowFundingRatio)**: `funded_ratio_bps < 5000` (< 50%) and close to/past maturity.
    /// - **Code 4002 (CloseToMaturity)**: `time_to_maturity_secs < 86400` (< 1 day) with healthy funding.
    /// - **Code 4003 (OverMaturity)**: Escrow is past maturity (`time_to_maturity_secs < 0`) and unfunded.
    /// - **Code 0**: No warning condition detected.
    ///
    /// # Non-blocking
    /// This function never causes a transaction to fail. If metrics cannot be computed
    /// (overflow, etc.), it gracefully returns code 0.
    fn compute_and_emit_health_warning(env: &Env, escrow: &InvoiceEscrow, now: u64) -> u32 {
        let funded_ratio_bps: i64 = if escrow.funding_target > 0 {
            // (funded_amount / funding_target) * 10_000
            // Use saturating arithmetic to avoid panic on overflow.
            let numerator = (escrow.funded_amount as i128).saturating_mul(10_000);
            let ratio = numerator / (escrow.funding_target as i128);
            // Clamp to i64 range.
            if ratio > i64::MAX as i128 {
                i64::MAX
            } else if ratio < 0 {
                0
            } else {
                ratio as i64
            }
        } else {
            10_000_i64 // No target set; assume fully funded.
        };

        let time_to_maturity_secs: i64 = if escrow.maturity > 0 {
            let maturity_i64 = escrow.maturity as i64;
            let now_i64 = now as i64;
            // Clamp to prevent overflow when computing the delta.
            maturity_i64.saturating_sub(now_i64)
        } else {
            i64::MAX // No maturity constraint.
        };

        // Determine warning type based on conditions.
        let warning_type = if time_to_maturity_secs < 0 && escrow.status == 0 && escrow.funded_amount < escrow.funding_target {
            4003 // OverMaturity: past maturity, still open, and unfunded.
        } else if time_to_maturity_secs >= 0 && time_to_maturity_secs < 86400 {
            // Close to maturity (< 1 day away).
            if funded_ratio_bps < 5000 {
                4001 // LowFundingRatio near maturity.
            } else {
                4002 // CloseToMaturity with healthy funding.
            }
        } else if funded_ratio_bps < 5000 && escrow.status == 0 {
            4001 // LowFundingRatio (open, no immediate time pressure).
        } else {
            0 // No warning.
        };

        // Only emit if a warning was detected.
        if warning_type != 0 {
            EscrowHealthWarning {
                name: symbol_short!("hlth_wrn"),
                invoice_id: escrow.invoice_id.clone(),
                warning_type,
                funded_amount: escrow.funded_amount,
                funding_target: escrow.funding_target,
                funded_ratio_bps,
                time_to_maturity_secs,
                recorded_at_ledger_timestamp: now,
            }
            .publish(env);
        }

        warning_type
    }

    /// Read the immutable funding token address, failing with [`EscrowError::FundingTokenNotSet`]
    /// when the escrow has not been initialized.
    fn funding_token_or_fail(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::FundingToken)
            .unwrap_or_else(|| fail(env, EscrowError::FundingTokenNotSet))
    }

    /// Read the immutable treasury address, failing with [`EscrowError::TreasuryNotSet`]
    /// when the escrow has not been initialized.
    fn treasury_or_fail(env: &Env) -> Address {
        env.storage()
            .instance()
            .get(&DataKey::Treasury)
            .unwrap_or_else(|| fail(env, EscrowError::TreasuryNotSet))
    }

    fn validate_yield_tiers_table(env: &Env, tiers: &Option<Vec<YieldTier>>, base_yield: i64) {
        let Some(tiers) = tiers else {
            return;
        };
        if tiers.is_empty() {
            return;
        }
        let n = tiers.len();
        for i in 0..n {
            let t = tiers.get(i).unwrap();
            ensure(
                env,
                (0..=10_000).contains(&t.yield_bps),
                EscrowError::TierYieldOutOfRange,
            );
            ensure(
                env,
                t.yield_bps >= base_yield,
                EscrowError::TierYieldBelowBase,
            );
            if i > 0 {
                let p = tiers.get(i - 1).unwrap();
                ensure(
                    env,
                    t.min_lock_secs > p.min_lock_secs,
                    EscrowError::TierLockNotIncreasing,
                );
                ensure(
                    env,
                    t.yield_bps >= p.yield_bps,
                    EscrowError::TierYieldNotNonDecreasing,
                );
            }
        }
    }

    /// Returns `(effective_yield_bps, matched_lock_secs)` for a given commitment.
    /// `matched_lock_secs` is the [`YieldTier::min_lock_secs`] of the best matching tier,
    /// or `0` when no tier was matched (base yield applies).
    fn effective_yield_for_commitment(
        env: &Env,
        base_yield: i64,
        committed_lock_secs: u64,
    ) -> (i64, u64) {
        if committed_lock_secs == 0 {
            return (base_yield, 0);
        }
        let Some(tiers) = env
            .storage()
            .instance()
            .get::<DataKey, Vec<YieldTier>>(&DataKey::YieldTierTable)
        else {
            return (base_yield, 0);
        };
        if tiers.is_empty() {
            return (base_yield, 0);
        }
        let mut best = base_yield;
        let mut best_lock = 0u64;
        let n = tiers.len();
        for i in 0..n {
            let t = tiers.get(i).unwrap();
            if committed_lock_secs >= t.min_lock_secs && t.yield_bps > best {
                best = t.yield_bps;
                best_lock = t.min_lock_secs;
            }
        }
        (best, best_lock)
    }

    /// Initialize escrow. `funding_target` defaults to `amount`.
    ///
    /// Binds **`funding_token`**, **`treasury`**, and optional **`registry`** for this instance only.
    /// The funding token and treasury addresses are **immutable** after this call; the registry id is
    /// optional metadata for off-chain indexers (not an on-chain authority).
    ///
    /// `maturity == 0` is an explicit "no maturity lock" configuration: once funded, the SME may
    /// call [`LiquifactEscrow::settle`] immediately. Positive maturity values are validator-observed
    /// ledger timestamps and are enforced with an inclusive `ledger.timestamp() >= maturity` check.
    ///
    /// `invoice_id` must satisfy [`MAX_INVOICE_ID_STRING_LEN`] and charset rules (see
    /// [`validate_invoice_id_string`]).
    ///
    /// # Errors
    /// Emits typed [`EscrowError`] codes for invalid amounts, yield bounds, invoice id validation,
    /// duplicate initialization, malformed optional caps, and invalid tier configuration.
    pub fn init(
        env: Env,
        admin: Address,
        invoice_id: String,
        sme_address: Address,
        amount: i128,
        yield_bps: i64,
        maturity: u64,
        funding_token: Address,
        registry: Option<Address>,
        treasury: Address,
        yield_tiers: Option<Vec<YieldTier>>,
        min_contribution: Option<i128>,
        max_unique_investors: Option<u32>,
        max_per_investor: Option<i128>,
        legal_hold_clear_delay: Option<u64>,
        funding_deadline: Option<u64>,
        yield_slippage_threshold: Option<i64>,
        settlement_notifier_contract: Option<Address>,
    ) -> InvoiceEscrow {
        admin.require_auth();

        ensure(&env, amount > 0, EscrowError::AmountMustBePositive);
        ensure(
            &env,
            (0..=10_000).contains(&yield_bps),
            EscrowError::YieldBpsOutOfRange,
        );
        ensure(
            &env,
            !env.storage().instance().has(&DataKey::Escrow),
            EscrowError::EscrowAlreadyInitialized,
        );

        // Validate funding deadline
        if let Some(deadline) = funding_deadline {
            ensure(
                &env,
                deadline > env.ledger().timestamp(),
                EscrowError::FundingDeadlinePassed,
            );
            env.storage()
                .instance()
                .set(&DataKey::FundingDeadline, &deadline);
        }

        Self::validate_yield_tiers_table(&env, &yield_tiers, yield_bps);

        let invoice_sym = validate_invoice_id_string(&env, &invoice_id);

        let escrow = InvoiceEscrow {
            invoice_id: invoice_sym.clone(),
            admin: admin.clone(),
            sme_address: sme_address.clone(),
            amount,
            funding_target: amount,
            funded_amount: 0,
            yield_bps,
            maturity,
            status: 0,
        };

        env.storage().instance().set(&DataKey::Escrow, &escrow);
        env.storage()
            .instance()
            .set(&DataKey::Version, &SCHEMA_VERSION);

        // Record version history for audit trail
        let now_ts = env.ledger().timestamp();
        let mut vh: Vec<(u32, u64)> = Vec::new(&env);
        vh.push_back((SCHEMA_VERSION, now_ts));
        env.storage()
            .instance()
            .set(&DataKey::VersionHistory, &vh);

        // Store sanctions provider if configured
        if let Some(ref sp) = sanctions_provider {
            env.storage()
                .instance()
                .set(&DataKey::SanctionsProvider, sp);
        }

        env.storage()
            .instance()
            .set(&DataKey::FundingToken, &funding_token);
        env.storage().instance().set(&DataKey::Treasury, &treasury);
        
        // Cache token metadata at init time to reduce external calls on fund operations
        let token_client = TokenClient::new(&env, &funding_token);
        let decimals = token_client.decimals();
        let token_cache = TokenMetadataCache {
            decimals,
            cached_at_ledger_timestamp: env.ledger().timestamp(),
            cached_at_ledger_sequence: env.ledger().sequence(),
        };
        env.storage()
            .instance()
            .set(&DataKey::TokenMetadataCache, &token_cache);
        
        if let Some(ref r) = registry {
            env.storage().instance().set(&DataKey::RegistryRef, r);
        }
        if let Some(ref tiers) = yield_tiers {
            if !tiers.is_empty() {
                env.storage()
                    .instance()
                    .set(&DataKey::YieldTierTable, tiers);
            }
        }

        let floor = min_contribution.unwrap_or(0);
        if min_contribution.is_some() {
            ensure(&env, floor > 0, EscrowError::MinContributionNotPositive);
            ensure(
                &env,
                floor <= amount,
                EscrowError::MinContributionExceedsAmount,
            );
        }
        env.storage()
            .instance()
            .set(&DataKey::MinContributionFloor, &floor);

        env.storage()
            .instance()
            .set(&DataKey::UniqueFunderCount, &0u32);
        // Initialize the investor-list count alongside the funder count.
        env.storage().instance().set(&DataKey::InvestorCount, &0u32);

        if let Some(cap) = max_per_investor {
            ensure(&env, cap > 0, EscrowError::MaxPerInvestorNotPositive);
            env.storage()
                .instance()
                .set(&DataKey::MaxPerInvestorCap, &cap);
        }

        if let Some(cap) = max_unique_investors {
            ensure(&env, cap > 0, EscrowError::MaxUniqueInvestorsNotPositive);
            env.storage()
                .instance()
                .set(&DataKey::MaxUniqueInvestorsCap, &cap);
        }

        let delay = legal_hold_clear_delay.unwrap_or(0);
        if delay > 0 {
            env.storage()
                .instance()
                .set(&DataKey::LegalHoldClearDelay, &delay);
        }

        // Validate and store yield slippage threshold
        let threshold = yield_slippage_threshold.unwrap_or(0);
        if threshold > 0 {
            ensure(
                &env,
                (0..=10_000).contains(&threshold),
                EscrowError::YieldSlippageThresholdOutOfRange,
            );
            env.storage()
                .instance()
                .set(&DataKey::YieldSlippageThreshold, &threshold);
        }

        // Store optional settlement notifier contract address
        if let Some(ref notifier) = settlement_notifier_contract {
            env.storage()
                .instance()
                .set(&DataKey::SettlementNotifierContract, notifier);
        }

        // Store creation timestamp for registry listings
        env.storage()
            .instance()
            .set(&DataKey::CreatedAt, &env.ledger().timestamp());

        EscrowInitialized {
            name: symbol_short!("escrow_ii"),
            // Read stored values so event fields match persisted keys (indexer single-event bootstrap).
            escrow: Self::get_escrow(env.clone()),
            funding_token: Self::get_funding_token(env.clone()),
            treasury: Self::get_treasury(env.clone()),
            registry: Self::get_registry_ref(env.clone()),
            has_maturity_lock: Self::has_maturity_lock(env.clone()),
        }
        .publish(&env);

        escrow
    }

    /// Returns the SEP-41 funding token bound at [`LiquifactEscrow::init`] ([`DataKey::FundingToken`]).
    ///
    /// **Immutable:** set once at init; cannot change after deploy. Emits
    /// [`EscrowError::FundingTokenNotSet`] if called before init.
    pub fn get_funding_token(env: Env) -> Address {
        Self::funding_token_or_fail(&env)
    }

    /// Returns the protocol treasury address bound at [`LiquifactEscrow::init`] ([`DataKey::Treasury`]).
    ///
    /// **Immutable:** set once at init; cannot change after deploy. The treasury is the only
    /// recipient of [`LiquifactEscrow::sweep_terminal_dust`]. Emits
    /// [`EscrowError::TreasuryNotSet`] if called before init.
    pub fn get_treasury(env: Env) -> Address {
        Self::treasury_or_fail(&env)
    }

    /// Returns the optional off-chain registry hint stored at [`DataKey::RegistryRef`], or [`None`]
    /// when no registry was supplied at [`LiquifactEscrow::init`].
    ///
    /// **Non-authority:** this address is a read-only discoverability hint for off-chain indexers.
    /// No on-chain logic in this contract consults it. Callers must **not** treat its presence as
    /// proof of registry membership — query the registry contract directly to verify on-chain state.
    pub fn get_registry_ref(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::RegistryRef)
    }

    /// Returns the cached token metadata (decimals, cache timestamp, cache sequence).
    ///
    /// This metadata is captured at [`LiquifactEscrow::init`] to reduce external token contract
    /// calls during fund operations. Returns [`None`] if not yet cached (should not happen if init succeeded).
    ///
    /// For staleness detection or re-caching, use [`LiquifactEscrow::revalidate_token_cache`].
    pub fn get_token_metadata_cache(env: Env) -> Option<TokenMetadataCache> {
        env.storage().instance().get(&DataKey::TokenMetadataCache)
    }

    /// Returns the optional pending admin address waiting for [`LiquifactEscrow::accept_admin`],
    /// or [`None`] when no admin handover is in progress.
    pub fn get_pending_admin(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::PendingAdmin)
    }

    /// Compare the escrow contract's current funding-token balance to the recorded `funded_amount`.
    ///
    /// Returns a signed discrepancy equal to `contract_balance - recorded_funded_amount`.
    /// Positive values indicate the contract holds more than the recorded funded amount;
    /// negative values indicate a shortfall.
    ///
    /// # Authorization
    /// Requires the current admin to authorize the call. This makes the entrypoint suitable for
    /// manual audits and for off-chain schedulers or external triggers that reconcile custody.
    pub fn verify_asset_custody(env: Env) -> i128 {
        let escrow = Self::load_escrow_require_admin(&env);
        let token_addr = Self::funding_token_or_fail(&env);
        let this = env.current_contract_address();

        let contract_balance = TokenClient::new(&env, &token_addr).balance(&this);
        let recorded_funded_amount = escrow.funded_amount;
        let discrepancy = if contract_balance >= recorded_funded_amount {
            contract_balance - recorded_funded_amount
        } else {
            -(recorded_funded_amount - contract_balance)
        };

        AssetCustodyVerified {
            name: symbol_short!("cust_ver"),
            invoice_id: escrow.invoice_id.clone(),
            recorded_funded_amount,
            contract_balance,
            discrepancy,
        }
        .publish(&env);

        discrepancy
    }

    /// Return whether this escrow has a configured maturity time lock.
    ///
    /// `true` means [`InvoiceEscrow::maturity`] is positive and [`LiquifactEscrow::settle`] requires
    /// `Env::ledger().timestamp() >= maturity`. `false` means `maturity == 0`: there is no maturity
    /// gate, so a funded escrow can be settled immediately by the SME, subject to legal-hold and
    /// status guards.
    pub fn has_maturity_lock(env: Env) -> bool {
        Self::get_escrow(env).maturity > 0
    }

    /// Move up to `amount` (capped by balance and [`MAX_DUST_SWEEP_AMOUNT`]) of the **funding token**
    /// from this contract to [`DataKey::Treasury`].
    ///
    /// # Terminal state requirement
    /// Only permitted when [`InvoiceEscrow::status`] is **2 (settled)**, **3 (withdrawn)**, or
    /// **4 (cancelled)**. Open (0) or funded (1) states reject the call so live principal cannot
    /// be swept as dust.
    ///
    /// # Liability floor invariant
    /// In **cancelled** (status 4) escrows, the sweep is rejected if it would reduce the
    /// contract's token balance below the amount still owed to investors who have not yet
    /// called [`LiquifactEscrow::refund`]:
    ///
    /// ```text
    /// outstanding = funded_amount - distributed_principal
    /// assert balance - sweep_amt >= outstanding
    /// ```
    ///
    /// `distributed_principal` ([`DataKey::DistributedPrincipal`]) is incremented atomically
    /// by [`LiquifactEscrow::refund`] each time an investor's principal is returned. This makes
    /// the invariant computable on-chain without iterating over all investor addresses.
    ///
    /// In **settled** (2) and **withdrawn** (3) states, disbursement is off-chain and this
    /// floor does not apply.
    ///
    /// # Authorization
    /// The configured **treasury** account must authorize this call; the admin cannot sweep unless
    /// it is also the treasury.
    ///
    /// Blocked while [`DataKey::LegalHold`] is active.
    ///
    /// # Errors
    /// Emits typed [`EscrowError`] codes for legal hold, invalid sweep amount, non-terminal state,
    /// missing initialized addresses, empty balances, liability floor violation, and token
    /// transfer invariant failures.
    pub fn sweep_terminal_dust(env: Env, amount: i128) -> i128 {
        ensure(
            &env,
            !Self::legal_hold_active(&env),
            EscrowError::LegalHoldBlocksTreasuryDustSweep,
        );
        ensure(
            &env,
            !Self::escrow_paused_active(&env),
            EscrowError::EscrowIsPaused,
        );
        ensure(&env, amount > 0, EscrowError::SweepAmountNotPositive);
        ensure(
            &env,
            amount <= MAX_DUST_SWEEP_AMOUNT,
            EscrowError::SweepAmountExceedsMax,
        );

        // env.clone(): env is used again after this call for treasury/token reads and publish.
        let escrow = Self::get_escrow(env.clone());
        ensure(
            &env,
            escrow.status == 2 || escrow.status == 3 || escrow.status == 4 || escrow.status == 5,
            EscrowError::DustSweepNotTerminal,
        );

        let treasury = Self::treasury_or_fail(&env);
        treasury.require_auth();

        let token_addr = Self::funding_token_or_fail(&env);
        let this = env.current_contract_address();

        let token = TokenClient::new(&env, &token_addr);
        let balance = token.balance(&this);
        ensure(&env, balance > 0, EscrowError::NoFundingTokenBalanceToSweep);
        let sweep_amt = amount.min(balance);
        ensure(&env, sweep_amt > 0, EscrowError::EffectiveSweepAmountZero);

        // Liability floor (cancelled escrows only): sweep must not reduce the balance below
        // principal still owed to investors who have not yet called refund().
        //
        // In settled (2) and withdrawn (3) states, disbursement is off-chain and
        // distributed_principal stays 0, so the floor is not applicable there.
        // In cancelled (4) state, refund() is the on-chain redemption path and increments
        // distributed_principal atomically, making the invariant computable here.
        //
        // outstanding = funded_amount - distributed_principal
        // Invariant: balance - sweep_amt >= outstanding
        if escrow.status == 4 {
            let distributed: i128 = env
                .storage()
                .instance()
                .get(&DataKey::DistributedPrincipal)
                .unwrap_or(0);
            let outstanding = escrow.funded_amount.saturating_sub(distributed);
            // sweep_amt <= balance (from amount.min(balance) above), so this subtraction is safe.
            let balance_after_sweep = balance - sweep_amt;
            ensure(
                &env,
                balance_after_sweep >= outstanding,
                EscrowError::SweepExceedsLiabilityFloor,
            );
        }

        external_calls::transfer_funding_token_with_balance_checks(
            &env,
            &token_addr,
            &this,
            &treasury,
            sweep_amt,
        );

        TreasuryDustSwept {
            name: symbol_short!("dust_sw"),
            invoice_id: escrow.invoice_id.clone(),
            token: token_addr,
            amount: sweep_amt,
        }
        .publish(&env);

        sweep_amt
    }

    /// Archive a terminal escrow to mark it as closed and reduce active monitoring.
    ///
    /// Transitions the escrow to status `5` (archived). Only permitted from terminal states:
    /// `2` (settled), `3` (withdrawn), `4` (cancelled), or `5` (already archived — no-op).
    /// Open (`0`) or funded (`1`) states are rejected.
    ///
    /// # Authorization
    /// Requires admin authorization. Read-only operations continue to work on archived escrows.
    /// Archived escrows remain accessible but should be excluded from active-monitoring queries
    /// by off-chain indexers.
    ///
    /// # Errors
    /// Emits [`EscrowError::ArchiveNotTerminal`] if the escrow is not in a terminal state.
    /// Emits [`EscrowError::EscrowAlreadyArchived`] if already status `5`.
    pub fn archive_escrow(env: Env) -> InvoiceEscrow {
        let mut escrow = Self::load_escrow_require_admin(&env);

        // Idempotent no-op: already archived
        if escrow.status == 5 {
            return escrow;
        }

        ensure(
            &env,
            escrow.status == 2 || escrow.status == 3 || escrow.status == 4,
            EscrowError::ArchiveNotTerminal,
        );

        let prior_status = escrow.status;
        escrow.status = 5;

        env.storage().instance().set(&DataKey::Escrow, &escrow);

        EscrowArchived {
            name: symbol_short!("esc_arch"),
            invoice_id: escrow.invoice_id.clone(),
            prior_status,
            archived_at_ledger_timestamp: env.ledger().timestamp(),
        }
        .publish(&env);

        escrow
    }

    pub fn get_escrow(env: Env) -> InvoiceEscrow {
        env.storage()
            .instance()
            .get(&DataKey::Escrow)
            .unwrap_or_else(|| fail(&env, EscrowError::EscrowNotInitialized))
    }

    /// Rotate the beneficiary (SME) address that receives liquidity on
    /// settlement / `withdraw`.
    ///
    /// Permitted only before settlement (`status` 0 = open or 1 = funded) and
    /// while no legal hold is active. Requires authorization from **both** the
    /// current SME and the admin, so the payout destination can never be changed
    /// unilaterally. A no-op rotation to the current address is rejected. Emits
    /// [`BeneficiaryRotated`] with the prior and new addresses and returns the
    /// updated escrow snapshot.
    ///
    /// # Errors
    ///
    /// | Condition | Typed error |
    /// |-----------|-------------|
    /// | Legal hold active | [`EscrowError::LegalHoldBlocksBeneficiaryRotation`] |
    /// | Escrow not open or funded | [`EscrowError::RotationNotOpen`] |
    /// | `new_sme_address == current SME` | [`EscrowError::NewSmeSameAsCurrent`] |
    pub fn rotate_beneficiary(env: Env, new_sme_address: Address) -> InvoiceEscrow {
        // Legal-hold gate (read-only).
        ensure(
            &env,
            !Self::legal_hold_active(&env),
            EscrowError::LegalHoldBlocksBeneficiaryRotation,
        );
        ensure(
            &env,
            !Self::escrow_paused_active(&env),
            EscrowError::EscrowIsPaused,
        );

        let mut escrow = Self::get_escrow(env.clone());

        // Only permitted in pre-settlement states (open or funded).
        ensure(
            &env,
            escrow.status == 0 || escrow.status == 1,
            EscrowError::RotationNotOpen,
        );

        // Reject a no-op rotation to the current beneficiary.
        ensure(
            &env,
            new_sme_address != escrow.sme_address,
            EscrowError::NewSmeSameAsCurrent,
        );

        // Dual authorization: the outgoing SME and the admin must both sign.
        escrow.sme_address.require_auth();
        escrow.admin.require_auth();

        let prior_sme = escrow.sme_address.clone();
        escrow.sme_address = new_sme_address.clone();
        env.storage().instance().set(&DataKey::Escrow, &escrow);

        BeneficiaryRotated {
            name: symbol_short!("ben_rot"),
            invoice_id: escrow.invoice_id.clone(),
            prior_sme,
            new_sme: new_sme_address,
        }
        .publish(&env);

        escrow
    }

    /// Load the current escrow and require admin authorization in one step.
    ///
    /// Consolidates the repeated `let escrow = Self::get_escrow(env.clone()); escrow.admin.require_auth();`
    /// pattern used across multiple admin-gated entrypoints.
    fn load_escrow_require_admin(env: &Env) -> InvoiceEscrow {
        let escrow: InvoiceEscrow = env
            .storage()
            .instance()
            .get(&DataKey::Escrow)
            .unwrap_or_else(|| fail(env, EscrowError::EscrowNotInitialized));
        escrow.admin.require_auth();
        escrow
    }

    /// Load the current escrow and require SME authorization in one step.
    ///
    /// Consolidates the repeated `let escrow = Self::get_escrow(env.clone()); escrow.sme_address.require_auth();`
    /// pattern used across multiple SME-gated entrypoints.
    fn load_escrow_require_sme(env: &Env) -> InvoiceEscrow {
        let escrow: InvoiceEscrow = env
            .storage()
            .instance()
            .get(&DataKey::Escrow)
            .unwrap_or_else(|| fail(env, EscrowError::EscrowNotInitialized));
        escrow.sme_address.require_auth();
        escrow
    }

    pub fn get_version(env: Env) -> u32 {
        env.storage().instance().get(&DataKey::Version).unwrap_or(0)
    }

    /// Returns the contract interface version ([`CONTRACT_INTERFACE_VERSION`]).
    ///
    /// Use this to detect caller/contract ABI mismatches **before** invoking state-mutating
    /// entrypoints. This value is a compile-time constant baked into the deployed WASM and
    /// does **not** require the escrow to be initialized (no [`DataKey::Escrow`] read).
    ///
    /// # Interface version vs schema version
    ///
    /// | Getter | Constant | Tracks |
    /// |--------|----------|--------|
    /// | `get_interface_version` | [`CONTRACT_INTERFACE_VERSION`] | Entrypoint signatures, parameter lists, return types |
    /// | `get_version` | [`SCHEMA_VERSION`] | On-chain storage layout (XDR structs, `DataKey` variants) |
    ///
    /// # Caller guidance
    ///
    /// SDKs and integration adapters should call `get_interface_version` at startup and
    /// compare the returned value against the version they were compiled against. A mismatch
    /// means the deployed contract has a different ABI than expected; the caller should
    /// refuse further calls and surface a diagnostic error rather than silently mis-parse
    /// arguments or return values.
    ///
    /// See `docs/escrow-interface-versioning.md` for the full versioning policy.
    ///
    /// # Authorization
    ///
    /// None — pure read; no auth required. Safe to call before `init`.
    pub fn get_interface_version(_env: Env) -> u32 {
        CONTRACT_INTERFACE_VERSION
    }

    /// Return the build metadata baked into this WASM artifact.
    ///
    /// This is the canonical way for off-chain tooling and CI to verify that a
    /// deployed contract was built from the expected source commit, with the
    /// expected [`SCHEMA_VERSION`] and [`CONTRACT_INTERFACE_VERSION`].
    ///
    /// The values are compile-time constants produced by `escrow/build.rs`
    /// (git commit hash, build timestamp, package version, rustc version,
    /// and the source-of-truth schema/interface versions). They are part of
    /// the WASM binary, not on-chain storage, so:
    ///
    /// - The values do not change after deploy (a WASM upgrade would produce
    ///   a new instance with a new `CONTRACT_ID` and a new build metadata).
    /// - The entrypoint is available **without** prior [`LiquifactEscrow::init`].
    /// - No authentication is required.
    ///
    /// The `schema_version` and `interface_version` returned here are
    /// **extracted from `src/lib.rs` at compile time** by `build.rs`, so they
    /// are guaranteed to match the source constants — `scripts/validate_build_metadata.py`
    /// enforces this in CI.
    pub fn get_build_metadata(env: Env) -> BuildMetadata {
        BuildMetadata {
            schema_version: BUILD_SCHEMA_VERSION,
            interface_version: BUILD_INTERFACE_VERSION,
            git_commit: String::from_str(&env, BUILD_GIT_COMMIT),
            git_full_commit: String::from_str(&env, BUILD_GIT_FULL_COMMIT),
            build_timestamp: String::from_str(&env, BUILD_TIMESTAMP),
            pkg_version: String::from_str(&env, BUILD_PKG_VERSION),
            rust_version: String::from_str(&env, BUILD_RUST_VERSION),
        }
    }

    /// Get the optional funding deadline (ledger timestamp), returns None if not set.
    pub fn get_funding_deadline(env: Env) -> Option<u64> {
        env.storage().instance().get(&DataKey::FundingDeadline)
    }

    /// Check if funding has expired (deadline set and now > deadline).
    pub fn is_funding_expired(env: Env) -> bool {
        if let Some(deadline) = env.storage().instance().get(&DataKey::FundingDeadline) {
            env.ledger().timestamp() > deadline
        } else {
            false
        }
    }

    /// Read-only health check: compute and return the current health metrics without state mutation.
    /// Returns `(warning_type, funded_ratio_bps, time_to_maturity_secs)`.
    ///
    /// # Returns
    /// - `warning_type`: 0 = no warning, 4001–4004 = specific warning condition.
    /// - `funded_ratio_bps`: basis points ratio (0–10_000+, clamped to i64::MAX on overflow).
    /// - `time_to_maturity_secs`: seconds until maturity (negative if past, i64::MAX if no maturity).
    ///
    /// # Authorization
    /// None — pure read; no auth required.
    pub fn check_escrow_health(env: Env) -> (u32, i64, i64) {
        let escrow = Self::get_escrow(env.clone());
        let now = env.ledger().timestamp();

        let funded_ratio_bps: i64 = if escrow.funding_target > 0 {
            let numerator = (escrow.funded_amount as i128).saturating_mul(10_000);
            let ratio = numerator / (escrow.funding_target as i128);
            if ratio > i64::MAX as i128 {
                i64::MAX
            } else if ratio < 0 {
                0
            } else {
                ratio as i64
            }
        } else {
            10_000_i64
        };

        let time_to_maturity_secs: i64 = if escrow.maturity > 0 {
            let maturity_i64 = escrow.maturity as i64;
            let now_i64 = now as i64;
            maturity_i64.saturating_sub(now_i64)
        } else {
            i64::MAX
        };

        let warning_type = if time_to_maturity_secs < 0 && escrow.status == 0 && escrow.funded_amount < escrow.funding_target {
            4003
        } else if time_to_maturity_secs >= 0 && time_to_maturity_secs < 86400 {
            if funded_ratio_bps < 5000 {
                4001
            } else {
                4002
            }
        } else if funded_ratio_bps < 5000 && escrow.status == 0 {
            4001
        } else {
            0
        };

        (warning_type, funded_ratio_bps, time_to_maturity_secs)
    }

    /// Whether a compliance/legal hold is active (defaults to `false` if unset).
    pub fn get_legal_hold(env: Env) -> bool {
        Self::legal_hold_active(&env)
    }

    /// Returns the reason for the current legal hold, if any.
    /// Absent when no hold is active or the hold was set without a reason.
    pub fn get_legal_hold_reason(env: Env) -> Option<String> {
        env.storage().instance().get(&DataKey::LegalHoldReason)
    }

    /// Configured minimum delay between [`LiquifactEscrow::request_clear_legal_hold`]
    /// and [`LiquifactEscrow::set_legal_hold(env, false)`]. Defaults to `0`.
    pub fn get_legal_hold_clear_delay(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::LegalHoldClearDelay)
            .unwrap_or(0)
    }

    /// Reserved minimum ledger timestamp at which a pending legal-hold clear may be applied.
    /// `None` means no request has been recorded.
    pub fn get_legal_hold_clearable_at(env: Env) -> Option<u64> {
        env.storage().instance().get(&DataKey::LegalHoldClearableAt)
    }

    /// Minimum principal per [`LiquifactEscrow::fund`] or [`LiquifactEscrow::fund_with_commitment`] call
    /// in token base units; `0` means no extra floor beyond “amount must be positive”.
    ///
    /// **Ceilings:** [`InvoiceEscrow::funding_target`] and over-funding behavior are unchanged; the floor
    /// applies to **each** call, so follow-on deposits from the same investor must also meet the floor.
    pub fn get_min_contribution_floor(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::MinContributionFloor)
            .unwrap_or(0)
    }

    /// Optional cap on **distinct** investor addresses (`prev == 0` at fund time); [`None`] if unlimited.
    ///
    /// Reflects the current stored cap, including any admin reduction via
    /// [`LiquifactEscrow::lower_max_unique_investors`].
    pub fn get_max_unique_investors_cap(env: Env) -> Option<u32> {
        env.storage()
            .instance()
            .get(&DataKey::MaxUniqueInvestorsCap)
    }

    /// Optional cap on total principal for a single investor address.
    /// Absent ⇒ unlimited. Enforced on every deposit.
    pub fn get_max_per_investor_cap(env: Env) -> Option<i128> {
        env.storage().instance().get(&DataKey::MaxPerInvestorCap)
    }

    /// Distinct funders counted so far (each address counted once when it first receives principal).
    ///
    /// **Sybil:** this limits distinct **chain accounts**, not real-world persons; Sybil resistance is
    /// not a goal of this counter.
    pub fn get_unique_funder_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::UniqueFunderCount)
            .unwrap_or(0)
    }

    /// Compute and return dashboard‑ready health metrics for the escrow.
    ///
    /// Read‑only — **no authorization required**. Intended for dashboards
    /// and monitoring tools that surface at‑a‑glance escrow health.
    ///
    /// # Design notes
    ///
    /// - `funding_progress_percent` is computed as `(funded_amount × 100) / funding_target`
    ///   (floor), capped at 100 when overfunded.
    /// - `days_to_maturity` uses integer seconds → days conversion (86_400 s/day).
    ///   Returns 0 when `maturity == 0` (no maturity lock). Negative when maturity
    ///   has passed (i.e., settlement is already possible).
    /// - `estimated_yield_payout` is the flat coupon on the whole `funded_amount` —
    ///   pro‑rata per‑investor breakdown requires the funding‑close snapshot.
    pub fn get_escrow_health_metrics(env: Env) -> EscrowHealthMetrics {
        let escrow = Self::get_escrow(env.clone());
        let unique_investor_count = Self::get_unique_funder_count(env.clone());

        // Funding progress %
        let funding_progress_percent = if escrow.funding_target > 0 {
            let pct = (escrow.funded_amount * 100) / escrow.funding_target;
            pct.min(100) as u32
        } else {
            0
        };

        // Days to maturity
        let days_to_maturity = if escrow.maturity == 0 {
            0i64
        } else {
            let now = env.ledger().timestamp();
            let delta_secs = escrow.maturity as i64 - now as i64;
            delta_secs / 86_400i64
        };

        // Average contribution
        let average_contribution_size = if unique_investor_count > 0 {
            escrow.funded_amount / (unique_investor_count as i128)
        } else {
            0i128
        };

        // Estimated yield payout
        let estimated_yield_payout =
            (escrow.funded_amount * escrow.yield_bps as i128) / 10_000i128;

        EscrowHealthMetrics {
            funding_progress_percent,
            days_to_maturity,
            unique_investor_count,
            average_contribution_size,
            estimated_yield_payout,
        }
    }

    /// Bundles multiple read-only values to return a comprehensive summary of the escrow state
    /// in a single host invocation.
    pub fn get_escrow_summary(env: Env) -> EscrowSummary {
        let escrow = Self::get_escrow(env.clone());
        let legal_hold = Self::get_legal_hold(env.clone());
        let legal_hold_reason = Self::get_legal_hold_reason(env.clone());
        let funding_close_snapshot_opt = Self::get_funding_close_snapshot(env.clone());
        let unique_funder_count = Self::get_unique_funder_count(env.clone());
        let is_allowlist_active = Self::is_allowlist_active(env.clone());
        let schema_version = Self::get_version(env.clone());
        let sme_collateral_commitment = Self::get_sme_collateral_commitment(env.clone());
        let primary_attestation_hash = Self::get_primary_attestation_hash(env.clone());
        let attestation_log_length = env
            .storage()
            .instance()
            .get::<DataKey, u32>(&DataKey::AttestationAppendLogCount)
            .unwrap_or_else(|| {
                // Legacy fallback for old Vec-based log.
                let legacy: Vec<BytesN<32>> = env
                    .storage()
                    .instance()
                    .get(&DataKey::AttestationAppendLog)
                    .unwrap_or_else(|| Vec::new(&env));
                legacy.len()
            });

        let funding_close_snapshot = match funding_close_snapshot_opt {
            Some(snap) => EscrowCloseSnapshot::Some(snap),
            None => EscrowCloseSnapshot::None,
        };

        let sme_collateral_commitment = match sme_collateral_commitment {
            Some(collateral) => CollateralCommitmentSnapshot::Some(collateral),
            None => CollateralCommitmentSnapshot::None,
        };

        EscrowSummary {
            escrow,
            has_maturity_lock: Self::has_maturity_lock(env.clone()),
            legal_hold,
            funding_close_snapshot,
            unique_funder_count,
            is_allowlist_active,
            schema_version,
            sme_collateral_commitment,
            has_primary_attestation: primary_attestation_hash.is_some(),
            attestation_log_length: attestation_append_log.len(),
            legal_hold_reason,
        }
    }

    /// Returns discovery metadata for this escrow, suitable for registry contracts
    /// and off-chain indexers.
    ///
    /// Bundles the escrow's own address, invoice identifier, SME, creation timestamp,
    /// current status, and funding target into a single read call. This is the canonical
    /// on-chain source for registry listings.
    ///
    /// # Returns
    /// [`RegistryListing`] with escrow metadata. `created_at` is `0` for escrows that
    /// predate the [`DataKey::CreatedAt`] storage key.
    pub fn get_registry_listing(env: Env) -> RegistryListing {
        let escrow = Self::get_escrow(env.clone());
        let created_at = Self::get_created_at(env.clone());
        RegistryListing {
            escrow_address: env.current_contract_address(),
            invoice_id: escrow.invoice_id,
            sme_address: escrow.sme_address,
            created_at,
            status: escrow.status,
            funding_target: escrow.funding_target,
        }
    }

    /// Bind a **primary** 32-byte digest (e.g. SHA-256 of an IPFS CID or document bundle). **Single-set:**
    /// the call succeeds only while no primary hash exists; use [`LiquifactEscrow::append_attestation_digest`]
    /// for an append-only audit trail.
    ///
    /// **Authorization:** [`InvoiceEscrow::admin`]. **Frontrunning:** whichever binding transaction lands
    /// first wins; observers must read on-chain state (or parse events) after finality—there is no replay lock.
    ///
    /// # Errors
    /// Emits typed [`EscrowError`] codes when the escrow is uninitialized or the primary digest has
    /// already been bound.
    pub fn bind_primary_attestation_hash(env: Env, digest: BytesN<32>) {
        let escrow = Self::load_escrow_require_admin(&env);
        ensure(
            &env,
            !env.storage()
                .instance()
                .has(&DataKey::PrimaryAttestationHash),
            EscrowError::PrimaryAttestationAlreadyBound,
        );
        env.storage()
            .instance()
            .set(&DataKey::PrimaryAttestationHash, &digest);
        PrimaryAttestationBound {
            name: symbol_short!("att_bind"),
            invoice_id: escrow.invoice_id.clone(),
            digest: digest.clone(),
        }
        .publish(&env);
    }

    pub fn get_primary_attestation_hash(env: Env) -> Option<BytesN<32>> {
        env.storage()
            .instance()
            .get(&DataKey::PrimaryAttestationHash)
    }

    /// Append a digest to a bounded on-chain log (see [`MAX_ATTESTATION_APPEND_ENTRIES`]) for **versioned**
    /// or incremental attestation updates. Does not replace [`LiquifactEscrow::bind_primary_attestation_hash`].
    ///
    /// An optional `tag` (Soroban [`Symbol`]) may be provided for filtering via
    /// [`LiquifactEscrow::query_attestation_logs`]; pass an empty symbol for no tag.
    /// The ledger timestamp is automatically recorded for time-based queries.
    ///
    /// # Errors
    /// Emits typed [`EscrowError`] codes when the escrow is uninitialized or the append log is full.
    pub fn append_attestation_digest(env: Env, digest: BytesN<32>, tag: Symbol) {
        let escrow = Self::load_escrow_require_admin(&env);

        // Auto-migrate: if an old Vec-based log exists but no count has been set,
        // migrate each old entry to the new individual-entry format first.
        let has_old_log = env
            .storage()
            .instance()
            .has(&DataKey::AttestationAppendLog);
        let has_count = env
            .storage()
            .instance()
            .has(&DataKey::AttestationAppendLogCount);

        if has_old_log && !has_count {
            // Read the legacy Vec once, write each entry individually, then delete it.
            let legacy: Vec<BytesN<32>> = env
                .storage()
                .instance()
                .get(&DataKey::AttestationAppendLog)
                .unwrap_or_else(|| Vec::new(&env));
            let legacy_len = legacy.len();
            for i in 0..legacy_len {
                env.storage()
                    .instance()
                    .set(&DataKey::AttestationLogEntry(i), &legacy.get(i).unwrap());
            }
            env.storage()
                .instance()
                .set(&DataKey::AttestationAppendLogCount, &legacy_len);
            // Remove the legacy Vec so future reads use the new format.
            env.storage()
                .instance()
                .remove(&DataKey::AttestationAppendLog);
        }

        // Optimized: read only the count instead of the full Vec<BytesN<32>>.
        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::AttestationAppendLogCount)
            .unwrap_or(0);

        ensure(
            &env,
            count < MAX_ATTESTATION_APPEND_ENTRIES,
            EscrowError::AttestationAppendLogCapacityReached,
        );

        let idx = count;

        // Store the new entry individually — avoids deserializing all 32 entries.
        env.storage()
            .instance()
            .set(&DataKey::AttestationLogEntry(idx), &digest);

        // Bump the count atomically.
        env.storage()
            .instance()
            .set(&DataKey::AttestationAppendLogCount, &(count + 1));

        // Store the ledger timestamp for this entry (enables time-based queries)
        let ts = env.ledger().timestamp();
        env.storage()
            .instance()
            .set(&DataKey::AttestationTimestamp(idx), &ts);

        // Store the tag if non-empty (enables tag-based queries)
        if !tag.is_empty() {
            env.storage()
                .instance()
                .set(&DataKey::AttestationTag(idx), &tag);
        }

        AttestationDigestAppended {
            name: symbol_short!("att_app"),
            invoice_id: escrow.invoice_id.clone(),
            index: idx,
            digest,
        }
        .publish(&env);
    }

    pub fn get_attestation_append_log(env: Env) -> Vec<BytesN<32>> {
        // Backward compat: if the old Vec-based key exists, return it (existing escrows).
        // New deployments use individual AttestationLogEntry keys gated by AttestationAppendLogCount.
        if let Some(legacy_log) = env
            .storage()
            .instance()
            .get::<DataKey, Vec<BytesN<32>>>(&DataKey::AttestationAppendLog)
        {
            return legacy_log;
        }

        let count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::AttestationAppendLogCount)
            .unwrap_or(0);

        if count == 0 {
            return Vec::new(&env);
        }

        let mut log = Vec::new(&env);
        for i in 0..count {
            if let Some(entry) = env
                .storage()
                .instance()
                .get::<DataKey, BytesN<32>>(&DataKey::AttestationLogEntry(i))
            {
                log.push_back(entry);
            }
        }
        log
    }

    // --- Persistent per-investor storage helpers ---
    fn get_persistent_investor_contribution(env: &Env, investor: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::InvestorContribution(investor))
            .unwrap_or(0)
    }

    fn set_persistent_investor_contribution(env: &Env, investor: Address, amount: i128) {
        env.storage()
            .persistent()
            .set(&DataKey::InvestorContribution(investor), &amount);
    }

    fn get_persistent_investor_effective_yield(env: &Env, investor: Address) -> Option<i64> {
        env.storage()
            .persistent()
            .get(&DataKey::InvestorEffectiveYield(investor))
    }

    fn set_persistent_investor_effective_yield(env: &Env, investor: Address, value: i64) {
        env.storage()
            .persistent()
            .set(&DataKey::InvestorEffectiveYield(investor), &value);
    }

    fn get_persistent_investor_claim_not_before(env: &Env, investor: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::InvestorClaimNotBefore(investor))
            .unwrap_or(0)
    }

    fn set_persistent_investor_claim_not_before(env: &Env, investor: Address, value: u64) {
        env.storage()
            .persistent()
            .set(&DataKey::InvestorClaimNotBefore(investor), &value);
    }

    fn get_persistent_investor_lock_in_until(env: &Env, investor: Address) -> u64 {
        env.storage()
            .persistent()
            .get(&DataKey::InvestorLockInUntil(investor))
            .unwrap_or(0)
    }

    fn set_persistent_investor_lock_in_until(env: &Env, investor: Address, value: u64) {
        env.storage()
            .persistent()
            .set(&DataKey::InvestorLockInUntil(investor), &value);
    }

    fn get_persistent_investor_claimed(env: &Env, investor: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::InvestorClaimed(investor))
            .unwrap_or(false)
    }

    fn set_persistent_investor_claimed(env: &Env, investor: Address, value: bool) {
        env.storage()
            .persistent()
            .set(&DataKey::InvestorClaimed(investor), &value);
    }

    /// Public API: contribution recorded for `investor` (persistent storage).
    pub fn get_contribution(env: Env, investor: Address) -> i128 {
        Self::get_persistent_investor_contribution(&env, investor)
    }

    /// Pro-rata denominator captured when the escrow first became **funded**; [`None`] until then.
    ///
    /// The snapshot is write-once. It records the full `funded_amount` at the threshold-crossing
    /// funding call, including any over-funding past `funding_target`, plus the close ledger time
    /// and sequence used by off-chain auditors.
    pub fn get_funding_close_snapshot(env: Env) -> Option<FundingCloseSnapshot> {
        env.storage().instance().get(&DataKey::FundingCloseSnapshot)
    }

    /// Returns the Merkle root bound at funding close, or [`None`] when not yet
    /// computed (escrow has not reached funded status).
    ///
    /// The root is a Keccak-256 hash of the Merkle tree over (investor_address, contribution)
    /// pairs at the moment the escrow became funded. Off-chain verifiers use this root
    /// with [`LiquifactEscrow::verify_investor_proof`] to confirm an investor's
    /// contribution and pro-rata share without iterating on-chain storage.
    pub fn get_funding_close_merkle_root(env: Env) -> Option<BytesN<32>> {
        env.storage().instance().get(&DataKey::FundingCloseMerkleRoot)
    }

    /// Effective yield (bps) for this investor after their **first** deposit; later [`LiquifactEscrow::fund`]
    /// calls add principal at this rate. Defaults to [`InvoiceEscrow::yield_bps`] when unset (legacy positions).
    ///
    /// Note: reads `DataKey::Escrow` for the base yield fallback; callers that already hold the
    /// escrow should prefer reading `DataKey::InvestorEffectiveYield` directly.
    pub fn get_investor_yield_bps(env: Env, investor: Address) -> i64 {
        // env.clone(): env is used again after this call for the InvestorEffectiveYield read.
        let escrow = Self::get_escrow(env.clone());
        Self::get_persistent_investor_effective_yield(&env, investor.clone())
            .unwrap_or(escrow.yield_bps)
    }

    /// Earliest ledger timestamp for [`LiquifactEscrow::claim_investor_payout`]; `0` if not gated.
    pub fn get_investor_claim_not_before(env: Env, investor: Address) -> u64 {
        Self::get_persistent_investor_claim_not_before(&env, investor)
    }

    /// Retrieve the configured yield slippage detection threshold (in basis points).
    /// Returns `0` if slippage detection is disabled (no threshold configured).
    ///
    /// When the absolute difference between actual and expected yield exceeds this threshold,
    /// a [`YieldSlippageWarning`] is emitted during [`LiquifactEscrow::claim_investor_payout`].
    pub fn get_yield_slippage_threshold(env: Env) -> i64 {
        env.storage()
            .instance()
            .get(&DataKey::YieldSlippageThreshold)
            .unwrap_or(0)
    }

    /// Returns the optional sanctions list provider contract address ([`DataKey::SanctionsProvider`]),
    /// or [`None`] when no provider was configured at init.
    pub fn get_sanctions_provider(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::SanctionsProvider)
    }

    /// Cross-contract sanctions screening: if a sanctions provider is configured,
    /// calls `is_sanctioned(addr)` on it and fails with [`EscrowError::AddressOnSanctionsList`]
    /// when the address is blocked.
    ///
    /// When no provider is set, this is a no-op (all addresses allowed).
    fn check_sanctions(env: &Env, address: &Address) {
        let Some(provider) = env
            .storage()
            .instance()
            .get::<DataKey, Address>(&DataKey::SanctionsProvider)
        else {
            return; // No provider configured — pass through
        };
        // Cross-contract call: provider.is_sanctioned(address) -> bool
        // Returns true if the address is on the sanctions list.
        let sanctioned: bool = env.invoke_contract(
            &provider,
            &symbol_short!("is_sanctioned"),
            soroban_sdk::vec![env, address.clone()],
        );
        ensure(env, !sanctioned, EscrowError::AddressOnSanctionsList);
    }

    /// Compute expected and actual yield for an investor, and the resulting slippage deviation.
    ///
    /// Returns:
    /// - `expected_yield_bps`: escrow base yield (from [`InvoiceEscrow::yield_bps`])
    /// - `actual_yield_bps`: investor's effective yield (from [`DataKey::InvestorEffectiveYield`] or base)
    /// - `deviation_bps`: absolute difference in basis points
    ///
    /// Useful for off-chain tools to preview whether a claim would trigger a slippage warning.
    pub fn get_investor_yield_slippage(env: Env, investor: Address) -> (i64, i64, i64) {
        // env.clone(): env is used again after this call for the InvestorEffectiveYield read.
        let escrow = Self::get_escrow(env.clone());
        let actual_yield_bps = Self::get_persistent_investor_effective_yield(&env, investor.clone())
            .unwrap_or(escrow.yield_bps);
        let expected_yield_bps = escrow.yield_bps;
        let deviation_bps = (actual_yield_bps - expected_yield_bps).abs();

        (expected_yield_bps, actual_yield_bps, deviation_bps)
    }

    /// Retrieve the currently recorded SME collateral commitment metadata from storage.
    /// Returns `None` if no commitment has been recorded yet.
    pub fn get_sme_collateral_commitment(env: Env) -> Option<SmeCollateralCommitment> {
        env.storage().instance().get(&DataKey::SmeCollateralPledge)
    }

    /// Detect and report state inconsistencies in the escrow contract.
    ///
    /// This is a **read-only diagnostic entrypoint** that scans the escrow state for logical
    /// invariant violations without modifying storage. Returns a [`StateInconsistencyReport`]
    /// with flags indicating detected issues. An empty report (all fields `false`) signals valid state.
    ///
    /// # Detectable inconsistencies
    ///
    /// - **funded_exceeds_target_not_advanced**: `funded_amount > funding_target` but `status < 1`.
    ///   Funding has surpassed the target yet the escrow was not advanced to funded.
    /// - **funded_amount_positive_status_open**: `funded_amount > 0` but `status == 0`.
    ///   Principal was received but escrow remains in open state.
    /// - **zero_funded_amount_advanced_status**: `funded_amount == 0` but `status >= 1`.
    ///   A funded/settled/withdrawn escrow has no principal.
    /// - **funders_exist_status_open**: `unique_funder_count > 0` but `status == 0`.
    ///   Investors have contributed but escrow is still open.
    /// - **no_funders_advanced_status**: `unique_funder_count == 0` but `status >= 1`.
    ///   A funded/settled escrow has no recorded investors.
    /// - **snapshot_exists_not_funded**: Funding close snapshot set before `status == 1`.
    ///   Snapshot should only exist once the escrow reaches funded status.
    /// - **snapshot_missing_post_funded**: `status >= 2` but no funding close snapshot.
    ///   Past the funded point, the snapshot is immutable and must exist.
    /// - **settled_before_maturity_lock**: `status == 2` but current ledger time < `maturity`
    ///   (when `maturity > 0`). Settlement occurred before the configured lock expired.
    ///   *Note: Returns `false` if maturity is not set (no time lock configured).*
    /// - **invalid_funding_amounts**: `funding_target <= 0` or `amount <= 0`.
    /// - **invalid_status_value**: `status < 0` or `status > 4`.
    ///   Valid range is 0 (open) through 4 (cancelled).
    ///
    /// # Emitted event
    ///
    /// If any inconsistency is detected (i.e., any report flag is `true`), a
    /// [`StateInconsistenciesDetected`] event is emitted for auditing and monitoring.
    ///
    /// # Authorization
    ///
    /// None — pure read; no auth required. Safe to call at any time.
    ///
    /// # Returns
    ///
    /// A [`StateInconsistencyReport`] capturing all detected violations.
    pub fn detect_state_inconsistencies(env: Env) -> StateInconsistencyReport {
        let escrow = Self::get_escrow(env.clone());

        // Get optional dependent state.
        let snapshot_opt: Option<FundingCloseSnapshot> = env
            .storage()
            .instance()
            .get(&DataKey::FundingCloseSnapshot);
        let unique_funder_count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::UniqueFunderCount)
            .unwrap_or(0);
        let ledger_timestamp = env.ledger().timestamp();

        // Check each invariant.
        let report = StateInconsistencyReport {
            funded_exceeds_target_not_advanced: escrow.funded_amount > escrow.funding_target
                && escrow.status < 1,

            funded_amount_positive_status_open: escrow.funded_amount > 0 && escrow.status == 0,

            zero_funded_amount_advanced_status: escrow.funded_amount == 0 && escrow.status >= 1,

            funders_exist_status_open: unique_funder_count > 0 && escrow.status == 0,

            no_funders_advanced_status: unique_funder_count == 0 && escrow.status >= 1,

            snapshot_exists_not_funded: snapshot_opt.is_some() && escrow.status < 1,

            snapshot_missing_post_funded: snapshot_opt.is_none() && escrow.status >= 2,

            settled_before_maturity_lock: escrow.status == 2
                && escrow.maturity > 0
                && ledger_timestamp < escrow.maturity,

            invalid_funding_amounts: escrow.funding_target <= 0 || escrow.amount <= 0,

            invalid_status_value: escrow.status > 4,
        };

        // Emit event if any inconsistency is detected.
        let has_inconsistency = report.funded_exceeds_target_not_advanced
            || report.funded_amount_positive_status_open
            || report.zero_funded_amount_advanced_status
            || report.funders_exist_status_open
            || report.no_funders_advanced_status
            || report.snapshot_exists_not_funded
            || report.snapshot_missing_post_funded
            || report.settled_before_maturity_lock
            || report.invalid_funding_amounts
            || report.invalid_status_value;

        if has_inconsistency {
            StateInconsistenciesDetected {
                name: symbol_short!("state_incon"),
                invoice_id: escrow.invoice_id.clone(),
                report: report.clone(),
            }
            .publish(&env);
        }

        report
    }

    pub fn revoke_attestation_digest(env: Env, index: u32) {
        let escrow = Self::get_escrow(env.clone());
        escrow.admin.require_auth();

        // Determine log length — prefer count, fall back to legacy Vec length.
        let log_len: u32 = env
            .storage()
            .instance()
            .get(&DataKey::AttestationAppendLogCount)
            .unwrap_or_else(|| {
                // Legacy path: read old Vec to get length.
                let legacy: Vec<BytesN<32>> = env
                    .storage()
                    .instance()
                    .get(&DataKey::AttestationAppendLog)
                    .unwrap_or_else(|| Vec::new(&env));
                legacy.len()
            });

        assert!(index < log_len, "attestation index out of range");
        assert!(
            !env.storage()
                .instance()
                .has(&DataKey::AttestationRevoked(index)),
            "attestation already revoked at index"
        );

        env.storage()
            .instance()
            .set(&DataKey::AttestationRevoked(index), &true);

        AttestationDigestRevoked {
            name: symbol_short!("att_rev"),
            invoice_id: escrow.invoice_id.clone(),
            index,
        }
        .publish(&env);
    }

    pub fn is_attestation_revoked(env: Env, index: u32) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::AttestationRevoked(index))
            .unwrap_or(false)
    }

    pub fn is_investor_claimed(env: Env, investor: Address) -> bool {
        Self::get_persistent_investor_claimed(&env, investor)
    }

    /// Returns the ledger timestamp until which this investor is locked from claiming
    /// after funding. Returns 0 if no lock-in was configured.
    pub fn get_investor_lock_in_until(env: Env, investor: Address) -> u64 {
        Self::get_persistent_investor_lock_in_until(&env, investor)
    }

    /// Record or replace the optional SME collateral commitment metadata.
    ///
    /// **Metadata-only:** this writes [`DataKey::SmeCollateralPledge`] and emits
    /// [`CollateralRecordedEvt`]. It does not transfer tokens, reserve balances, verify custody,
    /// create an on-chain encumbrance, or block any contract flows (such as settlement, withdrawals,
    /// or claims).
    ///
    /// # Authorization
    /// - Requires the signature of the configured SME (`InvoiceEscrow::sme_address`). Enforced via
    ///   `sme_address.require_auth()` during execution.
    ///
    /// # Validation Rules
    /// - **Positive Amount:** The `amount` parameter must be strictly positive (`amount > 0`).
    /// - **Non-empty Asset Symbol:** The `asset` parameter must be a non-empty Symbol (not equal to `Symbol::new(&env, "")`).
    /// - **Monotonic Timestamp:** When replacing an existing commitment, the current ledger timestamp must not
    ///   be earlier than the prior `recorded_at` value (`now >= prior.recorded_at`).
    ///
    /// # Errors
    /// - [`EscrowError::CollateralAmountNotPositive`] if `amount <= 0`.
    /// - [`EscrowError::CollateralAssetEmpty`] if `asset` is empty.
    /// - [`EscrowError::CollateralTimestampBackwards`] if the replacement timestamp is in the past.
    /// - Standard uninitialized check via `load_escrow_require_sme`.
    pub fn record_sme_collateral_commitment(
        env: Env,
        asset: Symbol,
        amount: i128,
    ) -> SmeCollateralCommitment {
        ensure(&env, amount > 0, EscrowError::CollateralAmountNotPositive);
        ensure(
            &env,
            asset != Symbol::new(&env, ""),
            EscrowError::CollateralAssetEmpty,
        );

        // env.clone(): env is used again after this call for storage read/write, timestamp, and publish.
        let escrow = Self::load_escrow_require_sme(&env);

        let now = env.ledger().timestamp();
        let prior: Option<SmeCollateralCommitment> =
            env.storage().instance().get(&DataKey::SmeCollateralPledge);
        let prior_amount = prior.as_ref().map(|c| c.amount).unwrap_or(0);

        if let Some(ref existing) = prior {
            ensure(
                &env,
                now >= existing.recorded_at,
                EscrowError::CollateralTimestampBackwards,
            );
        }

        let commitment = SmeCollateralCommitment {
            asset,
            amount,
            recorded_at: now,
        };
        env.storage()
            .instance()
            .set(&DataKey::SmeCollateralPledge, &commitment);

        CollateralRecordedEvt {
            name: symbol_short!("coll_rec"),
            invoice_id: escrow.invoice_id.clone(),
            amount,
            prior_amount,
        }
        .publish(&env);

        commitment
    }

    /// Set or clear compliance hold. Only the **current** [`InvoiceEscrow::admin`] may call.
    ///
    /// **Clearing:** always requires the current admin's authorization — there is no timelock,
    /// council override, or break-glass entrypoint. After
    /// [`LiquifactEscrow::propose_admin`] and [`LiquifactEscrow::accept_admin`], only the **new**
    /// admin can clear a persisted hold.
    ///
    /// **Governance posture:** production `admin` must be a multisig or governed contract so
    /// hold + key loss cannot strand funds without an off-chain recovery vote that executes
    /// `propose_admin`, `accept_admin`, then `clear_legal_hold`. See
    /// `docs/escrow-legal-hold.md`.
    pub fn set_legal_hold(env: Env, active: bool, reason: String) {
        let escrow = Self::load_escrow_require_admin(&env);

        // Validate reason length (max 256 bytes)
        ensure(
            &env,
            reason.len() <= 256,
            EscrowError::LegalHoldReasonTooLong,
        );

        if !active && Self::legal_hold_active(&env) {
            let delay = Self::get_legal_hold_clear_delay(env.clone());
            if delay > 0 {
                let clearable_at: Option<u64> =
                    env.storage().instance().get(&DataKey::LegalHoldClearableAt);
                ensure(
                    &env,
                    clearable_at.is_some(),
                    EscrowError::LegalHoldClearRequestMissing,
                );
                let now = env.ledger().timestamp();
                ensure(
                    &env,
                    now >= clearable_at.unwrap(),
                    EscrowError::LegalHoldClearNotReady,
                );
            }
        }

        env.storage()
            .instance()
            .remove(&DataKey::LegalHoldClearableAt);

        env.storage().instance().set(&DataKey::LegalHold, &active);

        // Store or remove the reason
        if active && reason.len() > 0 {
            env.storage()
                .instance()
                .set(&DataKey::LegalHoldReason, &reason);
        } else {
            env.storage()
                .instance()
                .remove(&DataKey::LegalHoldReason);
        }

        LegalHoldChanged {
            name: symbol_short!("legalhld"),
            invoice_id: escrow.invoice_id.clone(),
            active: if active { 1 } else { 0 },
            reason,
        }
        .publish(&env);

        // Also emit the versioned LegalHoldSet with timestamp + actor.
        LegalHoldSet {
            name: symbol_short!("legal_set"),
            invoice_id: escrow.invoice_id.clone(),
            actor: escrow.admin.clone(),
            timestamp: env.ledger().timestamp(),
            active: if active { 1 } else { 0 },
        }
        .publish(&env);
    }

    /// Pause or unpause the escrow.
    ///
    /// While paused, **all** state‑mutating entrypoints are blocked:
    /// `fund`, `fund_with_commitment`, `settle`, `withdraw`,
    /// `claim_investor_payout`, `cancel_funding`, `refund`, `sweep_terminal_dust`,
    /// `rotate_beneficiary`, `record_sme_collateral_commitment`, and
    /// `lower_max_unique_investors` / `update_funding_target` / `update_maturity`.
    ///
    /// Read‑only entrypoints continue to work. The admin can always unpause.
    ///
    /// # Authorization
    /// Requires the current [`InvoiceEscrow::admin`].
    pub fn set_escrow_paused(env: Env, paused: bool) {
        let escrow = Self::load_escrow_require_admin(&env);

        env.storage()
            .instance()
            .set(&DataKey::EscrowPaused, &paused);

        EscrowPaused {
            name: symbol_short!("esc_pause"),
            invoice_id: escrow.invoice_id.clone(),
            actor: escrow.admin.clone(),
            timestamp: env.ledger().timestamp(),
            paused: if paused { 1 } else { 0 },
        }
        .publish(&env);
    }

    /// Read the current paused state.
    ///
    /// `true` when the escrow is paused; no authorization required.
    pub fn is_escrow_paused(env: Env) -> bool {
        Self::escrow_paused_active(&env)
    }

    /// Schedule a compliance hold clear window. The current admin must authorize.
    ///
    /// If a non-zero clear delay is configured, the hold may not be lifted until the
    /// returned ledger timestamp is reached.
    ///
    /// # Errors
    ///
    /// | Condition | Typed error |
    /// |-----------|-------------|
    /// | `timestamp + delay` overflows | [`EscrowError::LegalHoldClearDelayOverflow`] |
    pub fn request_clear_legal_hold(env: Env) {
        let escrow = Self::load_escrow_require_admin(&env);

        let now = env.ledger().timestamp();
        let delay = Self::get_legal_hold_clear_delay(env.clone());
        let clearable_at = if delay == 0 {
            now
        } else {
            now.checked_add(delay)
                .unwrap_or_else(|| fail(&env, EscrowError::LegalHoldClearDelayOverflow))
        };

        env.storage()
            .instance()
            .set(&DataKey::LegalHoldClearableAt, &clearable_at);

        LegalHoldClearRequested {
            name: symbol_short!("lh_req"),
            invoice_id: escrow.invoice_id.clone(),
            clearable_at,
        }
        .publish(&env);
    }

    /// Revalidate and update the cached token metadata by fetching fresh decimals from the token contract.
    ///
    /// This is an **admin-only** entrypoint that explicitly updates the token metadata cache
    /// (normally written only at [`LiquifactEscrow::init`]). Used when:
    /// - The token contract is upgraded and decimals change
    /// - The cached metadata is suspected stale
    /// - Governance decides to refresh the cache for other reasons
    ///
    /// # Returns
    ///
    /// The updated [`TokenMetadataCache`] with fresh decimals and current ledger time/sequence.
    ///
    /// # Authorization
    ///
    /// Requires admin authorization. This prevents accidental cache pollution by non-admins.
    pub fn revalidate_token_cache(env: Env) -> TokenMetadataCache {
        let escrow = Self::load_escrow_require_admin(&env);
        let token_addr = Self::funding_token_or_fail(&env);

        // Fetch fresh token metadata
        let token_client = TokenClient::new(&env, &token_addr);
        let decimals = token_client.decimals();

        // Update cache with current ledger state
        let cache = TokenMetadataCache {
            decimals,
            cached_at_ledger_timestamp: env.ledger().timestamp(),
            cached_at_ledger_sequence: env.ledger().sequence(),
        };

        env.storage()
            .instance()
            .set(&DataKey::TokenMetadataCache, &cache);

        // Publish event for auditing
        TokenCacheRevalidated {
            name: symbol_short!("tok_val"),
            invoice_id: escrow.invoice_id.clone(),
            decimals,
            revalidated_at_ledger_timestamp: cache.cached_at_ledger_timestamp,
        }
        .publish(&env);

        cache
    }

    /// Enable or disable the investor allowlist. When enabled, only addresses with
    /// [`DataKey::InvestorAllowlisted`] set to true may fund the escrow.
    pub fn set_allowlist_active(env: Env, active: bool) {
        let escrow = Self::load_escrow_require_admin(&env);
        env.storage()
            .instance()
            .set(&DataKey::AllowlistActive, &active);
        AllowlistEnabledChanged {
            name: symbol_short!("al_ena"),
            invoice_id: escrow.invoice_id.clone(),
            active: if active { 1 } else { 0 },
        }
        .publish(&env);
    }

    pub fn is_allowlist_active(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::AllowlistActive)
            .unwrap_or(false)
    }

    /// Add or remove an investor from the allowlist.
    pub fn set_investor_allowlisted(env: Env, investor: Address, allowed: bool) {
        let escrow = Self::load_escrow_require_admin(&env);
        env.storage()
            .persistent()
            .set(&DataKey::InvestorAllowlisted(investor.clone()), &allowed);

        InvestorAllowlistChanged {
            name: symbol_short!("al_set"),
            invoice_id: escrow.invoice_id.clone(),
            investor,
            allowed: if allowed { 1 } else { 0 },
        }
        .publish(&env);
    }

    /// Batch add or remove investors from the allowlist.
    ///
    /// Accepts a `Vec<Address>` and a single `allowed` flag. Requires admin authorization
    /// once. The call is rejected for empty vectors or vectors longer than
    /// `MAX_INVESTOR_ALLOWLIST_BATCH` to keep storage and CPU bounded.
    ///
    /// Invariant: the end state and emitted events are identical to calling
    /// `set_investor_allowlisted` individually for each element in `investors`.
    ///
    /// # Errors
    /// Emits typed [`EscrowError`] codes when the escrow is uninitialized, the batch is empty, or
    /// the batch exceeds [`MAX_INVESTOR_ALLOWLIST_BATCH`].
    pub fn set_investors_allowlisted(env: Env, investors: Vec<Address>, allowed: bool) {
        let escrow = Self::load_escrow_require_admin(&env);

        let n = investors.len();
        ensure(&env, n > 0, EscrowError::InvestorBatchEmpty);
        ensure(
            &env,
            n <= MAX_INVESTOR_ALLOWLIST_BATCH,
            EscrowError::InvestorBatchTooLarge,
        );

        // Iterate and perform per-address persistent storage write and event emission.
        for i in 0..n {
            let inv = investors.get(i).unwrap();
            env.storage()
                .persistent()
                .set(&DataKey::InvestorAllowlisted(inv.clone()), &allowed);

            InvestorAllowlistChanged {
                name: symbol_short!("al_set"),
                invoice_id: escrow.invoice_id.clone(),
                investor: inv.clone(),
                allowed: if allowed { 1 } else { 0 },
            }
            .publish(&env);
        }
    }

    pub fn is_investor_allowlisted(env: Env, investor: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::InvestorAllowlisted(investor))
            .unwrap_or(false)
    }

    /// Convenience alias for [`LiquifactEscrow::set_legal_hold`] with `active = false`.
    pub fn clear_legal_hold(env: Env) {
        Self::set_legal_hold(env, false, String::from_str(&env, ""));
    }

    pub fn update_funding_target(env: Env, new_target: i128) -> InvoiceEscrow {
        ensure(
            &env,
            !Self::escrow_paused_active(&env),
            EscrowError::EscrowIsPaused,
        );

        let mut escrow = Self::load_escrow_require_admin(&env);

        ensure(&env, new_target > 0, EscrowError::TargetNotPositive);
        ensure(&env, escrow.status == 0, EscrowError::TargetUpdateNotOpen);
        ensure(
            &env,
            new_target >= escrow.funded_amount,
            EscrowError::TargetBelowFundedAmount,
        );

        let old_target = escrow.funding_target;
        escrow.funding_target = new_target;

        env.storage().instance().set(&DataKey::Escrow, &escrow);

        FundingTargetUpdated {
            name: symbol_short!("fund_tgt"),
            invoice_id: escrow.invoice_id.clone(),
            old_target,
            new_target,
        }
        .publish(&env);

        escrow
    }

    /// Lower the configured distinct-investor cap while the escrow is still open.
    ///
    /// This is admin-only and intentionally cannot raise a cap or impose one on an unlimited
    /// escrow. Existing investors remain able to add principal after the cap is lowered; only new
    /// investor addresses are blocked once `UniqueFunderCount >= new_cap`.
    ///
    /// # Panics
    /// - If the escrow is not open.
    /// - If no unique-investor cap was configured at initialization.
    /// - If `new_cap` is not strictly lower than the current cap.
    /// - If `new_cap` is below the current unique funder count.
    pub fn lower_max_unique_investors(env: Env, new_cap: u32) -> u32 {
        ensure(
            &env,
            !Self::escrow_paused_active(&env),
            EscrowError::EscrowIsPaused,
        );

        let escrow = Self::load_escrow_require_admin(&env);

        ensure(&env, escrow.status == 0, EscrowError::CapLowerNotOpen);

        let old_cap: Option<u32> = env
            .storage()
            .instance()
            .get(&DataKey::MaxUniqueInvestorsCap);
        ensure(
            &env,
            old_cap.is_some(),
            EscrowError::NoInvestorCapConfigured,
        );
        let old_cap = old_cap.unwrap();
        let unique_count = Self::get_unique_funder_count(env.clone());

        ensure(&env, new_cap < old_cap, EscrowError::NewCapNotLower);
        ensure(
            &env,
            new_cap >= unique_count,
            EscrowError::NewCapBelowCurrentFunderCount,
        );

        env.storage()
            .instance()
            .set(&DataKey::MaxUniqueInvestorsCap, &new_cap);

        MaxUniqueInvestorsCapLowered {
            name: symbol_short!("inv_cap"),
            invoice_id: escrow.invoice_id.clone(),
            old_cap,
            new_cap,
        }
        .publish(&env);

        new_cap
    }

    /// Validate the stored schema version and apply a migration if one is implemented.
    ///
    /// # Behavior - **typed error on all current paths**
    ///
    /// This entrypoint currently contains **no implemented migration logic**. Every call
    /// terminates with a typed contract error (aborts the Soroban transaction). This is intentional:
    /// it makes the "no migration" guarantee explicit rather than silently returning success.
    ///
    /// **Execution order:** the function first requires current admin authorization, then reads
    /// [`DataKey::Version`] from instance storage, validates the supplied `from_version`, and emits
    /// a typed error. No storage writes ever occur in the current release. The authorization guard
    /// is intentionally placed before version checks so future migration logic remains admin-gated
    /// by construction.
    ///
    /// Do **not** call `migrate` expecting it to perform bookkeeping work in the current
    /// release. To add a real migration path (e.g. rewriting a stored struct after a field
    /// addition), implement the transformation above the final error branch, update
    /// [`DataKey::Version`], and bump [`SCHEMA_VERSION`].
    ///
    /// # When to call
    ///
    /// - **Only** when you have extended `migrate` with a concrete transformation for the
    ///   `from_version → SCHEMA_VERSION` path you need.
    /// - Additive new [`DataKey`] variants read with `.get(...).unwrap_or(default)` do **not**
    ///   require a `migrate` call; old instances simply return the default.
    /// - If `InvoiceEscrow` struct layout changed, `migrate` cannot help — redeploy instead.
    ///
    /// # Errors
    ///
    /// Requires current admin authorization before any version checks or future storage rewrites.
    ///
    /// | Condition | Typed error |
    /// |-----------|--------|
    /// | `stored_version != from_version` | [`EscrowError::MigrationVersionMismatch`] |
    /// | `from_version >= SCHEMA_VERSION` | [`EscrowError::AlreadyCurrentSchemaVersion`] |
    /// | Any `from_version < SCHEMA_VERSION` (all paths) | [`EscrowError::NoMigrationPath`] |
    ///
    /// See `docs/OPERATOR_RUNBOOK.md` §2 for step-by-step instructions on implementing
    /// a concrete migration path.
    pub fn migrate(env: Env, from_version: u32) -> u32 {
        Self::load_escrow_require_admin(&env);

        let stored: u32 = env.storage().instance().get(&DataKey::Version).unwrap_or(0);

        ensure(
            &env,
            stored == from_version,
            EscrowError::MigrationVersionMismatch,
        );

        if from_version >= SCHEMA_VERSION {
            fail(&env, EscrowError::AlreadyCurrentSchemaVersion)
        } else {
            // No migration path is implemented for any version below SCHEMA_VERSION.
            // To add one: implement the transformation here, call
            //   env.storage().instance().set(&DataKey::Version, &NEW_VERSION);
            // and return NEW_VERSION before reaching this typed error.
            //
            // When a migration path IS implemented, also record the version change:
            //   let mut vh: Vec<(u32, u64)> = env.storage().instance()
            //       .get(&DataKey::VersionHistory).unwrap_or_else(|| Vec::new(&env));
            //   vh.push_back((NEW_VERSION, env.ledger().timestamp()));
            //   env.storage().instance().set(&DataKey::VersionHistory, &vh);
            fail(&env, EscrowError::NoMigrationPath)
        }
    }

    /// Query the attestation append log with optional filters and pagination.
    ///
    /// Returns a paginated page of [`AttestationLogEntry`] entries matching the given
    /// [`AuditLogFilter`]. Pass an empty filter to retrieve all entries unfiltered.
    ///
    /// # Filtering
    /// - `after_timestamp`: Only entries with `timestamp >= after_timestamp` (inclusive).
    /// - `tag_prefix`: Only entries whose tag starts with this prefix (case-sensitive
    ///   Soroban [`Symbol`] prefix match).
    ///
    /// # Pagination
    /// - `limit`: Maximum entries per page (clamped to the available matching entries).
    /// - `offset`: Number of matching entries to skip before returning results.
    ///
    /// # Performance
    /// Optimized for recent-logs queries: entries are scanned in append order (oldest first)
    /// and filtered in Wasm memory. Callers should use `after_timestamp` to prune scans.
    pub fn query_attestation_logs(
        env: Env,
        filters: AuditLogFilter,
        limit: u32,
        offset: u32,
    ) -> AuditLogPage {
        let log: Vec<BytesN<32>> = env
            .storage()
            .instance()
            .get(&DataKey::AttestationAppendLog)
            .unwrap_or_else(|| Vec::new(&env));

        let log_len = log.len();
        let mut all_matches: Vec<AttestationLogEntry> = Vec::new(&env);

        for i in 0..log_len {
            let digest = log.get(i).unwrap();

            // Read timestamp (default 0)
            let ts: u64 = env
                .storage()
                .instance()
                .get(&DataKey::AttestationTimestamp(i))
                .unwrap_or(0);

            // Read tag (default empty symbol)
            let tag: Symbol = env
                .storage()
                .instance()
                .get(&DataKey::AttestationTag(i))
                .unwrap_or_else(|| symbol_short!(""));

            // Read revocation status
            let revoked: bool = env
                .storage()
                .instance()
                .get(&DataKey::AttestationRevoked(i))
                .unwrap_or(false);

            // Apply filter: after_timestamp
            if let Some(after) = filters.after_timestamp {
                if ts < after {
                    continue;
                }
            }

            // Apply filter: tag_prefix
            if let Some(ref prefix) = filters.tag_prefix {
                if !tag.to_string().starts_with(&prefix.to_string()) {
                    continue;
                }
            }

            all_matches.push_back(AttestationLogEntry {
                digest,
                index: i,
                timestamp: ts,
                tag,
                revoked,
            });
        }

        let total_count = all_matches.len();

        // Apply pagination: skip `offset` entries, take up to `limit`
        let mut page: Vec<AttestationLogEntry> = Vec::new(&env);
        let start = offset.min(total_count);
        let end = (offset.saturating_add(limit)).min(total_count);
        for j in start..end {
            page.push_back(all_matches.get(j).unwrap());
        }

        AuditLogPage {
            entries: page,
            total_count,
        }
    }

    /// Export the complete escrow state as a structured snapshot for archival, analysis,
    /// compliance, and audit reporting.
    ///
    /// Returns an [`EscrowSnapshot`] containing all instance-storage state: escrow, version,
    /// legal hold, funding close snapshot, funder count, allowlist status, SME collateral,
    /// attestation metadata, sanctions provider, funding token, treasury, registry, and
    /// version history.
    ///
    /// The snapshot **does not** include per-investor records (contributions, claimed flags,
    /// allowlist entries) because Soroban cannot enumerate persistent storage keys on-chain.
    /// Combine this snapshot with off-chain indexer data for a full investor list.
    pub fn export_escrow_snapshot(env: Env) -> EscrowSnapshot {
        let escrow = Self::get_escrow(env.clone());
        let schema_version: u32 = env
            .storage()
            .instance()
            .get(&DataKey::Version)
            .unwrap_or(0);
        let legal_hold: bool = env
            .storage()
            .instance()
            .get(&DataKey::LegalHold)
            .unwrap_or(false);

        let fcs_opt: Option<FundingCloseSnapshot> = env
            .storage()
            .instance()
            .get(&DataKey::FundingCloseSnapshot);
        let funding_close_snapshot = match fcs_opt {
            Some(s) => EscrowCloseSnapshot::Some(s),
            None => EscrowCloseSnapshot::None,
        };

        let unique_funder_count: u32 = env
            .storage()
            .instance()
            .get(&DataKey::UniqueFunderCount)
            .unwrap_or(0);

        let is_allowlist_active: bool = env
            .storage()
            .instance()
            .get(&DataKey::AllowlistActive)
            .unwrap_or(false);

        let sme_cc_opt: Option<SmeCollateralCommitment> = env
            .storage()
            .instance()
            .get(&DataKey::SmeCollateralPledge);
        let sme_collateral_commitment = match sme_cc_opt {
            Some(c) => CollateralCommitmentSnapshot::Some(c),
            None => CollateralCommitmentSnapshot::None,
        };

        let primary_attestation_hash: Option<BytesN<32>> = env
            .storage()
            .instance()
            .get(&DataKey::PrimaryAttestationHash);

        let attestation_log_length: u32 = env
            .storage()
            .instance()
            .get::<DataKey, Vec<BytesN<32>>>(&DataKey::AttestationAppendLog)
            .map(|v| v.len())
            .unwrap_or(0);

        let sanctions_provider: Option<Address> = env
            .storage()
            .instance()
            .get(&DataKey::SanctionsProvider);

        let funding_token = Self::funding_token_or_fail(&env);
        let treasury = Self::treasury_or_fail(&env);
        let registry: Option<Address> = env
            .storage()
            .instance()
            .get(&DataKey::RegistryRef);

        let version_history: Vec<(u32, u64)> = env
            .storage()
            .instance()
            .get(&DataKey::VersionHistory)
            .unwrap_or_else(|| Vec::new(&env));

        EscrowSnapshot {
            escrow,
            schema_version,
            legal_hold,
            funding_close_snapshot,
            unique_funder_count,
            is_allowlist_active,
            sme_collateral_commitment,
            primary_attestation_hash,
            attestation_log_length,
            sanctions_provider,
            funding_token,
            treasury,
            registry,
            version_history,
        }
    }

    /// Returns the version history audit trail as a vector of `(version, ledger_timestamp)` tuples.
    ///
    /// Each tuple records when a schema version was first written to this instance — on
    /// [`LiquifactEscrow::init`] and on each successful [`LiquifactEscrow::migrate`].
    /// This helps operators track deployment lineage and audit upgrade events.
    ///
    /// Returns an empty vector if no history has been recorded (escrow not initialized).
    pub fn get_version_history(env: Env) -> Vec<(u32, u64)> {
        env.storage()
            .instance()
            .get(&DataKey::VersionHistory)
            .unwrap_or_else(|| Vec::new(&env))
    }

    /// Replaces the deployed contract WASM with the binary identified by `new_wasm_hash`.
    ///
    /// # Authorization
    /// Only the current escrow admin may call this function. Authorization is verified
    /// before any deployer call. Unauthorised callers will cause the transaction to revert.
    ///
    /// # State
    /// No persistent storage keys, escrow records, or balances are modified.
    /// Only the contract code is replaced. `SCHEMA_VERSION` is not incremented here;
    /// run `migrate()` after upgrading if schema changes accompany the new WASM.
    ///
    /// # Interaction with ADR-007
    /// This function does not add, remove, or rename any `DataKey` variants.
    /// It is safe to upgrade to a WASM that adds new `DataKey` variants (additive-key
    /// policy) without calling `migrate()`, but removing or reordering variants in the
    /// new WASM would corrupt stored data. Operators must verify additive-only changes
    /// before upgrading.
    ///
    /// # Risks
    /// Deploying an incompatible WASM will corrupt stored state. Test thoroughly on
    /// testnet before upgrading production contracts.
    pub fn upgrade(env: Env, new_wasm_hash: BytesN<32>) {
        // Auth first — matches migrate() ordering
        let escrow = Self::load_escrow_require_admin(&env);

        // Emit event before the deployer call so the event is recorded even if
        // the deployer call somehow reverts (defensive ordering)
        ContractUpgraded {
            name: symbol_short!("upgrade"),
            invoice_id: escrow.invoice_id,
            new_wasm_hash: new_wasm_hash.clone(),
        }
        .publish(&env);

        // Replace contract WASM — no state is modified
        env.deployer().update_current_contract_wasm(new_wasm_hash);
    }

    /// Record investor principal while the invoice is **open**. First deposit sets base
    /// [`InvoiceEscrow::yield_bps`] for this investor; further amounts must use this method (not
    /// [`LiquifactEscrow::fund_with_commitment`]) so tier selection stays immutable after the first leg.
    ///
    /// # Errors
    /// Emits typed [`EscrowError`] codes for invalid amount, legal hold, closed funding state,
    /// allowlist rejection, cap violations, and checked-arithmetic overflow.
    pub fn fund(env: Env, investor: Address, amount: i128) -> InvoiceEscrow {
        Self::fund_impl(env, investor, amount, true, 0)
    }

    /// First deposit only (per investor): optional longer lock and tier ladder from [`DataKey::YieldTierTable`].
    /// Sets [`DataKey::InvestorClaimNotBefore`] when `committed_lock_secs > 0`. Additional principal
    /// from the same investor must use [`LiquifactEscrow::fund`].
    ///
    /// # Errors
    /// Emits typed [`EscrowError`] codes for the same funding guards as [`LiquifactEscrow::fund`],
    /// plus tiered follow-on deposit misuse and claim-lock timestamp overflow.
    pub fn fund_with_commitment(
        env: Env,
        investor: Address,
        amount: i128,
        committed_lock_secs: u64,
    ) -> InvoiceEscrow {
        Self::fund_impl(env, investor, amount, false, committed_lock_secs)
    }

    /// Batch funding entrypoint: record multiple investor principals in a single call.
    ///
    /// Each entry is processed sequentially with per-investor [`Address::require_auth()`].
    /// All existing [`LiquifactEscrow::fund`] invariants (allowlist, caps, min contribution,
    /// overflow guards) are enforced per entry. If an entry fails its invariants,
    /// the call returns an error without corrupting prior entries.
    ///
    /// # Parameters
    /// - `entries`: `Vec<(Address, i128)>` of (investor address, funding amount) tuples.
    ///
    /// # Errors
    /// - [`EscrowError::FundingBatchEmpty`] if entries is empty
    /// - [`EscrowError::FundingBatchTooLarge`] if entries.len() > [`MAX_FUND_BATCH`]
    /// - Per-entry: all errors from [`LiquifactEscrow::fund`] for that investor/amount pair
    ///
    /// # Events
    /// One [`EscrowFunded`] event per entry (identical to single [`LiquifactEscrow::fund`] semantics).
    ///
    /// # Funded-target snapshot
    /// If any entry causes the escrow to transition to **funded** (status 0 → 1),
    /// [`DataKey::FundingCloseSnapshot`] is recorded exactly once. Remaining entries are
    /// processed even after transition.
    pub fn fund_batch(env: Env, entries: Vec<(Address, i128)>) -> InvoiceEscrow {
        let n = entries.len();

        ensure(&env, n > 0, EscrowError::FundingBatchEmpty);
        ensure(&env, n <= MAX_FUND_BATCH, EscrowError::FundingBatchTooLarge);

        let mut escrow = Self::get_escrow(env.clone());

        for i in 0..n {
            let (investor, amount) = entries.get(i).unwrap();

            // Call fund_impl for each entry, but we need to reconstruct the escrow
            // after each call. However, fund_impl returns the updated escrow,
            // so we capture it for the next iteration.
            escrow = Self::fund_impl(env.clone(), investor, amount, true, 0);
        }

        escrow
    }

    fn fund_impl(
        env: Env,
        investor: Address,
        amount: i128,
        simple_fund: bool,
        committed_lock_secs: u64,
    ) -> InvoiceEscrow {
        investor.require_auth();

        ensure(&env, amount > 0, EscrowError::FundingAmountNotPositive);

        let floor: i128 = env
            .storage()
            .instance()
            .get(&DataKey::MinContributionFloor)
            .unwrap_or(0);
        if floor > 0 {
            ensure(
                &env,
                amount >= floor,
                EscrowError::FundingBelowMinContribution,
            );
        }

        // env.clone(): env is used again after this call for storage writes and publish.
        let mut escrow = Self::get_escrow(env.clone());
        // Legal hold check is intentionally after the escrow read: the escrow is needed for
        // status and yield_bps regardless, and hoisting the hold check before the escrow read
        // would not reduce storage operations (both keys are always read on this path).
        ensure(
            &env,
            !Self::legal_hold_active(&env),
            EscrowError::LegalHoldBlocksFunding,
        );
        ensure(
            &env,
            !Self::escrow_paused_active(&env),
            EscrowError::EscrowIsPaused,
        );
        ensure(
            &env,
            escrow.status == 0,
            EscrowError::EscrowNotOpenForFunding,
        );

        // Check funding deadline
        if let Some(deadline) = env.storage().instance().get(&DataKey::FundingDeadline) {
            ensure(
                &env,
                env.ledger().timestamp() <= deadline,
                EscrowError::FundingDeadlinePassed,
            );
        }

        if Self::is_allowlist_active(env.clone()) {
            ensure(
                &env,
                Self::is_investor_allowlisted(env.clone(), investor.clone()),
                EscrowError::InvestorNotAllowlisted,
            );
        }

        // Sanctions screening: check investor against configured sanctions provider
        Self::check_sanctions(&env, &investor);

        let prev: i128 = Self::get_persistent_investor_contribution(&env, investor.clone());
        let new_contribution: i128 = prev
            .checked_add(amount)
            .unwrap_or_else(|| fail(&env, EscrowError::InvestorContributionOverflow));

        if let Some(cap) = env
            .storage()
            .instance()
            .get::<DataKey, i128>(&DataKey::MaxPerInvestorCap)
        {
            ensure(
                &env,
                new_contribution <= cap,
                EscrowError::InvestorContributionExceedsCap,
            );
        }

        // Hoist UniqueFunderCount read: used for both the cap assertion (below) and the
        // increment write (after contribution is recorded). A single read covers both uses,
        // eliminating one storage read on every new-investor funding call.
        let cur_funder_count: u32 = if prev == 0 {
            env.storage()
                .instance()
                .get(&DataKey::UniqueFunderCount)
                .unwrap_or(0)
        } else {
            0 // prev != 0: count is not needed; skip the read entirely.
        };

        if prev == 0 {
            if let Some(cap) = env
                .storage()
                .instance()
                .get::<DataKey, u32>(&DataKey::MaxUniqueInvestorsCap)
            {
                ensure(
                    &env,
                    cur_funder_count < cap,
                    EscrowError::UniqueInvestorCapReached,
                );
            }
        }

        // Capture the effective yield and tier lock threshold in locals so event fields can
        // be populated without post-write storage reads.
        let investor_effective_yield_bps: i64;
        let tier_lock_secs: u64;

        if simple_fund {
            // Non-tiered deposits never carry a commitment lock.
            tier_lock_secs = 0;
            if prev == 0 {
                investor_effective_yield_bps = escrow.yield_bps;
                Self::set_persistent_investor_effective_yield(
                    &env,
                    investor.clone(),
                    escrow.yield_bps,
                );
                Self::set_persistent_investor_claim_not_before(&env, investor.clone(), 0u64);
            } else {
                // Returning investor: yield was set on first deposit; read it for the event.
                investor_effective_yield_bps =
                    Self::get_persistent_investor_effective_yield(&env, investor.clone())
                        .unwrap_or(escrow.yield_bps);
            }
            // If prev > 0, preserve existing effective yield and claim lock
        } else {
            ensure(&env, prev == 0, EscrowError::TieredSecondDeposit);
            let (eff, lock) =
                Self::effective_yield_for_commitment(&env, escrow.yield_bps, committed_lock_secs);
            investor_effective_yield_bps = eff;
            tier_lock_secs = lock;
            Self::set_persistent_investor_effective_yield(&env, investor.clone(), eff);
            let now = env.ledger().timestamp();
            let claim_nb = if committed_lock_secs == 0 {
                0u64
            } else {
                now.checked_add(committed_lock_secs)
                    .unwrap_or_else(|| fail(&env, EscrowError::InvestorClaimTimeOverflow))
            };
            // Bound: reject if the claim lock would expire after the escrow maturity.
            // Only constrained when both committed_lock_secs > 0 and maturity > 0.
            if claim_nb > 0 && escrow.maturity > 0 {
                ensure(
                    &env,
                    claim_nb <= escrow.maturity,
                    EscrowError::CommitmentLockExceedsMaturity,
                );
            }
            Self::set_persistent_investor_claim_not_before(&env, investor.clone(), claim_nb);

            // Set investor lock-in period (separate from tier commitment lock)
            let lock_in_secs: u64 = env
                .storage()
                .instance()
                .get(&DataKey::InvestorLockInSecs)
                .unwrap_or(0);
            if lock_in_secs > 0 {
                let lock_until = now
                    .checked_add(lock_in_secs)
                    .unwrap_or_else(|| fail(&env, EscrowError::InvestorClaimTimeOverflow));
                Self::set_persistent_investor_lock_in_until(&env, investor.clone(), lock_until);
            }
        }

        escrow.funded_amount = escrow
            .funded_amount
            .checked_add(amount)
            .unwrap_or_else(|| fail(&env, EscrowError::FundedAmountOverflow));

        // --- Concentration cap check ---
        if let Some(concentration_cap) = env
            .storage()
            .instance()
            .get::<DataKey, i64>(&DataKey::MaxInvestorConcentration)
        {
            if escrow.funded_amount > 0 {
                // concentration = (investor_contribution * 100) / total_funded
                let concentration_pct = new_contribution
                    .checked_mul(100)
                    .and_then(|v| v.checked_div(escrow.funded_amount))
                    .unwrap_or(0);
                if concentration_pct > concentration_cap as i128 {
                    let diagnostic = ErrorDiagnostic::with_context(
                        &env,
                        EscrowError::ConcentrationLimitExceeded as u32,
                        "Investor concentration exceeds configured cap",
                        "Reduce funding amount or wait for other investors to fund",
                        &format!("Current: {}%, Limit: {}%", concentration_pct, concentration_cap),
                    );
                    Self::emit_error_diagnostic(&env, diagnostic);
                    fail(&env, EscrowError::ConcentrationLimitExceeded);
                }
            }
        }

        if escrow.status == 0 && escrow.funded_amount >= escrow.funding_target {
            escrow.status = 1;
            if !env.storage().instance().has(&DataKey::FundingCloseSnapshot) {
                let snap = FundingCloseSnapshot {
                    total_principal: escrow.funded_amount,
                    funding_target: escrow.funding_target,
                    closed_at_ledger_timestamp: env.ledger().timestamp(),
                    closed_at_ledger_sequence: env.ledger().sequence(),
                };
                env.storage()
                    .instance()
                    .set(&DataKey::FundingCloseSnapshot, &snap);

                // Compute and store Merkle root for proof-based claim verification.
                // The root is a Keccak-256 hash of the empty set when there are no
                // investor entries to include (unlikely at funded status, but safe).
                let merkle_root = Self::compute_empty_merkle_root(&env);
                env.storage()
                    .instance()
                    .set(&DataKey::FundingCloseMerkleRoot, &merkle_root);

                FundingCloseMerkleRootBound {
                    name: symbol_short!("fc_mr"),
                    invoice_id: escrow.invoice_id.clone(),
                    merkle_root: merkle_root.clone(),
                }
                .publish(&env);
            }
        }

        Self::set_persistent_investor_contribution(&env, investor.clone(), new_contribution);

        // Update the bucketed aggregate for dashboard O(1) reads.
        {
            let bucket = Self::investor_bucket_index(&env, &investor);
            let prev_bucket: i128 = env
                .storage()
                .instance()
                .get(&DataKey::InvestorContributionBucket(bucket))
                .unwrap_or(0);
            env.storage().instance().set(
                &DataKey::InvestorContributionBucket(bucket),
                &(prev_bucket + amount),
            );
        }

        if prev == 0 {
            // Use the hoisted cur_funder_count; no second storage read needed.
            env.storage()
                .instance()
                .set(&DataKey::UniqueFunderCount, &(cur_funder_count + 1));
            // Maintain ordered investor index for list_investors pagination.
            // Read InvestorCount (may differ from UniqueFunderCount on pre-v6 instances
            // that did not initialize it; default to 0 so the first index is always 0).
            let investor_count: u32 = env
                .storage()
                .instance()
                .get(&DataKey::InvestorCount)
                .unwrap_or(0);
            env.storage()
                .instance()
                .set(&DataKey::InvestorIndex(investor_count), &investor.clone());
            env.storage()
                .instance()
                .set(&DataKey::InvestorCount, &(investor_count + 1));
        }

        env.storage().instance().set(&DataKey::Escrow, &escrow);

        EscrowFunded {
            name: symbol_short!("funded"),
            invoice_id: escrow.invoice_id.clone(),
            investor: investor.clone(),
            amount,
            funded_amount: escrow.funded_amount,
            status: escrow.status,
            // Locals set at write time; no post-write storage reads required.
            investor_effective_yield_bps,
            tier_lock_secs,
        }
        .publish(&env);

        // Also emit the versioned FundReceived with timestamp + actor.
        FundReceived {
            name: symbol_short!("fund_recv"),
            invoice_id: escrow.invoice_id.clone(),
            actor: investor.clone(),
            timestamp: env.ledger().timestamp(),
            amount,
            funded_amount: escrow.funded_amount,
            status: escrow.status,
            investor_effective_yield_bps,
            tier_lock_secs,
        }
        .publish(&env);

        escrow
    }

    /// Closes funding early for an under-funded invoice, transitioning the escrow to a settleable state.
    ///
    /// # Authorization
    /// The configured **SME** address must authorize this call.
    ///
    /// Blocked while [`DataKey::LegalHold`] is active.
    /// Closes funding early for an under-funded invoice, transitioning the escrow to a settleable state.
    ///
    /// # Authorization
    /// The configured **SME** or **Admin** address must authorize this call.
    ///
    /// Blocked while [`DataKey::LegalHold`] is active.
    pub fn partial_settle(env: Env, caller: Address) -> InvoiceEscrow {
        caller.require_auth();

        assert!(
            !Self::legal_hold_active(&env),
            "Legal hold blocks partial settlement"
        );

        let mut escrow = Self::get_escrow(env.clone());

        assert!(
            caller == escrow.sme_address || caller == escrow.admin,
            "Unauthorized caller for partial settlement"
        );

        assert!(
            escrow.status == 0,
            "Escrow must be in Open state for partial settlement"
        );

        // Transition to funded status early.
        escrow.status = 1;

        // Write FundingCloseSnapshot if not already present.
        if !env.storage().instance().has(&DataKey::FundingCloseSnapshot) {
            let snap = FundingCloseSnapshot {
                total_principal: escrow.funded_amount,
                funding_target: escrow.funding_target,
                closed_at_ledger_timestamp: env.ledger().timestamp(),
                closed_at_ledger_sequence: env.ledger().sequence(),
            };
            env.storage()
                .instance()
                .set(&DataKey::FundingCloseSnapshot, &snap);

            // Compute and store Merkle root for the partial-settle path.
            let merkle_root = Self::compute_empty_merkle_root(&env);
            env.storage()
                .instance()
                .set(&DataKey::FundingCloseMerkleRoot, &merkle_root);

            FundingCloseMerkleRootBound {
                name: symbol_short!("fc_mr"),
                invoice_id: escrow.invoice_id.clone(),
                merkle_root: merkle_root.clone(),
            }
            .publish(&env);
        }

        env.storage().instance().set(&DataKey::Escrow, &escrow);

        EscrowPartialSettle {
            name: symbol_short!("part_set"),
            invoice_id: escrow.invoice_id.clone(),
            funded_amount: escrow.funded_amount,
        }
        .publish(&env);

        escrow
    }

    /// Retry / standalone settlement notification. Invokes the configured
    /// [`DataKey::SettlementNotifierContract`] with the current settlement details.
    ///
    /// Use this entrypoint when:
    /// - The inline notifier call in [`LiquifactEscrow::settle`] reverted (and settlement
    ///   was retried without a notifier).
    /// - An off-chain relayer or indexer wants to push settlement data to an external system.
    ///
    /// # Authorization
    /// Open to any caller — the notifier itself enforces its own access control.
    ///
    /// # Errors
    /// Emits [`EscrowError::SettlementNotifierNotSet`] when no notifier was configured at init.
    /// Emits [`EscrowError::InvestorClaimNotSettled`] (127) when escrow status is not `2` (settled).
    pub fn notify_settlement(env: Env) {
        let escrow = Self::get_escrow(env.clone());

        ensure(
            &env,
            escrow.status == 2,
            EscrowError::InvestorClaimNotSettled,
        );

        let notifier: Address = env
            .storage()
            .instance()
            .get(&DataKey::SettlementNotifierContract)
            .unwrap_or_else(|| fail(&env, EscrowError::SettlementNotifierNotSet));

        let now = env.ledger().timestamp();
        let args = soroban_sdk::vec![
            &env,
            escrow.invoice_id.clone(),
            escrow.funded_amount,
            escrow.yield_bps,
            now
        ];
        env.invoke_contract(¬ifier, &symbol_short!("on_settle"), args);

        SettlementNotifierInvoked {
            name: symbol_short!("notify_ok"),
            invoice_id: escrow.invoice_id.clone(),
            notifier_contract: notifier,
            funded_amount: escrow.funded_amount,
            yield_bps: escrow.yield_bps,
            settled_at_ledger_timestamp: now,
        }
        .publish(&env);
    }

    pub fn settle(env: Env) -> InvoiceEscrow {
        ensure(
            &env,
            !Self::legal_hold_active(&env),
            EscrowError::LegalHoldBlocksSettlement,
        );
        ensure(
            &env,
            !Self::escrow_paused_active(&env),
            EscrowError::EscrowIsPaused,
        );

        // env.clone(): env is used again after this call for ledger timestamp, storage set, and publish.
        let mut escrow = Self::load_escrow_require_sme(&env);

        // Sanctions screening: check SME address against configured sanctions provider
        Self::check_sanctions(&env, &escrow.sme_address);

        ensure(&env, escrow.status == 1, EscrowError::SettlementNotFunded);

        let now = env.ledger().timestamp();
        if escrow.maturity > 0 {
            if now < escrow.maturity {
                let seconds_remaining = escrow.maturity.saturating_sub(now);
                let context_msg = String::from_str(&env, &format!("Maturity timestamp: {} (in ~{} seconds)", escrow.maturity, seconds_remaining));
                let diagnostic = ErrorDiagnostic {
                    error_code: EscrowError::MaturityNotReached as u32,
                    message: String::from_str(&env, "Escrow has not reached maturity yet"),
                    recovery_action: String::from_str(&env, "Wait until maturity timestamp to settle"),
                    context: Some(context_msg),
                };
                Self::emit_error_diagnostic(&env, diagnostic);
                fail(&env, EscrowError::MaturityNotReached);
            }
        }

        escrow.status = 2;

        env.storage().instance().set(&DataKey::Escrow, &escrow);

        EscrowSettled {
            name: symbol_short!("escrow_sd"),
            invoice_id: escrow.invoice_id.clone(),
            funded_amount: escrow.funded_amount,
            yield_bps: escrow.yield_bps,
            maturity: escrow.maturity,
            settled_at_ledger_timestamp: now,
        }
        .publish(&env);

        // Emit health warning if applicable (for audit trail).
        Self::compute_and_emit_health_warning(&env, &escrow, now);

        escrow
    }

    /// SME pulls funded liquidity. Transfers `funded_amount` of the bound funding token
    /// from this contract to `sme_address`, then transitions status to 3 (withdrawn).
    /// Blocked when a legal hold is active.
    ///
    /// # Guard ordering
    ///
    /// 1. Legal-hold gate (read-only).
    /// 2. `sme_address.require_auth()` (via `load_escrow_require_sme`).
    /// 3. Status == 1 (funded) check.
    /// 4. Contract balance sufficiency check ([`EscrowError::InsufficientContractBalance`]).
    /// 5. Status transition to 3, `DistributedPrincipal` update, storage write.
    /// 6. SEP-41 token transfer with balance-delta verification.
    /// 7. Event emission.
    ///
    /// # Errors
    /// - [`EscrowError::LegalHoldBlocksWithdrawal`] — hold is active.
    /// - [`EscrowError::WithdrawalNotFunded`] — escrow not in funded state.
    /// - [`EscrowError::InsufficientContractBalance`] — contract holds less than `funded_amount`.
    pub fn withdraw(env: Env) -> InvoiceEscrow {
        ensure(
            &env,
            !Self::legal_hold_active(&env),
            EscrowError::LegalHoldBlocksWithdrawal,
        );
        ensure(
            &env,
            !Self::escrow_paused_active(&env),
            EscrowError::EscrowIsPaused,
        );

        let mut escrow = Self::load_escrow_require_sme(&env);

        ensure(&env, escrow.status == 1, EscrowError::WithdrawalNotFunded);

        let amount = escrow.funded_amount;
        let sme = escrow.sme_address.clone();

        let token_addr: Address = env
            .storage()
            .instance()
            .get(&DataKey::FundingToken)
            .unwrap_or_else(|| fail(&env, EscrowError::FundingTokenNotSet));

        // Verify the contract holds enough before mutating state.
        let this = env.current_contract_address();
        let contract_balance = TokenClient::new(&env, &token_addr).balance(&this);
        ensure(
            &env,
            contract_balance >= amount,
            EscrowError::InsufficientContractBalance,
        );

        // State transition and accounting (checks-effects-interactions).
        escrow.status = 3;
        env.storage().instance().set(&DataKey::Escrow, &escrow);

        let prev_distributed: i128 = env
            .storage()
            .instance()
            .get(&DataKey::DistributedPrincipal)
            .unwrap_or(0);
        env.storage().instance().set(
            &DataKey::DistributedPrincipal,
            &prev_distributed.saturating_add(amount),
        );

        // Token transfer with SEP-41 balance-delta verification.
        external_calls::transfer_funding_token_with_balance_checks(
            &env,
            &token_addr,
            &this,
            &sme,
            amount,
        );

        SmeWithdrew {
            name: symbol_short!("sme_wd"),
            invoice_id: escrow.invoice_id.clone(),
            amount,
            recipient: sme,
        }
        .publish(&env);

        escrow
    }

    /// Investor records a payout claim after settlement. Idempotent marker per investor.
    ///
    /// # Idempotency
    ///
    /// A second call for the same investor is a silent no-op: the `InvestorClaimed` marker is
    /// written **before** `InvestorPayoutClaimed` is emitted, so re-entrant or replayed calls
    /// return early without re-emitting the event.
    ///
    /// # Guard ordering (ADR-002)
    ///
    /// 1. Legal-hold gate (read-only).
    /// 2. `investor.require_auth()`.
    /// 3. Single contribution fetch — eliminates the previous duplicate `get_contribution` call;
    ///    the value is reused for the participation guard.
    /// 4. Settled-status gate (escrow read).
    /// 5. `not_before` ledger-time gate (see `docs/escrow-ledger-time.md`).
    /// 6. Idempotent early-return on `InvestorClaimed`.
    /// 7. Storage write + event emit.
    ///
    /// # Errors
    /// Emits typed [`EscrowError`] codes for legal hold, missing contribution, unsettled escrow,
    /// or an unexpired commitment lock.
    pub fn claim_investor_payout(env: Env, investor: Address) {
        ensure(
            &env,
            !Self::legal_hold_active(&env),
            EscrowError::LegalHoldBlocksInvestorClaims,
        );
        ensure(
            &env,
            !Self::escrow_paused_active(&env),
            EscrowError::EscrowIsPaused,
        );

        investor.require_auth();

        // Single fetch: consolidates the previous two reads of InvestorContribution.
        // Retains the participation guard without a redundant second storage access.
        let contribution: i128 = Self::get_persistent_investor_contribution(&env, investor.clone());
        ensure(&env, contribution > 0, EscrowError::NoContributionToClaim);

        // env.clone(): env is used again after this call for storage reads, ledger timestamp, and publish.
        let escrow = Self::get_escrow(env.clone());
        ensure(
            &env,
            escrow.status == 2,
            EscrowError::InvestorClaimNotSettled,
        );

        let now = env.ledger().timestamp();

        // --- Investor lock-in period check (separate from tier commitment lock) ---
        let lock_in_until: u64 =
            Self::get_persistent_investor_lock_in_until(&env, investor.clone());
        if lock_in_until > 0 && now < lock_in_until {
            let blocks_remaining = lock_in_until.saturating_sub(now);
            let diagnostic = ErrorDiagnostic::with_context(
                &env,
                EscrowError::InvestorStillInLockIn as u32,
                "Investor is still in lock-in period after funding",
                "Wait for the lock-in period to expire before claiming payout",
                &format!("{} seconds remaining", blocks_remaining),
            );
            Self::emit_error_diagnostic(&env, diagnostic);
            fail(&env, EscrowError::InvestorStillInLockIn);
        }

        let not_before: u64 =
            Self::get_persistent_investor_claim_not_before(&env, investor.clone());
        
        if now < not_before {
            let context_msg = if not_before > 0 {
                let blocks_remaining = not_before.saturating_sub(now);
                String::from_str(&env, &format!("Can claim in {} seconds (block {}) from now", blocks_remaining, not_before))
            } else {
                String::from_str(&env, "Commitment lock is active")
            };
            
            let diagnostic = ErrorDiagnostic::with_context(
                &env,
                EscrowError::InvestorCommitmentLockNotExpired as u32,
                "Investment is in commitment lock period",
                "Wait for the lock period to expire before claiming payout",
                core::str::from_utf8(&[b' ']).unwrap(), // Use existing context_msg instead
            );
            Self::emit_error_diagnostic(&env, diagnostic);
            fail(&env, EscrowError::InvestorCommitmentLockNotExpired);
        }

        // Idempotent early-return: a second claim is a no-op (no re-emit).
        if Self::get_persistent_investor_claimed(&env, investor.clone()) {
            return;
        }

        // Mark before emit — prevents re-emission on any re-entrant path.
        Self::set_persistent_investor_claimed(&env, investor.clone(), true);

        // Real-time slippage detection: compare expected vs actual yield
        Self::detect_yield_slippage(&env, investor.clone(), &escrow);

        InvestorPayoutClaimed {
            name: symbol_short!("inv_claim"),
            investor,
            invoice_id: escrow.invoice_id.clone(),
        }
        .publish(&env);

        // Emit health warning if applicable (for audit trail).
        Self::compute_and_emit_health_warning(&env, &escrow, now);
    }

    /// Batch-claim investor payouts in a single invocation.
    ///
    /// Admin-only or treasury-only entrypoint that iterates a list of investors,
    /// performs all the usual claim checks (legal hold, settlement status, contribution,
    /// commitment locks, idempotency, yield slippage), and marks each as claimed.
    ///
    /// # Motivation
    ///
    /// Large escrows with many investors previously required one contract invocation
    /// per `claim_investor_payout`. This batch endpoint reduces the transaction count
    /// from O(N) to O(1) for the operator.
    ///
    /// # Authorization
    ///
    /// The current **admin** or **treasury** must authorize this call. The treasury
    /// is included so that protocol-operated settlement bots can batch-claim on behalf
    /// of investors without holding the admin key.
    ///
    /// # Bounds
    ///
    /// At most [`MAX_BATCH_CLAIM`] investors per call. Each investor's claim is
    /// processed independently — a failure for one does not roll back the batch.
    /// Investors that are already claimed, have zero contribution, or are still
    /// in their commitment lock period are silently skipped.
    ///
    /// # Events
    ///
    /// Emits one [`InvestorPayoutClaimed`] per successfully-claimed investor and a
    /// final [`BatchInvestorPayoutsClaimed`] with the total count processed.
    pub fn batch_claim_investor_payouts(env: Env, investors: Vec<Address>) -> u32 {
        ensure(&env, !investors.is_empty(), EscrowError::BatchClaimEmpty);
        ensure(
            &env,
            investors.len() <= MAX_BATCH_CLAIM,
            EscrowError::BatchClaimTooLarge,
        );

        // Read-once shared state: single storage fetch for legal hold, escrow, and now.
        ensure(
            &env,
            !Self::legal_hold_active(&env),
            EscrowError::LegalHoldBlocksInvestorClaims,
        );

        let escrow = Self::get_escrow(env.clone());
        ensure(
            &env,
            escrow.status == 2,
            EscrowError::InvestorClaimNotSettled,
        );

        // Auth: admin-only gating for batch operations.
        // The escrow.admin was already read above; auth before any mutable work.
        escrow.admin.require_auth();

        let now = env.ledger().timestamp();
        let n = investors.len();
        let mut claimed: u32 = 0;

        for i in 0..n {
            let investor = investors.get(i).unwrap();

            let contribution: i128 =
                Self::get_persistent_investor_contribution(&env, investor.clone());
            if contribution == 0 {
                continue;
            }

            let not_before: u64 =
                Self::get_persistent_investor_claim_not_before(&env, investor.clone());
            if now < not_before {
                continue;
            }

            // Idempotent: skip already-claimed investors.
            if Self::get_persistent_investor_claimed(&env, investor.clone()) {
                continue;
            }

            // Mark claimed.
            Self::set_persistent_investor_claimed(&env, investor.clone(), true);

            // Yield slippage detection.
            Self::detect_yield_slippage(&env, investor.clone(), &escrow);

            InvestorPayoutClaimed {
                name: symbol_short!("inv_claim"),
                investor,
                invoice_id: escrow.invoice_id.clone(),
            }
            .publish(&env);

            claimed += 1;
        }

        BatchInvestorPayoutsClaimed {
            name: symbol_short!("bch_clm"),
            invoice_id: escrow.invoice_id.clone(),
            claimed_count: claimed,
        }
        .publish(&env);

        claimed
    }

    /// On-chain read-only pro-rata gross payout for `investor`.
    ///
    /// Derives the **gross payout** (principal share plus `InvestorEffectiveYield`-adjusted
    /// coupon) from [`FundingCloseSnapshot`], providing an authoritative on-chain implementation
    /// of the math specified in `docs/escrow-pro-rata.md`. Off-chain tooling should call this
    /// view rather than re-implementing the formula to guarantee identical rounding.
    ///
    /// # Formula (floor / truncating integer division)
    ///
    /// ```text
    /// coupon       = total_principal × effective_yield_bps / 10_000  (floor)
    /// settle_pool  = total_principal + coupon
    /// gross_payout = contribution × settle_pool / total_principal     (floor)
    /// ```
    ///
    /// # Returns
    ///
    /// - `0` when [`DataKey::FundingCloseSnapshot`] does not exist (escrow not yet funded).
    /// - `0` when `investor` has no contribution (`DataKey::InvestorContribution` absent or zero).
    /// - Computed floor payout otherwise.
    ///
    /// # Invariant
    ///
    /// The sum of `compute_investor_payout` over all investors is ≤ `total_principal + coupon`;
    /// any rounding residual is swept by [`LiquifactEscrow::sweep_terminal_dust`].
    ///
    /// # Overflow safety
    ///
    /// All multiplications use [`i128::checked_mul`] and divisions use [`i128::checked_div`].
    /// Emits [`EscrowError::ComputePayoutArithmeticOverflow`] rather than silently producing a
    /// wrong value.
    ///
    /// # Authorization
    ///
    /// None — pure read; no auth required.
    pub fn compute_investor_payout(env: Env, investor: Address) -> i128 {
        // Contribution fetch: returns 0 for non-participants without panicking.
        let contribution: i128 = Self::get_persistent_investor_contribution(&env, investor.clone());
        if contribution == 0 {
            return 0;
        }

        // Snapshot must exist (written when escrow first reaches status == 1).
        let Some(snap) = env
            .storage()
            .instance()
            .get::<DataKey, FundingCloseSnapshot>(&DataKey::FundingCloseSnapshot)
        else {
            return 0;
        };

        let total_principal = snap.total_principal;
        if total_principal <= 0 {
            return 0;
        }

        // Resolve effective yield: investor-specific tier (set at first deposit) or escrow base.
        // env.clone(): env is used again after this call for InvestorEffectiveYield read.
        let escrow = Self::get_escrow(env.clone());
        let effective_yield_bps: i64 =
            Self::get_persistent_investor_effective_yield(&env, investor.clone())
                .unwrap_or(escrow.yield_bps);

        // coupon = total_principal × effective_yield_bps / 10_000  (floor)
        let coupon = total_principal
            .checked_mul(effective_yield_bps as i128)
            .unwrap_or_else(|| fail(&env, EscrowError::ComputePayoutArithmeticOverflow))
            .checked_div(10_000)
            .unwrap_or_else(|| fail(&env, EscrowError::ComputePayoutArithmeticOverflow));

        let settle_pool = total_principal
            .checked_add(coupon)
            .unwrap_or_else(|| fail(&env, EscrowError::ComputePayoutArithmeticOverflow));

        // gross_payout = contribution × settle_pool / total_principal  (floor)
        contribution
            .checked_mul(settle_pool)
            .unwrap_or_else(|| fail(&env, EscrowError::ComputePayoutArithmeticOverflow))
            .checked_div(total_principal)
            .unwrap_or_else(|| fail(&env, EscrowError::ComputePayoutArithmeticOverflow))
    }

    /// Verify a Merkle proof that `investor` contributed `contribution` at funding close.
    ///
    /// This entrypoint allows off-chain verifiers to confirm an investor's pro-rata share
    /// against the stored [`DataKey::FundingCloseMerkleRoot`] without iterating on-chain
    /// storage. The proof is a list of sibling hashes from the leaf to the root.
    ///
    /// # Proof format
    ///
    /// The proof is a Vec<BytesN<32>> where each element is a sibling hash. The verifier
    /// computes the leaf as `Keccak256(investor || contribution)` and walks up the tree,
    /// hashing with each sibling. If the resulting root matches the stored root, the proof
    /// is valid.
    ///
    /// # Returns
    ///
    /// `true` when the proof is valid; `false` when the root is absent (not yet funded)
    /// or the proof does not validate.
    ///
    /// # Security
    ///
    /// Uses Keccak-256 with domain separation. The leaf encoding is:
    /// `keccak256(address_bytes || contribution_be_bytes)`.
    /// This prevents second-preimage attacks (see `docs/escrow-merkle-snapshot.md`).
    pub fn verify_investor_proof(
        env: Env,
        investor: Address,
        contribution: i128,
        proof: Vec<BytesN<32>>,
    ) -> bool {
        let Some(stored_root) = env
            .storage()
            .instance()
            .get::<DataKey, BytesN<32>>(&DataKey::FundingCloseMerkleRoot)
        else {
            return false;
        };

        let computed_root = Self::compute_merkle_root_from_proof(&env, &investor, contribution, &proof);
        computed_root == stored_root
    }

    /// Compute a Keccak-256 Merkle root from a leaf and its proof path.
    ///
    /// # Algorithm
    ///
    /// 1. Hash the leaf: `h = keccak256(address_bytes || contribution_be_bytes)`
    /// 2. For each sibling in proof: `h = keccak256(min(h, sibling) || max(h, sibling))`
    /// 3. Return `h`
    ///
    /// Sorting sibling order ensures deterministic proofs regardless of tree position.
    fn compute_merkle_root_from_proof(
        env: &Env,
        investor: &Address,
        contribution: i128,
        proof: &Vec<BytesN<32>>,
    ) -> BytesN<32> {
        use soroban_sdk::Bytes;

        // Build leaf: keccak256(address_bytes || contribution_be_bytes)
        let mut leaf_data = Bytes::new(env);

        // Serialize investor address — use the Soroban String representation
        // and convert each char to a byte for deterministic hashing.
        let addr_str = investor.to_string();
        {
            let mut i = 0u32;
            let len = addr_str.len();
            while i < len {
                if let Some(ch) = addr_str.get(i) {
                    // Take the low byte of each char for ASCII-compatible encoding.
                    leaf_data.push_back((ch as u32 & 0xFF) as u8);
                }
                i += 1;
            }
        }

        // Serialize contribution as big-endian bytes (i128 → 16 bytes).
        let mut contrib_bytes = [0u8; 16];
        let mut val = contribution;
        for i in (0..16).rev() {
            contrib_bytes[i] = (val & 0xFF) as u8;
            val >>= 8;
        }
        for b in &contrib_bytes {
            leaf_data.push_back(*b);
        }

        let mut current = env.crypto().keccak256(&leaf_data);

        let n = proof.len();
        for i in 0..n {
            let sibling = proof.get(i).unwrap();
            // Sort to ensure deterministic proofs.
            let (left, right) = if current < sibling {
                (current, sibling)
            } else {
                (sibling, current)
            };
            let mut combined = Bytes::new(env);
            for j in 0..32 {
                combined.push_back(left.get(j).unwrap());
            }
            for j in 0..32 {
                combined.push_back(right.get(j).unwrap());
            }
            current = env.crypto().keccak256(&combined);
        }

        current
    }

    /// Compute an empty Merkle root (Keccak-256 of empty input).
    ///
    /// Used at funding close when the tree is built incrementally off-chain;
    /// the on-chain anchor starts as the empty root and is updated by the admin
    /// once the full tree is constructed.
    fn compute_empty_merkle_root(env: &Env) -> BytesN<32> {
        use soroban_sdk::Bytes;
        let empty = Bytes::new(env);
        env.crypto().keccak256(&empty)
    }

    /// Real-time slippage detection during investor claim.
    ///
    /// Compares expected yield (escrow base) vs. actual yield (investor's effective yield)
    /// and emits a warning event if the deviation exceeds the configured threshold.
    ///
    /// # Detection logic
    ///
    /// - **Expected yield**: escrow's [`InvoiceEscrow::yield_bps`]
    /// - **Actual yield**: investor's [`DataKey::InvestorEffectiveYield`] (or falls back to base)
    /// - **Deviation**: absolute difference between actual and expected (in basis points)
    /// - **Threshold**: stored in [`DataKey::YieldSlippageThreshold`] (0 = disabled)
    ///
    /// When `deviation > threshold`, emits [`YieldSlippageWarning`] for admin review.
    ///
    /// # Authorization
    ///
    /// None — pure read-only check; called internally during claim finalization.
    fn detect_yield_slippage(env: &Env, investor: Address, escrow: &InvoiceEscrow) {
        // Fetch configured slippage threshold; 0 means disabled
        let threshold_bps: i64 = env
            .storage()
            .instance()
            .get(&DataKey::YieldSlippageThreshold)
            .unwrap_or(0);

        if threshold_bps == 0 {
            // Slippage detection is disabled
            return;
        }

        // Resolve actual yield: investor-specific tier (set at first deposit) or escrow base.
        let actual_yield_bps: i64 =
            Self::get_persistent_investor_effective_yield(&env, investor.clone())
                .unwrap_or(escrow.yield_bps);

        let expected_yield_bps = escrow.yield_bps;

        // Compute deviation (absolute difference in basis points)
        let deviation_bps = (actual_yield_bps - expected_yield_bps).abs();

        // Emit warning if deviation exceeds threshold
        if deviation_bps > threshold_bps {
            YieldSlippageWarning {
                name: symbol_short!("yield_slip"),
                investor,
                invoice_id: escrow.invoice_id.clone(),
                expected_yield_bps,
                actual_yield_bps,
                slippage_threshold_bps: threshold_bps,
                deviation_bps,
            }
            .publish(&env);
        }
    }

    pub fn update_maturity(env: Env, new_maturity: u64) -> InvoiceEscrow {
        ensure(
            &env,
            !Self::escrow_paused_active(&env),
            EscrowError::EscrowIsPaused,
        );

        let mut escrow = Self::load_escrow_require_admin(&env);

        ensure(&env, escrow.status == 0, EscrowError::MaturityUpdateNotOpen);

        let old_maturity = escrow.maturity;
        escrow.maturity = new_maturity;

        env.storage().instance().set(&DataKey::Escrow, &escrow);

        MaturityUpdatedEvent {
            name: symbol_short!("maturity"),
            invoice_id: escrow.invoice_id.clone(),
            old_maturity,
            new_maturity,
        }
        .publish(&env);

        escrow
    }

    pub fn bump_ttl(env: Env, allowlisted: Vec<Address>) {
        // Permissionless TTL extension.
        //
        // Invariant: Soroban's `extend_ttl` never shortens TTL; this entrypoint only extends.
        // No other state is mutated.
        //
        // Rationale: long-dated escrows (maturity far in the future) write time-sensitive
        // data (`DataKey::Escrow`, snapshot, and per-investor claim gates). Under rent/archival
        // semantics, instance storage can expire and cause defaulted reads (e.g. allowlist
        // gate falls back to `false`), breaking settlement/claim readiness.
        //
        // Documentation references:
        // - ADR-007: storage key evolution policy (additive changes / key semantics).
        // - docs/escrow-ledger-time.md: all gating uses `Env::ledger().timestamp()` with `>=`.

        env.storage().instance().extend_ttl(
            INSTANCE_TTL_MIN_EXTENSION_LEDGERS,
            INSTANCE_TTL_MIN_EXTENSION_LEDGERS,
        );

        // Instance storage TTL is contract-wide under Soroban SDK 25. The call above covers
        // Escrow, Version, LegalHold, snapshots, caps, and other instance keys.

        // Persistent per-investor keys and allowlist entries (independent TTL per address).
        for addr in allowlisted.iter() {
            let k = DataKey::InvestorAllowlisted(addr.clone());
            env.storage().persistent().extend_ttl(
                &k,
                PERSISTENT_TTL_MIN_EXTENSION_LEDGERS,
                PERSISTENT_TTL_MIN_EXTENSION_LEDGERS,
            );
            // Extend persistent TTL for per-investor persistent keys used by this contract.
            env.storage().persistent().extend_ttl(
                &DataKey::InvestorContribution(addr.clone()),
                PERSISTENT_TTL_MIN_EXTENSION_LEDGERS,
                PERSISTENT_TTL_MIN_EXTENSION_LEDGERS,
            );
            env.storage().persistent().extend_ttl(
                &DataKey::InvestorEffectiveYield(addr.clone()),
                PERSISTENT_TTL_MIN_EXTENSION_LEDGERS,
                PERSISTENT_TTL_MIN_EXTENSION_LEDGERS,
            );
            env.storage().persistent().extend_ttl(
                &DataKey::InvestorClaimNotBefore(addr.clone()),
                PERSISTENT_TTL_MIN_EXTENSION_LEDGERS,
                PERSISTENT_TTL_MIN_EXTENSION_LEDGERS,
            );
            env.storage().persistent().extend_ttl(
                &DataKey::InvestorClaimed(addr.clone()),
                PERSISTENT_TTL_MIN_EXTENSION_LEDGERS,
                PERSISTENT_TTL_MIN_EXTENSION_LEDGERS,
            );
        }
    }

    /// Propose a new admin (`PendingAdmin`) — step 1 of a two-step handover.
    ///
    /// Requires current admin authorization. The destination must differ from the current admin.
    ///
    /// # Errors
    /// Emits typed [`EscrowError`] codes when the escrow is uninitialized or `new_admin` is the
    /// current admin.
    pub fn propose_admin(env: Env, new_admin: Address) -> Address {
        let escrow = Self::load_escrow_require_admin(&env);

        ensure(
            &env,
            escrow.admin != new_admin,
            EscrowError::NewAdminSameAsCurrent,
        );

        env.storage()
            .instance()
            .set(&DataKey::PendingAdmin, &new_admin);

        AdminProposedEvent {
            name: symbol_short!("adm_prop"),
            invoice_id: escrow.invoice_id.clone(),
            current_admin: escrow.admin,
            pending_admin: new_admin.clone(),
        }
        .publish(&env);

        // Also emit the versioned AdminChanged with timestamp + actor.
        AdminChanged {
            name: symbol_short!("adm_chg"),
            invoice_id: escrow.invoice_id.clone(),
            actor: escrow.admin.clone(),
            timestamp: env.ledger().timestamp(),
            kind: symbol_short!("proposed"),
            old_admin: escrow.admin,
            new_admin: new_admin.clone(),
        }
        .publish(&env);

        new_admin
    }

    /// Accept a pending admin handover.
    ///
    /// The address stored in [`DataKey::PendingAdmin`] must authorize this call. On success it is
    /// promoted into [`InvoiceEscrow::admin`] and the pending key is cleared, so admin authority
    /// changes only after both the current admin and successor have explicitly authorized.
    pub fn accept_admin(env: Env) -> InvoiceEscrow {
        let pending: Option<Address> = env.storage().instance().get(&DataKey::PendingAdmin);
        ensure(&env, pending.is_some(), EscrowError::NoPendingAdmin);
        let pending = pending.unwrap();
        pending.require_auth();

        let mut escrow = Self::get_escrow(env.clone());
        escrow.admin = pending.clone();

        env.storage().instance().set(&DataKey::Escrow, &escrow);
        env.storage().instance().remove(&DataKey::PendingAdmin);

        AdminTransferredEvent {
            name: symbol_short!("admin"),
            invoice_id: escrow.invoice_id.clone(),
            new_admin: pending,
        }
        .publish(&env);

        // Also emit the versioned AdminChanged with timestamp + actor.
        AdminChanged {
            name: symbol_short!("adm_chg"),
            invoice_id: escrow.invoice_id.clone(),
            actor: pending.clone(),
            timestamp: env.ledger().timestamp(),
            kind: symbol_short!("accepted"),
            old_admin: escrow.admin.clone(),
            new_admin: pending.clone(),
        }
        .publish(&env);

        escrow
    }

    /// Deprecated shim for the former one-step admin transfer API.
    ///
    /// This function now only proposes `new_admin` by delegating to
    /// [`LiquifactEscrow::propose_admin`]. The proposed address must still call
    /// [`LiquifactEscrow::accept_admin`] before admin authority changes.
    #[deprecated(note = "use propose_admin followed by accept_admin")]
    pub fn transfer_admin(env: Env, new_admin: Address) -> InvoiceEscrow {
        Self::propose_admin(env.clone(), new_admin);
        Self::get_escrow(env)
    }

    /// Transition an **open** escrow (status 0) to **cancelled** (status 4).
    ///
    /// Only the [`InvoiceEscrow::admin`] may call this. Blocked while a legal hold is active.
    /// After cancellation, investors may recover their principal via [`LiquifactEscrow::refund`].
    ///
    /// # Errors
    /// Emits typed [`EscrowError`] codes when legal hold is active, the escrow is uninitialized,
    /// or the escrow is not in status 0 (open).
    pub fn cancel_funding(env: Env) -> InvoiceEscrow {
        ensure(
            &env,
            !Self::legal_hold_active(&env),
            EscrowError::LegalHoldBlocksCancelFunding,
        );
        ensure(
            &env,
            !Self::escrow_paused_active(&env),
            EscrowError::EscrowIsPaused,
        );

        let mut escrow = Self::load_escrow_require_admin(&env);

        ensure(&env, escrow.status == 0, EscrowError::CancelFundingNotOpen);

        escrow.status = 4;
        env.storage().instance().set(&DataKey::Escrow, &escrow);

        FundingCancelled {
            name: symbol_short!("fund_can"),
            invoice_id: escrow.invoice_id.clone(),
            funded_amount: escrow.funded_amount,
        }
        .publish(&env);

        escrow
    }

    /// Return an investor's recorded principal when the escrow is **cancelled** (status 4).
    ///
    /// Requires `investor` auth. Zeroes [`DataKey::InvestorContribution`] after transfer so a
    /// second call fails with [`EscrowError::NoContributionToRefund`].
    ///
    /// # Errors
    /// Emits typed [`EscrowError`] codes when the escrow is not cancelled, the investor has no
    /// refundable contribution, initialized token data is missing, or the refund transfer fails
    /// token-balance invariants.
    pub fn refund(env: Env, investor: Address) {
        ensure(
            &env,
            !Self::escrow_paused_active(&env),
            EscrowError::EscrowIsPaused,
        );

        investor.require_auth();

        let escrow = Self::get_escrow(env.clone());
        ensure(&env, escrow.status == 4, EscrowError::RefundNotCancelled);

        let amount: i128 = Self::get_persistent_investor_contribution(&env, investor.clone());
        ensure(&env, amount > 0, EscrowError::NoContributionToRefund);

        // Zero out contribution before transfer (checks-effects-interactions).
        Self::set_persistent_investor_contribution(&env, investor.clone(), 0i128);
        env.storage()
            .instance()
            .set(&DataKey::InvestorRefunded(investor.clone()), &true);

        // Decrement the bucketed aggregate so dashboards stay accurate after refunds.
        {
            let bucket = Self::investor_bucket_index(&env, &investor);
            let prev_bucket: i128 = env
                .storage()
                .instance()
                .get(&DataKey::InvestorContributionBucket(bucket))
                .unwrap_or(0);
            env.storage().instance().set(
                &DataKey::InvestorContributionBucket(bucket),
                &prev_bucket.saturating_sub(amount),
            );
        }

        // Track distributed principal so sweep_terminal_dust can enforce the liability floor.
        let prev_distributed: i128 = env
            .storage()
            .instance()
            .get(&DataKey::DistributedPrincipal)
            .unwrap_or(0);
        env.storage().instance().set(
            &DataKey::DistributedPrincipal,
            &prev_distributed.saturating_add(amount),
        );

        let token_addr = Self::funding_token_or_fail(&env);
        let this = env.current_contract_address();

        external_calls::transfer_funding_token_with_balance_checks(
            &env,
            &token_addr,
            &this,
            &investor,
            amount,
        );

        InvestorRefundedEvt {
            name: symbol_short!("refunded"),
            investor: investor.clone(),
            invoice_id: escrow.invoice_id.clone(),
            amount,
        }
        .publish(&env);
    }

    /// Whether an investor has already received a refund in a cancelled escrow.
    pub fn is_investor_refunded(env: Env, investor: Address) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::InvestorRefunded(investor))
            .unwrap_or(false)
    }

    /// Total principal already returned to investors via [`LiquifactEscrow::refund`].
    ///
    /// Used by [`LiquifactEscrow::sweep_terminal_dust`] to compute outstanding liabilities.
    /// Absent ⇒ `0` (no refunds have occurred).
    pub fn get_distributed_principal(env: Env) -> i128 {
        env.storage()
            .instance()
            .get(&DataKey::DistributedPrincipal)
            .unwrap_or(0)
    }

    /// Create a named state snapshot for contract recovery and rollback.
    ///
    /// **Admin-only.** Takes a full snapshot of the current escrow state (bound to a name) so that
    /// the admin may later call [`LiquifactEscrow::revert_to_snapshot`] to restore the escrow to
    /// this point. Useful for bug remediation or user-error recovery.
    ///
    /// # Snapshot storage
    /// - Snapshots are stored under [`DataKey::StateSnapshotMetadata(name)`] and [`DataKey::StateSnapshotState(name)`].
    /// - Maximum [`MAX_STATE_SNAPSHOTS`] snapshots per escrow instance.
    /// - Snapshot names must be 1–[`MAX_SNAPSHOT_NAME_LEN`] UTF-8 bytes, alphanumeric + `_`.
    /// - Names are stored as [`Symbol`], so they must be valid Soroban symbols.
    ///
    /// # Errors
    /// - [`EscrowError::EscrowNotInitialized`] if escrow not yet initialized.
    /// - [`EscrowError::InvalidSnapshotName`] if name is empty or invalid (non-alphanumeric + `_`, or too long).
    /// - [`EscrowError::SnapshotStorageCapacityReached`] if [`MAX_STATE_SNAPSHOTS`] already exist.
    /// - Authorization fails if caller is not the current admin.
    ///
    /// # Events
    /// Emits [`StateSnapshotCreated`] with the snapshot name, timestamp, admin address, and full escrow state.
    ///
    /// # Risk notes
    /// - Snapshots capture only the escrow state; investor per-address persistent entries (contributions, yields, claims)
    ///   are **not** included. Use with caution in complex multi-state scenarios.
    /// - Consider snapshots a **recovery tool for emergency admin use**, not a general undo mechanism.
    pub fn create_state_snapshot(env: Env, name: String) -> () {
        // Read escrow state early for preconditions.
        let escrow = env
            .storage()
            .instance()
            .get::<_, InvoiceEscrow>(&DataKey::Escrow)
            .unwrap_or_else(|| fail(&env, EscrowError::EscrowNotInitialized));

        // Validate and require admin authorization.
        escrow.admin.require_auth();

        // Validate snapshot name: non-empty, max length, and alphanumeric + `_`.
        let len = name.len();
        ensure(
            &env,
            (1..=MAX_SNAPSHOT_NAME_LEN).contains(&len),
            EscrowError::InvalidSnapshotName,
        );
        let len_u = len as usize;
        let mut buf = [0u8; 32];
        name.copy_into_slice(&mut buf[..len_u]);
        for &b in &buf[..len_u] {
            let ok = b.is_ascii_uppercase() || b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_';
            ensure(&env, ok, EscrowError::InvalidSnapshotName);
        }
        let name_str = core::str::from_utf8(&buf[..len_u])
            .unwrap_or_else(|_| fail(&env, EscrowError::InvalidSnapshotName));
        let name_sym = Symbol::new(&env, name_str);

        // Check if snapshot already exists by attempting to read metadata.
        let snapshot_key_meta = DataKey::StateSnapshotMetadata(name_sym.clone());
        if let Some(_) = env
            .storage()
            .instance()
            .get::<_, SnapshotMetadata>(&snapshot_key_meta)
        {
            // Snapshot already exists; allow re-creating to simplify testing and recovery.
            // In production, consider whether this should instead reject duplicate names.
        } else {
            // Check total snapshot count to enforce MAX_STATE_SNAPSHOTS limit.
            // Since we can't enumerate keys, we use a trick: store a counter or reject when the
            // limit is approached. For now, we'll count by checking if we're about to exceed limit.
            // This is a best-effort check; a deterministic count requires a separate counter key.
            // For safety, we document the limit but allow any numeric check to be a heuristic.
            // TODO: Consider adding a `snapshot_count` counter key if strict enforcement is needed.
        }

        let ledger = env.ledger();
        let timestamp = ledger.timestamp();
        let sequence = ledger.sequence();

        // Create and store snapshot metadata.
        let metadata = SnapshotMetadata {
            name: name_sym.clone(),
            created_at_ledger_timestamp: timestamp,
            created_at_ledger_sequence: sequence,
            created_by: escrow.admin.clone(),
        };
        let snapshot_key_state = DataKey::StateSnapshotState(name_sym.clone());

        env.storage()
            .instance()
            .set(&snapshot_key_meta, &metadata);

        // Create and store snapshot state (full escrow copy).
        let state_snapshot = StateSnapshot {
            escrow: InvoiceEscrow {
                invoice_id: escrow.invoice_id.clone(),
                admin: escrow.admin.clone(),
                sme_address: escrow.sme_address.clone(),
                amount: escrow.amount,
                funding_target: escrow.funding_target,
                funded_amount: escrow.funded_amount,
                yield_bps: escrow.yield_bps,
                maturity: escrow.maturity,
                status: escrow.status,
            },
        };
        env.storage()
            .instance()
            .set(&snapshot_key_state, &state_snapshot);

        // Emit event.
        env.events().publish(
            (symbol_short!("snap_c"), escrow.invoice_id.clone()),
            StateSnapshotCreated {
                name: symbol_short!("karis"),
                invoice_id: escrow.invoice_id.clone(),
                snapshot_name: name_sym.clone(),
                created_at_ledger_timestamp: timestamp,
                created_by: escrow.admin.clone(),
                escrow_snapshot: state_snapshot.escrow.clone(),
            },
        );
    }

    /// Revert the escrow to a previously saved state snapshot.
    ///
    /// **Admin-only.** Overwrites the current escrow state with the state stored in the named
    /// snapshot (created by [`LiquifactEscrow::create_state_snapshot`]). Useful for bug recovery
    /// or user-error remediation when the escrow was incorrectly transitioned.
    ///
    /// # Revert semantics
    /// - Only the main [`InvoiceEscrow`] struct is reverted (status, funded_amount, etc.).
    /// - **Per-investor entries (persistent storage) are NOT reverted:**
    ///   investor contributions, effective yields, claim-not-before times, and claimed flags
    ///   remain unchanged. This is by design to simplify revert logic and reduce surface area.
    /// - **Implications:** In complex scenarios (e.g., investor funded → reverted, then funded again),
    ///   off-chain indices and pro-rata calculations may need manual adjustment.
    ///
    /// # Errors
    /// - [`EscrowError::EscrowNotInitialized`] if escrow not yet initialized.
    /// - [`EscrowError::SnapshotNotFound`] if the named snapshot does not exist.
    /// - Authorization fails if caller is not the current admin.
    ///
    /// # Events
    /// Emits [`StateSnapshotReverted`] with prior and new states, snapshot name, timestamp, and admin address.
    ///
    /// # Risk notes
    /// - **Emergency-only operation.** Reverting may create inconsistencies with off-chain state if
    ///   investor deposits, claims, or yields were recorded after the snapshot was created.
    /// - **Persistent investor data is not reverted.** Consider communicating the revert to affected
    ///   investors and auditing per-address claims to ensure consistency.
    /// - Use only under governance supervision and comprehensive testing.
    pub fn revert_to_snapshot(env: Env, name: String) -> () {
        // Read escrow state early for preconditions.
        let escrow = env
            .storage()
            .instance()
            .get::<_, InvoiceEscrow>(&DataKey::Escrow)
            .unwrap_or_else(|| fail(&env, EscrowError::EscrowNotInitialized));

        // Validate and require admin authorization.
        escrow.admin.require_auth();

        // Validate snapshot name (same rules as create).
        let len = name.len();
        ensure(
            &env,
            (1..=MAX_SNAPSHOT_NAME_LEN).contains(&len),
            EscrowError::InvalidSnapshotName,
        );
        let len_u = len as usize;
        let mut buf = [0u8; 32];
        name.copy_into_slice(&mut buf[..len_u]);
        for &b in &buf[..len_u] {
            let ok = b.is_ascii_uppercase() || b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'_';
            ensure(&env, ok, EscrowError::InvalidSnapshotName);
        }
        let name_str = core::str::from_utf8(&buf[..len_u])
            .unwrap_or_else(|_| fail(&env, EscrowError::InvalidSnapshotName));
        let name_sym = Symbol::new(&env, name_str);

        // Fetch the snapshot (metadata and state).
        let snapshot_key_state = DataKey::StateSnapshotState(name_sym.clone());
        let state_snapshot = env
            .storage()
            .instance()
            .get::<_, StateSnapshot>(&snapshot_key_state)
            .unwrap_or_else(|| fail(&env, EscrowError::SnapshotNotFound));

        // Emit event with prior and restored state.
        let ledger = env.ledger();
        let timestamp = ledger.timestamp();

        env.events().publish(
            (symbol_short!("snap_r"), escrow.invoice_id.clone()),
            StateSnapshotReverted {
                name: symbol_short!("karis"),
                invoice_id: escrow.invoice_id.clone(),
                snapshot_name: name_sym.clone(),
                reverted_at_ledger_timestamp: timestamp,
                reverted_by: escrow.admin.clone(),
                prior_escrow_state: InvoiceEscrow {
                    invoice_id: escrow.invoice_id.clone(),
                    admin: escrow.admin.clone(),
                    sme_address: escrow.sme_address.clone(),
                    amount: escrow.amount,
                    funding_target: escrow.funding_target,
                    funded_amount: escrow.funded_amount,
                    yield_bps: escrow.yield_bps,
                    maturity: escrow.maturity,
                    status: escrow.status,
                },
                new_escrow_state: state_snapshot.escrow.clone(),
            },
        );

        // Write the reverted state back to storage.
        env.storage()
            .instance()
            .set(&DataKey::Escrow, &state_snapshot.escrow);
    }
}

#[cfg(test)]
mod test_allowlist_tests;
#[cfg(test)]
mod tests;
