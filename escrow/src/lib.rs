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
//! An admin may set [`DataKey::LegalHold`] to block state-mutating entrypoints until cleared.
//! Hold management, admin handover, initialization, and [`LiquifactEscrow::bump_ttl`] remain
//! available as explicit recovery/maintenance exceptions. **Clearing** requires the **current**
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

extern crate alloc;
use alloc::format;

use core::{clone::Clone, default::Default};
use soroban_sdk::{
    contract, contracterror, contractevent, contractimpl, contracttype, panic_with_error,
    symbol_short, token::TokenClient, Address, Bytes, BytesN, Env, Executable, String, Symbol, Vec,
};

mod admin;
mod attestations;
mod constants;
mod contract;
mod errors;
mod events;
mod funding;
mod legal_hold;
mod settlement;
mod storage;
mod types;
pub use constants::*;
pub use contract::*;
pub use errors::EscrowError;
pub(crate) use errors::{ensure, fail};
pub use events::*;
pub use storage::DataKey;
pub(crate) use storage::{PerInvestorStorage, PerInvestorStorageExt};
pub use types::*;

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
/// | 7 | Added `updated_at` to `SmeCollateralCommitment`; `bind_primary_attestation_hash` now accepts `Bytes` with explicit length validation (#207, #208) | **Redeploy required** — `SmeCollateralCommitment` XDR shape changed |
/// | 8 | Added optional `InvoiceEscrow::guardian` for two-step legal-hold activation | **Redeploy required** — `InvoiceEscrow` XDR shape changed |
///
/// See `docs/OPERATOR_RUNBOOK.md` for the full redeploy-vs-upgrade decision tree.
pub const SCHEMA_VERSION: u32 = 8;

/// Interface ABI version for the deployed escrow contract.
///
/// This value is extracted by `escrow/build.rs` and must stay aligned with the
/// public entrypoint signatures used by SDKs and external clients.
pub const CONTRACT_INTERFACE_VERSION: u32 = 1;

// --- Data types ---

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

#[cfg(test)]
mod test_allowlist_tests;
#[cfg(test)]
mod tests;
