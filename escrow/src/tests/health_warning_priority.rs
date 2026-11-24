//! Test for health warning priority ordering per FEATURE_INDEX.md.
//!
//! Issue #1: compute_and_emit_health_warning emits a single warning with priority
//! (e.g., 4001 LowFunding wins over 4002 CloseToMaturity). This test constructs an
//! escrow that is both low-funded and close to maturity, asserting the higher-priority
//! code is emitted. This pins the priority ordering against future changes.

use soroban_sdk::{Address, Env, String as SorobanString};

/// Test health warning priority: LowFunding (4001) beats CloseToMaturity (4002).
///
/// Constructs an escrow with:
/// - Low funding ratio (< 50%)
/// - Maturity approaching (< 1 day away)
///
/// Asserts that the emitted warning is 4001 (LowFunding), proving that
/// low funding takes priority over maturity warnings in the code.
#[test]
fn test_health_warning_priority_low_funding_beats_close_to_maturity() {
    let env = Env::default();
    let (client, admin, sme) = super::setup(&env);
    let investor = Address::generate(&env);

    let now = env.ledger().timestamp();
    let target = 1_000_000i128;

    // Initialize with maturity 1 hour in the future
    client.init(
        &admin,
        &SorobanString::from_str(&env, "INV_PRIORITY_TEST"),
        &sme,
        &target,
        &800i64,
        &(now + 3600), // Maturity 1 hour away
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );

    // Fund only 40% of target => conditions for both 4001 and 4002
    let funded_amount = 400_000i128;
    client.fund(&investor, &funded_amount);

    // Advance to 30 minutes before maturity (within 1-day window for 4002)
    // => Both conditions met: low funding (4001) AND close to maturity (4002)
    env.ledger().set_timestamp(now + 1800); // 30 minutes elapsed

    let (warning_type, funded_ratio_bps, time_to_maturity_secs) =
        client.check_escrow_health();

    // Per FEATURE_INDEX.md, LowFunding (4001) has priority over CloseToMaturity (4002)
    assert_eq!(
        warning_type, 4001,
        "When both LowFunding and CloseToMaturity conditions met, \
         LowFunding (4001) must win per priority ordering"
    );

    assert_eq!(
        funded_ratio_bps, 4000,
        "Funded ratio should remain 40% (4000 bps)"
    );

    assert!(
        time_to_maturity_secs > 0 && time_to_maturity_secs < 86400,
        "Time to maturity should be within 1-day window (0, 86400)"
    );
}

/// Test that LowFunding alone emits 4001 without maturity involvement.
///
/// Verifies the base case: low funding warning can occur independently
/// of maturity conditions (no maturity set).
#[test]
fn test_health_warning_priority_low_funding_no_maturity() {
    let env = Env::default();
    let (client, admin, sme) = super::setup(&env);
    let investor = Address::generate(&env);

    let target = 1_000_000i128;

    // Initialize with NO maturity (0)
    client.init(
        &admin,
        &SorobanString::from_str(&env, "INV_LOW_NO_MAT"),
        &sme,
        &target,
        &800i64,
        &0u64, // No maturity constraint
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );

    // Fund only 40%
    client.fund(&investor, &(400_000i128));

    let (warning_type, _, _) = client.check_escrow_health();

    assert_eq!(
        warning_type, 4001,
        "Low funding alone (without maturity) should emit 4001"
    );
}

/// Test that CloseToMaturity alone (without low funding) emits 4002.
///
/// Verifies that 4002 can occur when escrow is well-funded but time-constrained.
#[test]
fn test_health_warning_priority_close_to_maturity_well_funded() {
    let env = Env::default();
    let (client, admin, sme) = super::setup(&env);
    let investor = Address::generate(&env);

    let now = env.ledger().timestamp();
    let target = 1_000_000i128;

    // Initialize with maturity 1 hour away
    client.init(
        &admin,
        &SorobanString::from_str(&env, "INV_CLOSE_FUNDED"),
        &sme,
        &target,
        &800i64,
        &(now + 3600),
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );

    // Fund 100% to ensure well-funded
    client.fund(&investor, &target);

    // Advance to 30 minutes before maturity
    env.ledger().set_timestamp(now + 1800);

    let (warning_type, funded_ratio_bps, _) = client.check_escrow_health();

    assert_eq!(
        warning_type, 4002,
        "When escrow is well-funded (>= 50%) but close to maturity (< 1 day), \
         should emit CloseToMaturity (4002)"
    );

    assert!(
        funded_ratio_bps >= 5000,
        "Funded ratio should be >= 50% (5000 bps)"
    );
}

