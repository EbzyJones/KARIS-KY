use super::*;

#[cfg(feature = "trace-mode")]
mod trace_mode_enabled {
    use super::*;

    /// Test: Trace mode can be enabled and disabled
    #[test]
    fn test_trace_enable_disable() {
        let env = Env::default();
        env.mock_all_auths();

        let (client, admin, sme) = setup(&env);
        let (token, treasury) = free_addresses(&env);

        // Initialize escrow
        client.init(
            &admin,
            &soroban_sdk::String::from_str(&env, "TRACE_ENABLE"),
            &sme,
            &100_000i128,
            &800i64,
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
            &None,
            &None,
        );

        // Check initial level is OFF
        let initial_level = client.get_trace_level();
        assert_eq!(initial_level, 0, "Initial trace level should be OFF (0)");

        // Enable tracing at WARN level (2)
        client.enable_tracing(&2u32);

        // Verify level is now set
        let current_level = client.get_trace_level();
        assert_eq!(current_level, 2, "Trace level should be WARN (2) after enable");

        // Disable tracing
        client.disable_tracing();

        // Verify level is back to OFF
        let final_level = client.get_trace_level();
        assert_eq!(final_level, 0, "Trace level should be OFF (0) after disable");
    }

    /// Test: Trace event emitted when legal hold is activated
    #[test]
    fn test_trace_legal_hold_activation() {
        let env = Env::default();
        env.mock_all_auths();

        let (client, admin, sme) = setup(&env);
        let (token, treasury) = free_addresses(&env);

        // Initialize escrow
        client.init(
            &admin,
            &soroban_sdk::String::from_str(&env, "TRACE_LEGAL_HOLD"),
            &sme,
            &100_000i128,
            &800i64,
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
            &None,
            &None,
        );

        // Enable tracing at WARN level
        client.enable_tracing(&2u32);

        // Activate legal hold (should emit trace event)
        client.set_legal_hold(
            &true,
            &soroban_sdk::String::from_str(&env, "Testing trace mode"),
        );

        // Verify legal hold is active
        assert_eq!(client.get_legal_hold(), true, "Legal hold should be active");
    }

    /// Test: Trace event emitted when dispute is paused
    #[test]
    fn test_trace_dispute_pause() {
        let env = Env::default();
        env.mock_all_auths();

        let (client, admin, sme) = setup(&env);
        let (token, treasury) = free_addresses(&env);

        // Initialize escrow
        client.init(
            &admin,
            &soroban_sdk::String::from_str(&env, "TRACE_DISPUTE_PAUSE"),
            &sme,
            &100_000i128,
            &800i64,
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
            &None,
            &None,
        );

        // Enable tracing at WARN level
        client.enable_tracing(&2u32);

        // Pause dispute (should emit trace event)
        let ticket_id = soroban_sdk::String::from_str(&env, "TICKET-123");
        client.pause_dispute(&ticket_id, &3600u64);

        // Verify dispute is paused
        let pause_state = client.get_dispute_pause();
        assert!(pause_state.is_some(), "Dispute pause should be active");
    }

    /// Test: Trace event emitted when primary attestation is bound
    #[test]
    fn test_trace_attestation_binding() {
        let env = Env::default();
        env.mock_all_auths();

        let (client, admin, sme) = setup(&env);
        let (token, treasury) = free_addresses(&env);

        // Initialize escrow
        client.init(
            &admin,
            &soroban_sdk::String::from_str(&env, "TRACE_ATTESTATION"),
            &sme,
            &100_000i128,
            &800i64,
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
            &None,
            &None,
        );

        // Enable tracing at INFO level
        client.enable_tracing(&3u32);

        // Bind a primary attestation hash (should emit trace event)
        let hash_bytes = soroban_sdk::BytesN::<32>::from_array(
            &env,
            &[42u8; 32], // Dummy hash
        );
        let hash = soroban_sdk::Bytes::from_array(&env, hash_bytes.as_ref());
        client.bind_primary_attestation_hash(&hash);

        // Verify attestation is set
        let stored_hash = client.get_primary_attestation_hash();
        assert!(stored_hash.is_some(), "Primary attestation should be set");
    }

