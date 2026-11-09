//! OP076: replay stays blocked; adaptive retry never suppresses valid proof.
//! Source-level contracts complement pure policy tests and PostgreSQL
//! read-only canary. No production state is modified by these tests.
#[test]
fn one_identical_retry_threshold_for_bounded_read_and_atomic_claim() {
    let src = include_str!("../src/infrastructure/database/pending_rewards_repo.rs");
    assert!(src.contains("pub(crate) const OP076_RETRY_INTERVAL_SQL"));
    assert_eq!(src.matches("NOW()-{OP076_RETRY_INTERVAL_SQL}").count(), 2);
    assert!(src.contains("let query = format!("));
    assert!(
        src.contains("let claimed = sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(query))")
    );
    assert!(src.contains("ORDER BY last_checked_at,outpoint LIMIT 6"));
    assert!(src.contains("RETURNING outpoint"));
    assert_eq!(
        src.matches("AND first_seen_at >= TIMESTAMPTZ '2026-11-05 17:30:32+00'")
            .count(),
        2
    );
}
#[test]
fn strict_fixed_history_cutoff_retains_old_rows_without_replay() {
    let repo = include_str!("../src/infrastructure/database/pending_rewards_repo.rs");
    let worker = include_str!("../src/presentation/telegram/workers/utxo_monitor.rs");
    assert!(worker.contains("OP064_FIRST_ELIGIBLE_BLOCK_MS: u64 = 1793899832000"));
    assert!(worker.contains("op064_may_enqueue_coinbase(event.is_coinbase, event.block_time_ms)"));
    assert!(!repo.contains("DELETE FROM pending_rewards WHERE status='awaiting_block_proof'"));
    assert!(repo.contains("OR pending_rewards.last_checked_at < NOW()-INTERVAL '45 seconds'"));
}
#[test]
fn only_newer_proof_metadata_changes_trigger_early_retry() {
    let repo = include_str!("../src/infrastructure/database/pending_rewards_repo.rs");
    let delta = repo
        .split("pub async fn apply_vspc_proof_delta(")
        .nth(1)
        .unwrap()
        .split("pub async fn get_pending_accepting_hash(")
        .next()
        .unwrap();
    assert!(delta.contains("p.first_seen_at>=TIMESTAMPTZ '2026-11-05 17:30:32+00'"));
    assert!(delta.contains("first_seen_at>=TIMESTAMPTZ '2026-11-05 17:30:32+00'"));
    assert!(delta.contains("IS DISTINCT FROM lower(v.accepting_hash)"));
    assert!(delta.contains("THEN NOW()-INTERVAL '60 seconds'"));
    assert!(delta.contains("THEN 0 ELSE p.attempts END"));
    assert!(delta.contains("THEN 0 ELSE attempts END"));
    assert!(delta.contains("p.txid=v.txid"));
}
#[test]
fn only_public_aggregation_for_current_proof_health() {
    let sql = include_str!("../src/presentation/telegram/workers/telegram_delivery.rs");
    assert!(sql.contains("pending_rewards_repo::OP076_RETRY_INTERVAL_SQL"));
    let metrics = include_str!("../src/infrastructure/metrics.rs");
    for term in [
        "kaspa_pulse_proof_historic_excluded",
        "kaspa_pulse_proof_newer_waiting",
        "kaspa_pulse_proof_newer_due_for_retry",
        "kaspa_pulse_proof_newer_max_attempts",
    ] {
        assert!(metrics.contains(term));
    }
    let api = include_str!("../src/network/public_api_historical.rs");
    assert!(api.contains("proof_gate.acquire()"));
    assert!(api.contains("Semaphore::new(1)"));
    assert!(!api.contains("try_acquire()"));
}
#[test]
fn source_generation_vspc_commit_fence_and_single_wallet_writer_unchanged() {
    let worker = include_str!("../src/presentation/telegram/workers/utxo_monitor.rs");
    assert!(worker.contains("services.get(index).cloned()"));
    let vspc = include_str!("../src/network/vspc_proof.rs");
    assert!(vspc.contains("registry.fence(captured).await"));
    assert!(vspc.contains(".apply_vspc_proof_delta("));
    let registry = include_str!("../src/infrastructure/node/source_registry.rs");
    assert!(registry.contains("replacement of current transactional writer prohibited"));
    assert!(registry.contains("duplicate public PNN URL refused"));
}
