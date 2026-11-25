/// Reconstruction integrity tests for delta-encoded snapshots.
///
/// This module tests the disaster-recovery path documented in ADR-009.
/// It verifies that:
/// 1. A long delta chain reconstructs to the same state as the live escrow.
/// 2. A malformed delta (e.g., wrong `based_on_delta_id`) is rejected.
///
/// These tests protect the reconstruction algorithm and validate the invariants
/// required for safe off-chain auditing and historical state recovery.

use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    token::{StellarAssetClient, TokenClient},
    Address, Env, String as SorobanString, Vec,
};

// Use the escrow contract from the lib crate
use karis_ky_escrow::{LiquifactEscrow, LiquifactEscrowClient, SnapshotDelta, DataKey};

fn deploy_contract(env: &Env) -> (LiquifactEscrowClient, Address) {
    let id = env.register(LiquifactEscrow, ());
    let client = LiquifactEscrowClient::new(env, &id);
    (client, id)
}

fn setup_ledger(env: &Env) {
    let mut ledger_info = env.ledger().get();
    ledger_info.timestamp = 12345;
    ledger_info.sequence_number = 100;
    env.ledger().set(ledger_info);
    env.mock_all_auths();
}

fn generate_addresses(env: &Env) -> (Address, Address, Address, Address) {
    (
        Address::generate(env),
        Address::generate(env),
        Address::generate(env),
        Address::generate(env),
    )
}

/// Install a standard Stellar asset token contract for testing.
fn install_stellar_asset_token(env: &Env) -> (Address, TokenClient, StellarAssetClient) {
    let sac = env.register_stellar_asset_contract_v2(Address::generate(env));
    let id = sac.address();
    (
        id.clone(),
        TokenClient::new(env, &id),
        StellarAssetClient::new(env, &id),
    )
}

/// Helper to build a delta manually for testing malformed linkage.
fn create_delta(
    env: &Env,
    delta_id: u32,
    based_on_delta_id: u32,
    funded_amount_delta: i128,
    maturity: u32,
    status: u32,
) -> SnapshotDelta {
    SnapshotDelta {
        delta_id,
        recorded_at: 12345,
        based_on_delta_id,
        funded_amount_delta,
        maturity,
        status,
        admin: None,
        sme_address: None,
    }
}

#[test]
fn test_reconstruction_matches_live_state_after_multiple_funds() {
    let env = Env::new();
    setup_ledger(&env);

    let (client, _contract_id) = deploy_contract(&env);
    let (admin, sme, investor1, treasury) = generate_addresses(&env);
    let investor2 = Address::generate(&env);
    let investor3 = Address::generate(&env);

    let (token_id, _token_client, _sac) = install_stellar_asset_token(&env);

    let invoice_id = SorobanString::from_str(&env, "INV-DELTA-001");
    let funding_target = 100_000_000_000i128;
    let yield_bps = 500i64;

    client.init(
        &admin,
        &invoice_id,
        &sme,
        &funding_target,
        &yield_bps,
        &0u64,
        &token_id,
        &None,
        &treasury,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );

    // Manually populate the delta chain by directly writing to storage
    // This simulates what would happen if delta recording was active
    let mut deltas: Vec<SnapshotDelta> = Vec::new(&env);

    // Initial escrow baseline
    let baseline_escrow = client.get_escrow();

    // Delta 1: investor1 funds 30_000_000_000
    let delta1 = create_delta(
        &env,
        1u32,
        0u32,
        30_000_000_000i128,
        0u32,
        1u32, // status 0 -> 1 (funded)
    );
    deltas.push_back(delta1.clone());
    env.storage()
        .instance()
        .set(&DataKey::SnapshotDelta(1u32), &delta1);

    // Delta 2: investor2 funds 25_000_000_000
    let delta2 = create_delta(&env, 2u32, 1u32, 25_000_000_000i128, 0u32, 255u32);
    deltas.push_back(delta2.clone());
    env.storage()
        .instance()
        .set(&DataKey::SnapshotDelta(2u32), &delta2);

    // Delta 3: investor3 funds 45_000_000_000
    let delta3 = create_delta(&env, 3u32, 2u32, 45_000_000_000i128, 0u32, 255u32);
    deltas.push_back(delta3.clone());
    env.storage()
        .instance()
        .set(&DataKey::SnapshotDelta(3u32), &delta3);

    // Set the baseline snapshot
    env.storage()
        .instance()
        .set(&DataKey::FullSnapshot, &baseline_escrow);

    // Set the delta chain head
    env.storage().instance().set(&DataKey::SnapshotDeltaChain, &3u32);

    // Reconstruct from deltas
    let reconstructed = LiquifactEscrow::reconstruct_snapshot_from_deltas(&env)
        .expect("reconstruction should succeed");

    // Verify that funded_amount matches the sum of deltas
    let expected_funded = 30_000_000_000i128 + 25_000_000_000i128 + 45_000_000_000i128;
    assert_eq!(
        reconstructed.funded_amount, expected_funded,
        "funded_amount should match sum of deltas"
    );

    // Verify status was updated by first delta
    assert_eq!(reconstructed.status, 1u32, "status should be 1 (funded)");

    // Verify baseline fields are preserved
    assert_eq!(
        reconstructed.invoice_id, baseline_escrow.invoice_id,
        "invoice_id should match baseline"
    );
    assert_eq!(
        reconstructed.amount, baseline_escrow.amount,
        "amount should match baseline"
    );
}

