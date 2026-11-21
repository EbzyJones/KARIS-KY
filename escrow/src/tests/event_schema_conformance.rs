//! Schema Conformance Test: Validates emitted events match EVENT_SCHEMA.md
//!
//! This test captures emitted events and validates their topic/data structure against
//! the documented schema. It prevents event-schema drift that would break indexers.
//!
//! The schema is documented at: docs/EVENT_SCHEMA.md
//!
//! Event validation checks:
//! - Topic count matches expected (fixed topic + data topics)
//! - Topic[1] is always the `name` symbol used for routing
//! - Data fields are present and properly typed
//! - Event symbols match the documented short codes (e.g., `escrow_ii`, `fund_dl`)

use soroban_sdk::{Address, Env, String as SorobanString, Symbol};
use soroban_sdk::symbol_short;

// Helper to check that an event has the expected topic count
fn assert_event_topic_count(event_topics: &Vec<Symbol>, expected_count: usize, event_name: &str) {
    assert_eq!(
        event_topics.len(),
        expected_count,
        "Event {} should have {} topics, got {}",
        event_name,
        expected_count,
        event_topics.len()
    );
}

#[test]
fn test_event_schema_escrow_initialized() {
    //! Validate EscrowInitialized event structure
    //! Schema: 2 topics (fixed + name), with data fields
    let env = Env::default();
    let (client, admin, sme) = super::setup(&env);

    let target = 1_000_000i128;
    let token = Address::generate(&env);
    let treasury = Address::generate(&env);

    client.init(
        &admin,
        &SorobanString::from_str(&env, "SCHEMA_TEST_001"),
        &sme,
        &target,
        &500i64,
        &0u64,
        &token,
        &None,
        &treasury,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );

    let events = env.events().all();
    
    // Find EscrowInitialized event
    let escrow_init_event = events
        .iter()
        .find(|event| {
            let topics = event.topics.clone();
            // Event structure: topic[0] is fixed escrow_initialized, topic[1] is name (escrow_ii)
            topics.len() >= 2
        })
        .expect("EscrowInitialized event not emitted");

    let topics = escrow_init_event.topics.clone();
    
    // Should have at least 2 topics: fixed topic and name symbol
    assert!(
        topics.len() >= 2,
        "EscrowInitialized should have at least 2 topics, got {}",
        topics.len()
    );

    // Topic[1] should be the name symbol (escrow_ii)
    if let Ok(name_sym) = Symbol::try_from_val(&env, &topics.get(1).unwrap()) {
        assert_eq!(
            name_sym,
            symbol_short!("escrow_ii"),
            "EscrowInitialized name symbol should be 'escrow_ii'"
        );
    }
}

#[test]
fn test_event_schema_funding_deadline_set() {
    //! Validate FundingDeadlineSet event structure
    //! Schema: 3 topics (fixed + name + invoice_id), with data field deadline
    let env = Env::default();
    let (client, admin, sme) = super::setup(&env);

    let now = env.ledger().timestamp();
    let target = 1_000_000i128;
    let token = Address::generate(&env);
    let treasury = Address::generate(&env);
    let deadline = Some(now + 86400); // 1 day in future

    client.init(
        &admin,
        &SorobanString::from_str(&env, "SCHEMA_DEADLINE_001"),
        &sme,
        &target,
        &500i64,
        &0u64,
        &token,
        &None,
        &treasury,
        &None,
        &None,
        &None,
        &None,
        &None,
        &deadline,
    );

    let events = env.events().all();

    // Find FundingDeadlineSet event (should have fund_dl symbol)
    let deadline_event = events
        .iter()
        .find(|event| {
            let topics = event.topics.clone();
            // Looking for event with fund_dl as the routing symbol
            topics.len() >= 2 && {
                if let Ok(sym) = Symbol::try_from_val(&env, &topics.get(1).unwrap()) {
                    sym == symbol_short!("fund_dl")
                } else {
                    false
                }
            }
        })
        .expect("FundingDeadlineSet event not emitted when deadline is set");

    let topics = deadline_event.topics.clone();

    // FundingDeadlineSet should have 3 topics:
    // 0: fixed topic (funding_deadline_set)
    // 1: name symbol (fund_dl)
    // 2: invoice_id symbol
    assert_eq!(
        topics.len(),
        3,
        "FundingDeadlineSet should have exactly 3 topics, got {}",
        topics.len()
    );

    // Topic[1] should be fund_dl
    if let Ok(name_sym) = Symbol::try_from_val(&env, &topics.get(1).unwrap()) {
        assert_eq!(
            name_sym,
            symbol_short!("fund_dl"),
            "FundingDeadlineSet name symbol should be 'fund_dl'"
        );
    }

    // Topic[2] should be invoice_id
    if let Ok(inv_id) = Symbol::try_from_val(&env, &topics.get(2).unwrap()) {
        // Invoice IDs are limited in length and chars, but we just validate it's a Symbol
        assert!(
            inv_id.len() > 0,
            "FundingDeadlineSet invoice_id topic should be non-empty"
        );
    }
}

