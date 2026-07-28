//!
//! Tests for the `clone_settled_escrow` entrypoint.
//!
//! This module covers cloning a settled escrow template to create new independent
//! escrow instances with the same configuration parameters.
//!
//! # Clone model
//!
//! `clone_settled_escrow` takes a settled escrow (status == 2) as a template and
//! creates a fresh escrow with:
//! - Cloned: admin, sme_address, yield_bps, maturity, registry, token, treasury,
//!   yield_tiers, min_contribution, max_unique_investors, max_per_investor,
//!   legal_hold_clear_delay, funding_deadline
//! - New (caller-supplied): invoice_id, amount
//! - Reset: funded_amount = 0, status = 0 (open), all per-investor state

#[cfg(test)]
use super::{default_init, deploy, free_addresses, TARGET};
use crate::LiquifactEscrow;
use soroban_sdk::{
    testutils::{Address as _, Events},
    Address, Env, String,
};

// ──────────────────────────────────────────────────────────────────────────────
// Happy path tests
// ──────────────────────────────────────────────────────────────────────────────

/// Clone a settled escrow with minimal (no optional) config into a new instance.
#[test]
fn test_clone_settled_escrow_happy_path() {
    let env = Env::default();
    
    // Deploy and init template escrow
    let (template_client, _, _) = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let treasury = Address::generate(&env);

    template_client.init(
        &admin,
        &String::from_str(&env, "TEMPLATE"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &template_client.funding_token(),
        &None,
        &treasury,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );

    // Fund and settle the template
    let investor = Address::generate(&env);
    template_client.fund(&investor, &TARGET);
    template_client.settle();

    // Deploy target escrow for cloning
    let target_id = env.register(LiquifactEscrow, ());
    let target_client = super::LiquifactEscrowClient::new(&env, &target_id);

    let new_invoice_id = String::from_str(&env, "NEW_INV_001");
    let new_amount = 500_000i128;

    // Clone the settled escrow
    target_client.clone_settled_escrow(
        &env,
        &new_invoice_id,
        &new_amount,
    );

    // Verify new escrow state
    let template_summary = template_client.get_escrow_summary();
    let new_summary = target_client.get_escrow_summary();

    // Check cloned fields
    assert_eq!(new_summary.escrow.admin, admin, "admin should match");
    assert_eq!(new_summary.escrow.sme_address, sme, "sme should match");
    assert_eq!(
        new_summary.escrow.yield_bps, template_summary.escrow.yield_bps,
        "yield_bps should match"
    );
    assert_eq!(
        new_summary.escrow.maturity, template_summary.escrow.maturity,
        "maturity should match"
    );

    // Check reset fields
    assert_eq!(new_summary.escrow.amount, new_amount, "amount should be new");
    assert_eq!(
        new_summary.escrow.funding_target, new_amount,
        "funding_target should be new"
    );
    assert_eq!(
        new_summary.escrow.funded_amount, 0,
        "funded_amount should be 0"
    );
    assert_eq!(new_summary.escrow.status, 0, "status should be open");
    assert_eq!(
        new_summary.unique_funder_count, 0,
        "unique_funder_count should be 0"
    );
}

/// Clone fails if template escrow is not settled (status != 2).
#[test]
fn test_clone_settled_escrow_not_settled() {
    let env = Env::default();
    let (template_client, _, _) = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let treasury = Address::generate(&env);

    template_client.init(
        &admin,
        &String::from_str(&env, "OPEN_TEMPLATE"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &template_client.funding_token(),
        &None,
        &treasury,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );

    // Create target but don't fund/settle template
    let target_id = env.register(LiquifactEscrow, ());
    let target_client = super::LiquifactEscrowClient::new(&env, &target_id);

    // Try to clone open (not settled) escrow - should fail with CloneNotSettled (170)
    let result = target_client.try_clone_settled_escrow(
        &env,
        &String::from_str(&env, "SHOULD_FAIL"),
        &500_000i128,
    );

    assert!(result.is_err(), "clone should fail on non-settled escrow");
}

/// Clone fails with non-positive amount.
#[test]
fn test_clone_settled_escrow_zero_amount() {
    let env = Env::default();
    
    // Deploy and settle template
    let (template_client, _, _) = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let treasury = Address::generate(&env);

    template_client.init(
        &admin,
        &String::from_str(&env, "TEMPLATE"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &template_client.funding_token(),
        &None,
        &treasury,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );

    let investor = Address::generate(&env);
    template_client.fund(&investor, &TARGET);
    template_client.settle();

    // Deploy target
    let target_id = env.register(LiquifactEscrow, ());
    let target_client = super::LiquifactEscrowClient::new(&env, &target_id);

    // Try with zero amount - should fail with CloneAmountNotPositive (171)
    let result = target_client.try_clone_settled_escrow(
        &env,
        &String::from_str(&env, "ZERO"),
        &0i128,
    );

    assert!(result.is_err(), "clone should fail with zero amount");
}

/// Original template escrow is not modified after clone.
#[test]
fn test_clone_settled_escrow_template_unchanged() {
    let env = Env::default();
    
    let (template_client, _, _) = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let treasury = Address::generate(&env);

    template_client.init(
        &admin,
        &String::from_str(&env, "TEMPLATE"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &template_client.funding_token(),
        &None,
        &treasury,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );

    let investor = Address::generate(&env);
    template_client.fund(&investor, &TARGET);
    template_client.settle();

    let template_summary_before = template_client.get_escrow_summary();

    // Create target and clone
    let target_id = env.register(LiquifactEscrow, ());
    let target_client = super::LiquifactEscrowClient::new(&env, &target_id);

    target_client.clone_settled_escrow(
        &env,
        &String::from_str(&env, "CLONE_1"),
        &500_000i128,
    );

    let template_summary_after = template_client.get_escrow_summary();

    // Verify template is identical
    assert_eq!(
        template_summary_before.escrow.invoice_id,
        template_summary_after.escrow.invoice_id,
        "template invoice_id should not change"
    );
    assert_eq!(
        template_summary_before.escrow.amount,
        template_summary_after.escrow.amount,
        "template amount should not change"
    );
    assert_eq!(
        template_summary_before.escrow.status,
        template_summary_after.escrow.status,
        "template status should not change"
    );
}

/// After cloning, the new escrow can be funded normally.
#[test]
fn test_clone_settled_escrow_then_fund() {
    let env = Env::default();
    
    let (template_client, _, _) = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let treasury = Address::generate(&env);

    template_client.init(
        &admin,
        &String::from_str(&env, "TEMPLATE"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &template_client.funding_token(),
        &None,
        &treasury,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );

    let investor = Address::generate(&env);
    template_client.fund(&investor, &TARGET);
    template_client.settle();

    // Create and clone
    let target_id = env.register(LiquifactEscrow, ());
    let target_client = super::LiquifactEscrowClient::new(&env, &target_id);

    let new_invoice_id = String::from_str(&env, "FUND_TEST");
    let new_amount = 500_000i128;

    target_client.clone_settled_escrow(
        &env,
        &new_invoice_id,
        &new_amount,
    );

    // Fund the cloned escrow
    let investor2 = Address::generate(&env);
    target_client.fund(&investor2, &new_amount);

    let summary = target_client.get_escrow_summary();
    assert_eq!(
        summary.escrow.funded_amount, new_amount,
        "cloned escrow should be fundable"
    );
    assert_eq!(summary.escrow.status, 1, "status should become funded");
}

/// After cloning, the new escrow can be settled normally.
#[test]
fn test_clone_settled_escrow_then_settle() {
    let env = Env::default();
    
    let (template_client, _, _) = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let treasury = Address::generate(&env);

    template_client.init(
        &admin,
        &String::from_str(&env, "TEMPLATE"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &template_client.funding_token(),
        &None,
        &treasury,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );

    let investor = Address::generate(&env);
    template_client.fund(&investor, &TARGET);
    template_client.settle();

    // Create and clone
    let target_id = env.register(LiquifactEscrow, ());
    let target_client = super::LiquifactEscrowClient::new(&env, &target_id);

    let new_invoice_id = String::from_str(&env, "SETTLE_TEST");
    let new_amount = 500_000i128;

    target_client.clone_settled_escrow(
        &env,
        &new_invoice_id,
        &new_amount,
    );

    // Fund and settle the cloned escrow
    let investor2 = Address::generate(&env);
    target_client.fund(&investor2, &new_amount);
    target_client.settle();

    let summary = target_client.get_escrow_summary();
    assert_eq!(
        summary.escrow.status, 2,
        "cloned escrow should be settleable"
    );
}

/// Multiple independent clones can be created from the same template.
#[test]
fn test_clone_settled_escrow_idempotent() {
    let env = Env::default();
    
    let (template_client, _, _) = deploy(&env);
    let admin = Address::generate(&env);
    let sme = Address::generate(&env);
    let treasury = Address::generate(&env);

    template_client.init(
        &admin,
        &String::from_str(&env, "TEMPLATE"),
        &sme,
        &TARGET,
        &800i64,
        &0u64,
        &template_client.funding_token(),
        &None,
        &treasury,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );

    let investor = Address::generate(&env);
    template_client.fund(&investor, &TARGET);
    template_client.settle();

    // Create first clone
    let clone1_id = env.register(LiquifactEscrow, ());
    let clone1_client = super::LiquifactEscrowClient::new(&env, &clone1_id);

    clone1_client.clone_settled_escrow(
        &env,
        &String::from_str(&env, "CLONE_1"),
        &500_000i128,
    );

    // Create second clone (template still unchanged)
    let clone2_id = env.register(LiquifactEscrow, ());
    let clone2_client = super::LiquifactEscrowClient::new(&env, &clone2_id);

    clone2_client.clone_settled_escrow(
        &env,
        &String::from_str(&env, "CLONE_2"),
        &750_000i128,
    );

    // Verify both clones exist and have correct amounts
    let summary_1 = clone1_client.get_escrow_summary();
    let summary_2 = clone2_client.get_escrow_summary();

    assert_eq!(summary_1.escrow.amount, 500_000i128, "clone 1 amount");
    assert_eq!(summary_2.escrow.amount, 750_000i128, "clone 2 amount");
    assert_eq!(
        summary_1.escrow.admin, summary_2.escrow.admin,
        "both clones should have same admin"
    );

    // Template should still be settled
    let template_summary = template_client.get_escrow_summary();
    assert_eq!(template_summary.escrow.status, 2, "template still settled");
}