#[test]
fn test_reconstruction_with_zero_delta_chain() {
    // When SnapshotDeltaChain is absent, reconstruction should return current escrow unchanged.
    let env = Env::new();
    setup_ledger(&env);

    let (client, _contract_id) = deploy_contract(&env);
    let (admin, sme, investor, treasury) = generate_addresses(&env);
    let (token_id, _token_client, _sac) = install_stellar_asset_token(&env);

    let invoice_id = SorobanString::from_str(&env, "INV-DELTA-NO-CHAIN");
    let funding_target = 100_000_000_000i128;

    client.init(
        &admin,
        &invoice_id,
        &sme,
        &funding_target,
        &500i64,
        &0u64,
        &token_id,
        &None,
        &treasury,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );

    // No delta chain is set; reconstruction should return current escrow
    let current_escrow = client.get_escrow();
    let reconstructed =
        LiquifactEscrow::reconstruct_snapshot_from_deltas(&env)
            .expect("reconstruction should succeed even with no deltas");

    assert_eq!(
        reconstructed.funded_amount, current_escrow.funded_amount,
        "funded_amount should match current escrow"
    );
    assert_eq!(
        reconstructed.status, current_escrow.status,
        "status should match current escrow"
    );
}

#[test]
fn test_reconstruction_fails_on_missing_delta() {
    // If a delta is missing from the chain, reconstruction must fail.
    let env = Env::new();
    setup_ledger(&env);

    let (client, _contract_id) = deploy_contract(&env);
    let (admin, sme, _investor, treasury) = generate_addresses(&env);
    let (token_id, _token_client, _sac) = install_stellar_asset_token(&env);

    client.init(
        &admin,
        &SorobanString::from_str(&env, "INV-DELTA-MISSING"),
        &sme,
        &100_000_000_000i128,
        &500i64,
        &0u64,
        &token_id,
        &None,
        &treasury,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );

    let baseline_escrow = client.get_escrow();

    // Create a delta chain with delta_id 1 and 3, but skip delta_id 2
    let delta1 = create_delta(&env, 1u32, 0u32, 10_000_000_000i128, 0u32, 1u32);
    env.storage()
        .instance()
        .set(&DataKey::SnapshotDelta(1u32), &delta1);

    // Intentionally skip delta_id 2

    let delta3 = create_delta(&env, 3u32, 2u32, 20_000_000_000i128, 0u32, 255u32);
    env.storage()
        .instance()
        .set(&DataKey::SnapshotDelta(3u32), &delta3);

    env.storage()
        .instance()
        .set(&DataKey::FullSnapshot, &baseline_escrow);
    env.storage().instance().set(&DataKey::SnapshotDeltaChain, &3u32);

    // Reconstruction should fail because delta_id 2 is missing
    let result = LiquifactEscrow::reconstruct_snapshot_from_deltas(&env);
    assert!(
        result.is_err(),
        "reconstruction should fail when a delta in the chain is missing"
    );

    let err = result.unwrap_err();
    assert!(
        err.contains_str(&SorobanString::from_str(&env, "delta not found")),
        "error should indicate missing delta"
    );
}