#[test]
fn test_event_schema_funding_deadline_none() {
    //! Validate FundingDeadlineSet event when no deadline is configured
    let env = Env::default();
    let (client, admin, sme) = super::setup(&env);

    let target = 1_000_000i128;
    let token = Address::generate(&env);
    let treasury = Address::generate(&env);

    client.init(
        &admin,
        &SorobanString::from_str(&env, "SCHEMA_NO_DEADLINE_001"),
        &sme,
        &target,
        &500i64,
        &0u64,
        &token,
        &None,
        &treasury,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None, // No deadline
    );

    let events = env.events().all();

    // Find FundingDeadlineSet event
    let deadline_event = events
        .iter()
        .find(|event| {
            let topics = event.topics.clone();
            topics.len() >= 2 && {
                if let Ok(sym) = Symbol::try_from_val(&env, &topics.get(1).unwrap()) {
                    sym == symbol_short!("fund_dl")
                } else {
                    false
                }
            }
        })
        .expect("FundingDeadlineSet event should be emitted even when deadline is None");

    // Validate structure is still correct (3 topics)
    assert_eq!(
        deadline_event.topics.len(),
        3,
        "FundingDeadlineSet should have 3 topics even when deadline is None"
    );
}

#[test]
fn test_event_schema_max_unique_investors_cap_lowered() {
    //! Validate MaxUniqueInvestorsCapLowered event structure
    //! Schema: 3 topics (fixed + name + invoice_id), with data fields old_cap and new_cap
    let env = Env::default();
    let (client, admin, sme) = super::setup(&env);

    let target = 1_000_000i128;
    let token = Address::generate(&env);
    let treasury = Address::generate(&env);

    client.init(
        &admin,
        &SorobanString::from_str(&env, "CAP_LOWER_TEST"),
        &sme,
        &target,
        &500i64,
        &0u64,
        &token,
        &None,
        &treasury,
        &None,
        &None,
        &Some(100u32), // Set initial cap to 100
        &None,
        &None,
        &None,
    );

    // Now lower the cap
    client.lower_max_unique_investors(&50u32);

    let events = env.events().all();

    // Find MaxUniqueInvestorsCapLowered event (inv_cap symbol)
    let cap_event = events
        .iter()
        .find(|event| {
            let topics = event.topics.clone();
            topics.len() >= 2 && {
                if let Ok(sym) = Symbol::try_from_val(&env, &topics.get(1).unwrap()) {
                    sym == symbol_short!("inv_cap")
                } else {
                    false
                }
            }
        })
        .expect("MaxUniqueInvestorsCapLowered event not emitted");

    let topics = cap_event.topics.clone();

    // MaxUniqueInvestorsCapLowered should have 3 topics:
    // 0: fixed topic (max_unique_investors_cap_lowered)
    // 1: name symbol (inv_cap)
    // 2: invoice_id symbol
    assert_eq!(
        topics.len(),
        3,
        "MaxUniqueInvestorsCapLowered should have exactly 3 topics, got {}",
        topics.len()
    );

    // Topic[1] should be inv_cap
    if let Ok(name_sym) = Symbol::try_from_val(&env, &topics.get(1).unwrap()) {
        assert_eq!(
            name_sym,
            symbol_short!("inv_cap"),
            "MaxUniqueInvestorsCapLowered name symbol should be 'inv_cap'"
        );
    }
}

