//! OP070 safety contracts: only NEW (post-cutover) strict Coinbase proofs
//! enter the fair public archive route, and one wallet reader writes outbox.
#[test]
fn archived_proof_only_for_recovered_newer_coinbase_and_fresh_unchanged() {
    let s = include_str!("../src/wallet/wallet_use_cases.rs");
    assert!(s.contains("recovered_proof_backlog && utxo.is_coinbase"));
    assert!(s.contains("configured.max(std::time::Duration::from_secs(100))"));
    assert!(s.contains("let route_deadline = proof_route_timeout("));
    let sql = include_str!("../src/infrastructure/database/pending_rewards_repo.rs");
    assert!(sql.contains("AND first_seen_at >= TIMESTAMPTZ '2026-11-05 17:30:32+00'"));
    let scanner = include_str!("../src/presentation/telegram/workers/utxo_monitor.rs");
    assert!(scanner.contains("OP064_FIRST_ELIGIBLE_BLOCK_MS: u64 = 1793899832000"));
    assert!(scanner.contains("op064_may_enqueue_coinbase(event.is_coinbase, event.block_time_ms)"));
}
#[test]
fn single_bounded_fifo_public_archive_proof_and_visible_newer_slo() {
    let s = include_str!("../src/network/public_api_historical.rs");
    assert!(s.contains("Semaphore::new(1)"));
    assert!(s.contains("proof_gate.acquire()"));
    assert!(s.contains("Duration::from_secs(25)"));
    assert!(s.contains("tokio::time::sleep(wait).await"));
    assert!(s.contains("Duration::from_secs(40)"));
    let metrics = include_str!("../src/infrastructure/metrics.rs");
    assert!(metrics.contains("kaspa_pulse_archive_admitted_total"));
    assert!(metrics.contains("kaspa_pulse_proof_newer_over_10m"));
    let poll = include_str!("../src/presentation/telegram/workers/telegram_delivery.rs");
    assert!(poll.contains("first_seen_at >= TIMESTAMPTZ '2026-11-05 17:30:32+00'"));
}
#[test]
fn primary_notifications_not_conflated_with_healthy_reward_pool() {
    let primary = include_str!("../src/infrastructure/external_services/system.rs");
    // OP090: healthy independent reward pool MUST NOT produce an incident
    // every minute when only the auxiliary primary wRPC is flapping.
    assert!(!primary.contains("primary_pnn_degraded"));
    assert!(!primary.contains("primary_pnn_reconnected"));
    assert!(primary.contains("reward_pool_outage_reported = false"));
    assert!(primary.contains("if !reward_pool_outage_reported"));
    assert!(primary.contains(r#".bind("node_unreachable")"#));
    assert!(primary.contains(".bind(EventSeverity::Error.as_str())"));
    assert!(primary.contains("admin_outage_notice_sent"));
    assert!(primary.contains("pnn_reward_source_ready()"));
    assert!(primary.contains("ctx.rpc.connect(None)"));
    assert!(primary.contains("if !has_reward_alternate"));
    let m = include_str!("../src/infrastructure/metrics.rs");
    assert!(m.contains("last_successful_scan_ts"));
    assert!(m.contains("now - last_scan <= 90"));
}
#[test]
fn five_way_utxo_reader_remains_one_source_per_scan() {
    let s = include_str!("../src/presentation/telegram/workers/utxo_monitor.rs");
    assert!(s.contains("selector.choose(started.elapsed(), &observations)"));
    assert!(s.contains("services.get(index).cloned()"));
    assert!(s.contains("let mut urls = std::collections::HashSet::new()"));
    let vspc = include_str!("../src/network/vspc_proof.rs");
    assert!(vspc.contains("registry.selected().await"));
    assert!(vspc.contains("registry.fence(captured).await"));
    assert!(!vspc.contains("pnn_pool_active_index()"));
}
