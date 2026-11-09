//! OP075 source-level negative gates complement Rust behavior and real PNN canary.
#[test]
fn dynamic_rotation_is_opt_in_and_only_after_bounded_unhealthy_qualification() {
    let scanner = include_str!("../src/presentation/telegram/workers/utxo_monitor.rs");
    assert!(scanner.contains("KASPA_PNN_DYNAMIC_ROTATION"));
    assert!(scanner.contains("should_search_for_spare("));
    assert!(scanner.contains("unhealthy[*slot]"));
    assert!(scanner.contains("Duration::from_secs(600)"));
    assert!(scanner.contains("discover_public_spare(request)"));
    assert!(scanner.contains("qualified_coeval_spare(sources, &proposal.new_node).await"));
    assert!(scanner.contains(".replace_inactive("));
    assert!(scanner.contains("services[slot] = replacement_service"));
    assert!(scanner.contains("old.client.disconnect()"));
    assert!(scanner.contains("old.current_or_pinned_url()"));
    assert!(scanner.contains("existing.abort()"));
    assert!(scanner.contains("cancel.cancelled() => None"));
}
#[test]
fn both_writer_and_vspc_use_one_generation_with_fence_before_durable_commit() {
    let scanner = include_str!("../src/presentation/telegram/workers/utxo_monitor.rs");
    assert!(scanner.contains("let indexer_sources = pnn_generation.clone()"));
    assert!(scanner.contains("generation.select(choice.active_index).await"));
    let indexer = include_str!("../src/network/vspc_proof.rs");
    assert!(indexer.contains("registry.selected().await"));
    assert!(indexer.contains("registry.fence(captured).await"));
    assert!(indexer.contains("_generation_fence"));
    assert!(indexer.contains(".apply_vspc_proof_delta("));
    assert!(!indexer.contains("pnn_pool_active_index()"));
}
#[test]
fn url_identity_survives_disconnect_and_public_canary_requires_actual_vspc() {
    let adapter = include_str!("../src/infrastructure/node/kaspa_adapter.rs");
    assert!(adapter.contains("pinned_url: Option<String>"));
    assert!(adapter.contains("pub fn current_or_pinned_url("));
    let discovery = include_str!("../src/infrastructure/node/public_pnn_rotation.rs");
    assert!(discovery.contains("get_virtual_chain_from_block(dag.sink, true, Some(1))"));
    assert!(discovery.contains("find_corroborated_reference(&samples)"));
    assert!(discovery.contains("within_verified_daa_window(&reference, candidate)"));
    assert!(discovery.contains("pub fn should_search_for_spare("));
    let registry = include_str!("../src/infrastructure/node/source_registry.rs");
    assert!(registry.contains("Arc::ptr_eq(&state.sources[slot], expected_old)"));
    assert!(registry.contains("replacement of current transactional writer prohibited"));
    assert!(registry.contains("duplicate public PNN URL refused"));
}
#[test]
fn source_zero_auxiliary_primary_monitor_does_not_hold_retired_public_socket() {
    let boot = include_str!("../src/main.rs");
    assert!(!boot.contains("standby_node_provider = Some(sources[0].clone());"));
    assert!(boot.contains("standby_node_provider.is_some() || five_pnn_sources.is_some()"));
    let monitor = include_str!("../src/infrastructure/external_services/system.rs");
    assert!(monitor.contains("has_reward_alternate"));
    assert!(monitor.contains("pnn_reward_source_ready()"));
}
#[test]
fn historical_outbox_no_replay_and_new_proof_admission_still_strict() {
    let worker = include_str!("../src/presentation/telegram/workers/utxo_monitor.rs");
    assert!(worker.contains("OP064_FIRST_ELIGIBLE_BLOCK_MS: u64 = 1793899832000"));
    assert!(worker.contains("op064_may_enqueue_coinbase(event.is_coinbase, event.block_time_ms)"));
    let db = include_str!("../src/infrastructure/database/pending_rewards_repo.rs");
    assert!(db.contains("AND first_seen_at >= TIMESTAMPTZ '2026-11-05 17:30:32+00'"));
    let metrics = include_str!("../src/infrastructure/metrics.rs");
    assert!(metrics.contains("kaspa_pulse_pnn_dynamic_rotation_enabled"));
    assert!(metrics.contains("pub fn set_pnn_dynamic_rotation_enabled("));
    assert!(worker.contains("set_pnn_dynamic_rotation_enabled(dynamic_rotation)"));
    for name in [
        "kaspa_pulse_pnn_spare_discovery_total",
        "kaspa_pulse_pnn_spare_swap_success_total",
        "kaspa_pulse_pnn_spare_rejected_total",
        "kaspa_pulse_pnn_source_generation",
    ] {
        assert!(metrics.contains(name));
    }
}