/// Test priority ordering when both low funding and close-to-maturity exist but transition.
///
/// Scenario: Escrow is initially low-funded and close to maturity (4001).
/// Then more funding arrives, making it well-funded but still close to maturity.
/// Warning should switch from 4001 to 4002 to reflect new state.
#[test]
fn test_health_warning_priority_transition_from_low_to_close() {
    let env = Env::default();
    let (client, admin, sme) = super::setup(&env);
    let investor1 = Address::generate(&env);
    let investor2 = Address::generate(&env);

    let now = env.ledger().timestamp();
    let target = 1_000_000i128;

    client.init(
        &admin,
        &SorobanString::from_str(&env, "INV_TRANSITION"),
        &sme,
        &target,
        &800i64,
        &(now + 3600), // 1 hour to maturity
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );

    // First fund: 40% (low funding + close to maturity => 4001)
    client.fund(&investor1, &(400_000i128));
    env.ledger().set_timestamp(now + 1800); // 30 min to maturity

    let (warning_type_1, _, _) = client.check_escrow_health();
    assert_eq!(warning_type_1, 4001, "Initial state should emit 4001 (low funding)");

    // Second fund: additional 60% (now 100% funded, still close to maturity => 4002)
    client.fund(&investor2, &(600_000i128));

    let (warning_type_2, funded_ratio_bps_2, _) = client.check_escrow_health();

    assert_eq!(
        warning_type_2, 4002,
        "After sufficient funding, warning should transition to 4002 (close to maturity)"
    );

    assert!(
        funded_ratio_bps_2 >= 5000,
        "After additional funding, ratio should be >= 50%"
    );
}

/// Test that OverMaturity (4003) has higher priority than LowFunding (4001).
///
/// Scenario: Escrow is unfunded and past maturity.
/// Expected: 4003 (OverMaturity) emitted, not 4001 (LowFunding).
#[test]
fn test_health_warning_priority_over_maturity_beats_low_funding() {
    let env = Env::default();
    let (client, admin, sme) = super::setup(&env);

    let now = env.ledger().timestamp();
    let target = 1_000_000i128;

    // Initialize with maturity 1 hour away
    client.init(
        &admin,
        &SorobanString::from_str(&env, "INV_OVER_BEATS_LOW"),
        &sme,
        &target,
        &800i64,
        &(now + 3600),
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );

    // Do NOT fund at all (unfunded + past maturity => 4003)
    // Advance past maturity
    env.ledger().set_timestamp(now + 3600 + 1800); // 30 min past maturity

    let (warning_type, funded_ratio_bps, time_to_maturity_secs) =
        client.check_escrow_health();

    assert_eq!(
        warning_type, 4003,
        "When past maturity, unfunded, and status=0 (open), \
         should emit OverMaturity (4003), not LowFunding (4001)"
    );

    assert!(
        funded_ratio_bps < 5000,
        "Unfunded ratio confirms condition for 4001"
    );

    assert!(
        time_to_maturity_secs < 0,
        "Time to maturity is negative, confirming past maturity"
    );
}

/// Test that settled escrow emits no warning even if historically low-funded or past maturity.
///
/// Once status transitions to 2 (settled), no warnings should be emitted
/// regardless of funding or time conditions.
#[test]
fn test_health_warning_priority_settled_suppresses_all() {
    let env = Env::default();
    let (client, admin, sme) = super::setup(&env);
    let investor = Address::generate(&env);

    let now = env.ledger().timestamp();
    let target = 1_000_000i128;

    client.init(
        &admin,
        &SorobanString::from_str(&env, "INV_SETTLED_SUPPRESS"),
        &sme,
        &target,
        &800i64,
        &(now + 3600),
        &Address::generate(&env),
        &None,
        &Address::generate(&env),
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );

    // Fund only 30% (would normally trigger 4001)
    client.fund(&investor, &(300_000i128));

    // Settle before maturity (not typically allowed, but if it were settled)
    // After settle, status becomes 2, suppressing warnings
    env.ledger().set_timestamp(now + 3600);
    client.settle();

    let (warning_type, _, _) = client.check_escrow_health();

    assert_eq!(
        warning_type, 0,
        "Settled escrow (status 2) should emit no warning, \
         regardless of prior funding level"
    );
}