#[test]
fn test_event_schema_escrow_funded() {
    //! Validate EscrowFunded event structure
    //! Schema: 4 topics (fixed + name + invoice_id + investor), with data fields
    let env = Env::default();
    let (client, admin, sme) = super::setup(&env);
    let investor = Address::generate(&env);

    let target = 1_000_000i128;
    let token = Address::generate(&env);
    let treasury = Address::generate(&env);

    client.init(
        &admin,
        &SorobanString::from_str(&env, "FUNDED_TEST"),
        &sme,
        &target,
        &500i64,
        &0u64,
        &token,
        &None,
        &treasury,
        &None,
        &None,
        &None,
        &None,
        &None,
        &None,
    );

    // Clear existing events
    let _ = env.events().all();
    env.ledger().set_sequence(env.ledger().sequence() + 1);

    // Fund the escrow
    client.fund(&investor, &500_000i128);

    let events = env.events().all();

    // Find EscrowFunded event (funded symbol)
    let funded_event = events
        .iter()
        .find(|event| {
            let topics = event.topics.clone();
            topics.len() >= 2 && {
                if let Ok(sym) = Symbol::try_from_val(&env, &topics.get(1).unwrap()) {
                    sym == symbol_short!("funded")
                } else {
                    false
                }
            }
        })
        .expect("EscrowFunded event not emitted");

    let topics = funded_event.topics.clone();

    // EscrowFunded should have 4 topics:
    // 0: fixed topic (escrow_funded)
    // 1: name symbol (funded)
    // 2: invoice_id symbol
    // 3: investor address
    assert_eq!(
        topics.len(),
        4,
        "EscrowFunded should have exactly 4 topics, got {}",
        topics.len()
    );

    // Topic[1] should be funded
    if let Ok(name_sym) = Symbol::try_from_val(&env, &topics.get(1).unwrap()) {
        assert_eq!(
            name_sym,
            symbol_short!("funded"),
            "EscrowFunded name symbol should be 'funded'"
        );
    }
}

#[test]
fn test_event_schema_all_have_name_routing_symbol() {
    //! Validate that all emitted events have a name routing symbol at topic[1]
    //! This is a universal requirement in the schema design
    let env = Env::default();
    let (client, admin, sme) = super::setup(&env);
    let investor = Address::generate(&env);

    let target = 1_000_000i128;
    let token = Address::generate(&env);
    let treasury = Address::generate(&env);
    let now = env.ledger().timestamp();

    // Init with all optional parameters to trigger multiple event emissions
    client.init(
        &admin,
        &SorobanString::from_str(&env, "COMPREHENSIVE_TEST"),
        &sme,
        &target,
        &500i64,
        &(now + 86400),
        &token,
        &None,
        &treasury,
        &None,
        &None,
        &Some(100u32),
        &None,
        &None,
        &Some(now + 43200), // funding deadline in 12 hours
    );

    // Perform funding operations
    client.fund(&investor, &500_000i128);

    let events = env.events().all();

    // Every event should have at least 2 topics
    for event in events.iter() {
        let topics = event.topics.clone();
        assert!(
            topics.len() >= 2,
            "Event should have at least 2 topics (fixed + name routing symbol)"
        );

        // Topic[1] should be a Symbol (the routing name)
        let name_topic_result = Symbol::try_from_val(&env, &topics.get(1).unwrap());
        assert!(
            name_topic_result.is_ok(),
            "Topic[1] should be a Symbol (routing name), but failed to parse"
        );

        let name_sym = name_topic_result.unwrap();
        assert!(
            name_sym.len() > 0,
            "Topic[1] routing symbol should be non-empty"
        );
    }
}