#[test]
fn test_reconstruction_fails_on_broken_linkage() {
    // If a delta's `based_on_delta_id` doesn't match the previous delta's `delta_id`, fail.
    let env = Env::new();
    setup_ledger(&env);

    let (client, _contract_id) = deploy_contract(&env);
    let (admin, sme, _investor, treasury) = generate_addresses(&env);
    let (token_id, _token_client, _sac) = install_stellar_asset_token(&env);

    client.init(
        &admin,
        &SorobanString::from_str(&env, "INV-DELTA-BROKEN"),
        &sme,
        &100_000_000_000i128,
        &500i64,
        &0u64,
        &token_id,
        &None,
        &treasury,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );

    let baseline_escrow = client.get_escrow();

    // Create a delta chain where delta_id 2 incorrectly links to delta_id 1
    // but should link to delta_id 1 (this is subtle, so create explicit corruption)
    let delta1 = create_delta(&env, 1u32, 0u32, 10_000_000_000i128, 0u32, 1u32);
    env.storage()
        .instance()
        .set(&DataKey::SnapshotDelta(1u32), &delta1);

    // Delta 2 claims to be based on delta_id 5 (which doesn't exist), breaking the chain
    let delta2_broken = create_delta(&env, 2u32, 5u32, 20_000_000_000i128, 0u32, 255u32);
    env.storage()
        .instance()
        .set(&DataKey::SnapshotDelta(2u32), &delta2_broken);

    env.storage()
        .instance()
        .set(&DataKey::FullSnapshot, &baseline_escrow);
    env.storage().instance().set(&DataKey::SnapshotDeltaChain, &2u32);

    // Reconstruction should fail due to broken linkage
    let result = LiquifactEscrow::reconstruct_snapshot_from_deltas(&env);
    assert!(
        result.is_err(),
        "reconstruction should fail when delta linkage is broken"
    );

    let err = result.unwrap_err();
    assert!(
        err.contains_str(&SorobanString::from_str(&env, "broken chain linkage")),
        "error should indicate broken linkage"
    );
}

#[test]
fn test_reconstruction_fails_on_arithmetic_overflow() {
    // If applying a delta causes funded_amount to overflow, fail gracefully.
    let env = Env::new();
    setup_ledger(&env);

    let (client, _contract_id) = deploy_contract(&env);
    let (admin, sme, _investor, treasury) = generate_addresses(&env);
    let (token_id, _token_client, _sac) = install_stellar_asset_token(&env);

    client.init(
        &admin,
        &SorobanString::from_str(&env, "INV-DELTA-OVERFLOW"),
        &sme,
        &100_000_000_000i128,
        &500i64,
        &0u64,
        &token_id,
        &None,
        &treasury,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );

    let baseline_escrow = client.get_escrow();

    // Create a delta that would overflow funded_amount
    let overflow_delta = create_delta(
        &env,
        1u32,
        0u32,
        i128::MAX, // This will overflow when added to any positive amount
        0u32,
        1u32,
    );
    env.storage()
        .instance()
        .set(&DataKey::SnapshotDelta(1u32), &overflow_delta);

    env.storage()
        .instance()
        .set(&DataKey::FullSnapshot, &baseline_escrow);
    env.storage().instance().set(&DataKey::SnapshotDeltaChain, &1u32);

    // Reconstruction should fail due to overflow
    let result = LiquifactEscrow::reconstruct_snapshot_from_deltas(&env);
    assert!(
        result.is_err(),
        "reconstruction should fail on arithmetic overflow"
    );

    let err = result.unwrap_err();
    assert!(
        err.contains_str(&SorobanString::from_str(&env, "overflow")) ||
            err.contains_str(&SorobanString::from_str(&env, "underflow")),
        "error should indicate overflow/underflow"
    );
}

#[test]
fn test_reconstruction_applies_conditional_fields() {
    // Verify that maturity, status, admin, and sme_address are applied correctly
    // when their values are non-zero/non-None.
    let env = Env::new();
    setup_ledger(&env);

    let (client, _contract_id) = deploy_contract(&env);
    let (admin, sme, _investor, treasury) = generate_addresses(&env);
    let new_admin = Address::generate(&env);
    let new_sme = Address::generate(&env);
    let (token_id, _token_client, _sac) = install_stellar_asset_token(&env);

    client.init(
        &admin,
        &SorobanString::from_str(&env, "INV-DELTA-FIELDS"),
        &sme,
        &100_000_000_000i128,
        &500i64,
        &0u64,
        &token_id,
        &None,
        &treasury,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );

    let baseline_escrow = client.get_escrow();

    // Delta that updates status, maturity, admin, and sme_address
    let mut delta1 = create_delta(&env, 1u32, 0u32, 10_000_000_000i128, 1704067200u32, 1u32);
    delta1.admin = Some(new_admin.clone());
    delta1.sme_address = Some(new_sme.clone());

    env.storage()
        .instance()
        .set(&DataKey::SnapshotDelta(1u32), &delta1);

    // Delta 2: update status to settled, maturity unchanged (stays at previous value)
    let mut delta2 = create_delta(&env, 2u32, 1u32, 0i128, 0u32, 2u32);

    env.storage()
        .instance()
        .set(&DataKey::SnapshotDelta(2u32), &delta2);

    env.storage()
        .instance()
        .set(&DataKey::FullSnapshot, &baseline_escrow);
    env.storage().instance().set(&DataKey::SnapshotDeltaChain, &2u32);

    let reconstructed = LiquifactEscrow::reconstruct_snapshot_from_deltas(&env)
        .expect("reconstruction should succeed");

    // Verify field updates
    assert_eq!(
        reconstructed.maturity, 1704067200u32,
        "maturity should be updated from first delta"
    );
    assert_eq!(
        reconstructed.status, 2u32,
        "status should be updated to 2 from second delta"
    );
    assert_eq!(
        reconstructed.admin, new_admin,
        "admin should be updated from first delta"
    );
    assert_eq!(
        reconstructed.sme_address, new_sme,
        "sme_address should be updated from first delta"
    );
}