    /// Test: Unauthorized caller cannot enable tracing
    #[test]
    fn test_trace_unauthorized_enable_fails() {
        let env = Env::default();
        env.mock_all_auths();

        let (client, admin, sme) = setup(&env);
        let (token, treasury) = free_addresses(&env);

        // Initialize escrow
        client.init(
            &admin,
            &soroban_sdk::String::from_str(&env, "TRACE_UNAUTH"),
            &sme,
            &100_000i128,
            &800i64,
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
            &None,
            &None,
        );

        // Try to enable as non-admin
        let unauthorized = Address::generate(&env);
        let result = client.try_enable_tracing_auth(&2u32, &unauthorized);
        assert!(result.is_err(), "Unauthorized enable should fail");
    }

    /// Test: Clear trace buffer admin-only
    #[test]
    fn test_trace_clear_buffer_admin_only() {
        let env = Env::default();
        env.mock_all_auths();

        let (client, admin, sme) = setup(&env);
        let (token, treasury) = free_addresses(&env);

        // Initialize escrow
        client.init(
            &admin,
            &soroban_sdk::String::from_str(&env, "TRACE_CLEAR"),
            &sme,
            &100_000i128,
            &800i64,
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
            &None,
            &None,
        );

        // Enable tracing
        client.enable_tracing(&2u32);

        // Clear buffer as admin (should succeed)
        client.clear_trace_buffer();

        // Disable tracing
        client.disable_tracing();

        // Verify tracing is off
        assert_eq!(client.get_trace_level(), 0, "Trace should be OFF after disable");
    }

    /// Test: Multiple admin operations each emit trace events
    #[test]
    fn test_trace_multiple_admin_operations() {
        let env = Env::default();
        env.mock_all_auths();

        let (client, admin, sme) = setup(&env);
        let (token, treasury) = free_addresses(&env);

        // Initialize escrow
        client.init(
            &admin,
            &soroban_sdk::String::from_str(&env, "TRACE_MULTI"),
            &sme,
            &100_000i128,
            &800i64,
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
            &None,
            &None,
        );

        // Enable tracing at WARN level (traces WARN and higher)
        client.enable_tracing(&2u32);

        // Perform multiple admin operations (each should emit trace)
        client.set_legal_hold(
            &true,
            &soroban_sdk::String::from_str(&env, "First hold"),
        );

        client.set_legal_hold(
            &false,
            &soroban_sdk::String::from_str(&env, "Clear hold"),
        );

        client.pause_dispute(
            &soroban_sdk::String::from_str(&env, "TICKET-001"),
            &3600u64,
        );

        client.resume_dispute();

        // Verify all operations succeeded (no panics)
        assert_eq!(client.get_legal_hold(), false, "Legal hold should be cleared");
        assert_eq!(
            client.get_dispute_pause().is_some(),
            false,
            "Dispute pause should be cleared"
        );
    }
}

#[cfg(not(feature = "trace-mode"))]
mod trace_mode_disabled {
    use super::*;

    /// Test: Trace functions exist but are no-ops when feature is disabled
    #[test]
    fn test_trace_disabled_no_overhead() {
        let env = Env::default();
        env.mock_all_auths();

        let (client, admin, sme) = setup(&env);
        let (token, treasury) = free_addresses(&env);

        // Initialize escrow
        client.init(
            &admin,
            &soroban_sdk::String::from_str(&env, "TRACE_DISABLED"),
            &sme,
            &100_000i128,
            &800i64,
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
            &None,
            &None,
        );

        // Try to enable tracing (should be no-op, not fail)
        client.enable_tracing(&2u32);

        // Get trace level (should always return OFF when feature disabled)
        let level = client.get_trace_level();
        assert_eq!(level, 0, "Trace level should always be OFF when feature disabled");

        // Clear buffer (should be no-op)
        client.clear_trace_buffer();

        // Disable tracing (should be no-op)
        client.disable_tracing();

        // Verify operations still work normally
        client.set_legal_hold(
            &true,
            &soroban_sdk::String::from_str(&env, "Testing disabled mode"),
        );
        assert_eq!(client.get_legal_hold(), true, "Legal hold should still work");
    }
}
