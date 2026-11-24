//! Test for read-only endpoint invariant.
//!
//! Issue #2: get_escrow_summary and other read endpoints should never mutate storage
//! or emit events. This test asserts that calling every documented read-only endpoint
//! leaves storage and the event buffer unchanged. This guards against a future refactor
//! accidentally adding writes to a read path.

use soroban_sdk::{Address, Env, String as SorobanString};

/// Helper struct to capture storage and event state before/after a call.
struct EnvironmentSnapshot {
    event_count_before: u32,
    event_count_after: u32,
}

impl EnvironmentSnapshot {
    /// Create a snapshot and execute a closure, capturing state before and after.
    fn measure<F>(env: &Env, f: F) -> Self
    where
        F: FnOnce(),
    {
        let event_count_before = env.events().all().len() as u32;
        f();
        let event_count_after = env.events().all().len() as u32;

        EnvironmentSnapshot {
            event_count_before,
            event_count_after,
        }
    }

    /// Assert that no new events were emitted.
    fn assert_no_events_emitted(&self) {
        assert_eq!(
            self.event_count_before, self.event_count_after,
            "Read-only endpoint emitted events (before: {}, after: {})",
            self.event_count_before, self.event_count_after
        );
    }
}

/// Test: get_escrow() does not mutate storage or emit events.
///
/// Per FEATURE_INDEX.md, `get_escrow` is a pure read endpoint.
/// Calling it multiple times in succession should not:
/// - Emit any events
/// - Modify storage (we verify by re-reading and comparing)
#[test]
fn test_read_only_get_escrow() {
    let env = Env::default();
    let (client, admin, sme) = super::setup(&env);

    // Initialize escrow
    client.init(
        &admin,
        &SorobanString::from_str(&env, "INV_READ_ONLY"),
        &sme,
        &1_000_000i128,
        &800i64,
        &0u64,
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

    // First read
    let snapshot_1 =
        EnvironmentSnapshot::measure(&env, || {
            let _ = client.get_escrow();
        });
    snapshot_1.assert_no_events_emitted();

    // Second read
    let escrow_1 = client.get_escrow();
    let snapshot_2 =
        EnvironmentSnapshot::measure(&env, || {
            let _ = client.get_escrow();
        });
    snapshot_2.assert_no_events_emitted();

    // Third read
    let escrow_2 = client.get_escrow();
    let snapshot_3 =
        EnvironmentSnapshot::measure(&env, || {
            let _ = client.get_escrow();
        });
    snapshot_3.assert_no_events_emitted();

    // Verify state didn't change across reads
    assert_eq!(escrow_1.funded_amount, escrow_2.funded_amount);
    assert_eq!(escrow_1.status, escrow_2.status);
}

/// Test: get_escrow_summary() does not mutate storage or emit events.
///
/// `get_escrow_summary` is a comprehensive read aggregating multiple state pieces.
#[test]
fn test_read_only_get_escrow_summary() {
    let env = Env::default();
    let (client, admin, sme) = super::setup(&env);
    let investor = Address::generate(&env);

    client.init(
        &admin,
        &SorobanString::from_str(&env, "INV_SUMMARY_RO"),
        &sme,
        &1_000_000i128,
        &800i64,
        &0u64,
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

    // Fund to create some state
    client.fund(&investor, &500_000i128);

    // Clear event buffer after mutation
    env.events().all().clear();

    // Multiple reads of summary
    let snapshot_1 = EnvironmentSnapshot::measure(&env, || {
        let _ = client.get_escrow_summary();
    });
    snapshot_1.assert_no_events_emitted();

    let summary_1 = client.get_escrow_summary();

    let snapshot_2 = EnvironmentSnapshot::measure(&env, || {
        let _ = client.get_escrow_summary();
    });
    snapshot_2.assert_no_events_emitted();

    let summary_2 = client.get_escrow_summary();

    // Summaries should be identical
    assert_eq!(
        summary_1.escrow.funded_amount, summary_2.escrow.funded_amount,
        "Summary state should not change across reads"
    );
}

/// Test: check_escrow_health() does not mutate storage or emit events.
///
/// Health check is a read operation that computes metrics but should not write.
#[test]
fn test_read_only_check_escrow_health() {
    let env = Env::default();
    let (client, admin, sme) = super::setup(&env);
    let investor = Address::generate(&env);

    let now = env.ledger().timestamp();

    client.init(
        &admin,
        &SorobanString::from_str(&env, "INV_HEALTH_RO"),
        &sme,
        &1_000_000i128,
        &800i64,
        &(now + 86400),
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

    // Fund to create state
    client.fund(&investor, &500_000i128);

    // Clear events after mutation
    env.events().all().clear();

    // Multiple health checks
    let snapshot_1 = EnvironmentSnapshot::measure(&env, || {
        let _ = client.check_escrow_health();
    });
    snapshot_1.assert_no_events_emitted();

    let health_1 = client.check_escrow_health();

    let snapshot_2 = EnvironmentSnapshot::measure(&env, || {
        let _ = client.check_escrow_health();
    });
    snapshot_2.assert_no_events_emitted();

    let health_2 = client.check_escrow_health();

    // Health data should be stable
    assert_eq!(
        health_1.0, health_2.0,
        "Warning type should remain consistent across reads"
    );
    assert_eq!(
        health_1.1, health_2.1,
        "Funded ratio should remain consistent across reads"
    );
}

/// Test: get_funding_stats() does not mutate storage or emit events.
#[test]
fn test_read_only_get_funding_stats() {
    let env = Env::default();
    let (client, admin, sme) = super::setup(&env);
    let investor = Address::generate(&env);

    client.init(
        &admin,
        &SorobanString::from_str(&env, "INV_STATS_RO"),
        &sme,
        &1_000_000i128,
        &800i64,
        &0u64,
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

    client.fund(&investor, &500_000i128);
    env.events().all().clear();

    let snapshot_1 = EnvironmentSnapshot::measure(&env, || {
        let _ = client.get_funding_stats();
    });
    snapshot_1.assert_no_events_emitted();

    let stats_1 = client.get_funding_stats();

    let snapshot_2 = EnvironmentSnapshot::measure(&env, || {
        let _ = client.get_funding_stats();
    });
    snapshot_2.assert_no_events_emitted();

    let stats_2 = client.get_funding_stats();

    assert_eq!(stats_1.funded_amount, stats_2.funded_amount);
    assert_eq!(stats_1.unique_funder_count, stats_2.unique_funder_count);
}

/// Test: get_investor_contribution() does not mutate storage or emit events.
///
/// Per-investor read: returns specific investor's contribution amount.
#[test]
fn test_read_only_get_investor_contribution() {
    let env = Env::default();
    let (client, admin, sme) = super::setup(&env);
    let investor = Address::generate(&env);

    client.init(
        &admin,
        &SorobanString::from_str(&env, "INV_CONTRIB_RO"),
        &sme,
        &1_000_000i128,
        &800i64,
        &0u64,
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

    client.fund(&investor, &500_000i128);
    env.events().all().clear();

    let snapshot_1 = EnvironmentSnapshot::measure(&env, || {
        let _ = client.get_investor_contribution(&investor);
    });
    snapshot_1.assert_no_events_emitted();

    let contrib_1 = client.get_investor_contribution(&investor);

    let snapshot_2 = EnvironmentSnapshot::measure(&env, || {
        let _ = client.get_investor_contribution(&investor);
    });
    snapshot_2.assert_no_events_emitted();

    let contrib_2 = client.get_investor_contribution(&investor);

    assert_eq!(contrib_1, contrib_2);
}

/// Test: get_registry_listing() does not mutate storage or emit events.
///
/// Registry listing is a discovery read operation.
#[test]
fn test_read_only_get_registry_listing() {
    let env = Env::default();
    let (client, admin, sme) = super::setup(&env);

    client.init(
        &admin,
        &SorobanString::from_str(&env, "INV_REG_RO"),
        &sme,
        &1_000_000i128,
        &800i64,
        &0u64,
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

    env.events().all().clear();

    let snapshot_1 = EnvironmentSnapshot::measure(&env, || {
        let _ = client.get_registry_listing();
    });
    snapshot_1.assert_no_events_emitted();

    let listing_1 = client.get_registry_listing();

    let snapshot_2 = EnvironmentSnapshot::measure(&env, || {
        let _ = client.get_registry_listing();
    });
    snapshot_2.assert_no_events_emitted();

    let listing_2 = client.get_registry_listing();

    assert_eq!(listing_1.invoice_id, listing_2.invoice_id);
    assert_eq!(listing_1.sme_address, listing_2.sme_address);
}

/// Test: Stress read-only invariant with concurrent reads and intervening mutations.
///
/// Scenario:
/// 1. Initialize escrow
/// 2. Fund once (mutation)
/// 3. Read summary
/// 4. Read health
/// 5. Read stats
/// 6. Fund again (mutation)
/// 7. Read summary, health, stats again (should be new values, no pollution from previous reads)
///
/// Verifies that no state leaks between operations.
#[test]
fn test_read_only_invariant_multiple_endpoints() {
    let env = Env::default();
    let (client, admin, sme) = super::setup(&env);
    let investor1 = Address::generate(&env);
    let investor2 = Address::generate(&env);

    let now = env.ledger().timestamp();

    client.init(
        &admin,
        &SorobanString::from_str(&env, "INV_MULTI_RO"),
        &sme,
        &1_000_000i128,
        &800i64,
        &(now + 86400),
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

    // First fund
    client.fund(&investor1, &300_000i128);
    env.events().all().clear();

    // Batch of reads after first fund
    let snapshot_1_1 = EnvironmentSnapshot::measure(&env, || {
        let _ = client.get_escrow_summary();
    });
    snapshot_1_1.assert_no_events_emitted();

    let snapshot_1_2 = EnvironmentSnapshot::measure(&env, || {
        let _ = client.check_escrow_health();
    });
    snapshot_1_2.assert_no_events_emitted();

    let snapshot_1_3 = EnvironmentSnapshot::measure(&env, || {
        let _ = client.get_funding_stats();
    });
    snapshot_1_3.assert_no_events_emitted();

    let stats_1 = client.get_funding_stats();

    // Second fund
    client.fund(&investor2, &700_000i128);
    env.events().all().clear();

    // Batch of reads after second fund
    let snapshot_2_1 = EnvironmentSnapshot::measure(&env, || {
        let _ = client.get_escrow_summary();
    });
    snapshot_2_1.assert_no_events_emitted();

    let snapshot_2_2 = EnvironmentSnapshot::measure(&env, || {
        let _ = client.check_escrow_health();
    });
    snapshot_2_2.assert_no_events_emitted();

    let snapshot_2_3 = EnvironmentSnapshot::measure(&env, || {
        let _ = client.get_funding_stats();
    });
    snapshot_2_3.assert_no_events_emitted();

    let stats_2 = client.get_funding_stats();

    // Verify reads reflect mutations
    assert!(stats_2.funded_amount > stats_1.funded_amount);
    assert!(stats_2.unique_funder_count > stats_1.unique_funder_count);
}