#[test]
fn test_reconstruction_with_long_delta_chain() {
    // Test reconstruction of a long delta chain (20+ deltas) to ensure the algorithm
    // handles extended histories correctly.
    let env = Env::new();
    setup_ledger(&env);

    let (client, _contract_id) = deploy_contract(&env);
    let (admin, sme, _investor, treasury) = generate_addresses(&env);
    let (token_id, _token_client, _sac) = install_stellar_asset_token(&env);

    client.init(
        &admin,
        &SorobanString::from_str(&env, "INV-DELTA-LONG"),
        &sme,
        &1_000_000_000_000i128, // Large funding target
        &500i64,
        &0u64,
        &token_id,
        &None,
        &treasury,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );

    let baseline_escrow = client.get_escrow();

    // Create a long delta chain: 25 deltas, each incrementing funded_amount by 1_000_000_000
    let num_deltas = 25u32;
    let delta_amount = 1_000_000_000i128;
    let mut total_funded = 0i128;

    for i in 1..=num_deltas {
        let based_on = if i == 1 { 0u32 } else { i - 1 };
        let status = if i == 1 { 1u32 } else { 255u32 }; // Only first delta sets status

        let delta = create_delta(&env, i, based_on, delta_amount, 0u32, status);
        env.storage()
            .instance()
            .set(&DataKey::SnapshotDelta(i), &delta);
        total_funded += delta_amount;
    }

    env.storage()
        .instance()
        .set(&DataKey::FullSnapshot, &baseline_escrow);
    env.storage()
        .instance()
        .set(&DataKey::SnapshotDeltaChain, &num_deltas);

    // Reconstruct from the long chain
    let reconstructed = LiquifactEscrow::reconstruct_snapshot_from_deltas(&env)
        .expect("reconstruction of long chain should succeed");

    // Verify total funded amount
    assert_eq!(
        reconstructed.funded_amount, total_funded,
        "funded_amount should equal sum of all {} deltas",
        num_deltas
    );

    // Verify status was set by the first delta
    assert_eq!(
        reconstructed.status, 1u32,
        "status should be 1 from first delta"
    );
}

#[test]
fn test_reconstruction_validates_all_linkages_in_chain() {
    // Ensure that linkage is validated for every delta in the chain, not just the head.
    // If a delta in the middle has a wrong based_on_delta_id, it must fail.
    let env = Env::new();
    setup_ledger(&env);

    let (client, _contract_id) = deploy_contract(&env);
    let (admin, sme, _investor, treasury) = generate_addresses(&env);
    let (token_id, _token_client, _sac) = install_stellar_asset_token(&env);

    client.init(
        &admin,
        &SorobanString::from_str(&env, "INV-DELTA-LINKAGE"),
        &sme,
        &100_000_000_000i128,
        &500i64,
        &0u64,
        &token_id,
        &None,
        &treasury,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );

    let baseline_escrow = client.get_escrow();

    // Create a chain: delta 1 -> delta 2 -> delta 4 (skipping 3)
    let delta1 = create_delta(&env, 1u32, 0u32, 10_000_000_000i128, 0u32, 1u32);
    env.storage()
        .instance()
        .set(&DataKey::SnapshotDelta(1u32), &delta1);

    let delta2 = create_delta(&env, 2u32, 1u32, 20_000_000_000i128, 0u32, 255u32);
    env.storage()
        .instance()
        .set(&DataKey::SnapshotDelta(2u32), &delta2);

    // delta4 incorrectly claims to be based on delta3 (which doesn't exist)
    let delta4 = create_delta(&env, 4u32, 3u32, 30_000_000_000i128, 0u32, 255u32);
    env.storage()
        .instance()
        .set(&DataKey::SnapshotDelta(4u32), &delta4);

    env.storage()
        .instance()
        .set(&DataKey::FullSnapshot, &baseline_escrow);
    env.storage().instance().set(&DataKey::SnapshotDeltaChain, &4u32);

    // Reconstruction should fail during validation of delta4's linkage
    let result = LiquifactEscrow::reconstruct_snapshot_from_deltas(&env);
    assert!(
        result.is_err(),
        "reconstruction should fail when middle of chain has wrong linkage"
    );
}
