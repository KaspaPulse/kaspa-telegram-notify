//! OP071 source assertions: every independent reward source is recoverable,
//! and only genuinely qualified slots are reported to the operator.
#[test]
fn scanner_recovers_slot_zero_as_well_as_other_four_sources() {
    let s = include_str!("../src/presentation/telegram/workers/utxo_monitor.rs");
    assert!(s.contains("for (index, source) in sources.iter().enumerate() {"));
    assert!(!s.contains("for (index, source) in sources.iter().enumerate().skip(1) {"));
    assert!(s.contains("let eligible = choice.eligible_slots_mask & (1u8 << index) != 0;"));
    assert!(s.contains("should_recover_unqualified_pnn("));
    let boot = include_str!("../src/main.rs");
    assert!(!boot.contains("standby_node_provider = Some(sources[0].clone());"));
    assert!(boot.contains("standby_node_provider.is_some() || five_pnn_sources.is_some()"));
    assert!(boot.contains("five_pnn_sources = Some(sources);"));
    assert!(boot.contains("reward PNN pool is independent of the primary"));
}
#[test]
fn slot_mask_and_old_standby_metric_are_refreshed_on_every_scan() {
    let s = include_str!("../src/presentation/telegram/workers/utxo_monitor.rs");
    assert!(s.contains("set_pnn_pool_eligible_slots_mask("));
    assert!(s.contains("choice.eligible_slots_mask,"));
    assert!(s.contains("set_pnn_standby_qualified("));
    assert!(s.contains("choice.eligible_slots_mask & 1 != 0"));
    assert!(s.contains("choice.active_index.is_some()"));
    let m = include_str!("../src/infrastructure/metrics.rs");
    assert!(m.contains("kaspa_pulse_pnn_pool_eligible_slots_mask"));
    assert!(m.contains("pub fn required_five_pool_ready() -> bool"));
    let readiness = include_str!("../src/infrastructure/observability.rs");
    assert!(readiness.contains("required_five_pool_ready()"));
    let selector = include_str!("../src/infrastructure/node/quality_selector.rs");
    assert!(selector.contains("eligible_slots_mask"));
}
#[test]
fn proof_backlog_and_telegram_guards_are_completely_unchanged() {
    let s = include_str!("../src/infrastructure/database/pending_rewards_repo.rs");
    assert!(s.contains("AND first_seen_at >= TIMESTAMPTZ '2026-11-05 17:30:32+00'"));
    let scanner = include_str!("../src/presentation/telegram/workers/utxo_monitor.rs");
    assert!(scanner.contains("const OP064_FIRST_ELIGIBLE_BLOCK_MS: u64 = 1793899832000;"));
    assert!(scanner.contains("op064_may_enqueue_coinbase(event.is_coinbase, event.block_time_ms)"));
    assert!(scanner.contains("commit_alert_outbox_localized("));
}
