#[test]
fn historical_proof_recovery_requires_original_fixed_cutover_boundary() {
    let src = include_str!("../src/infrastructure/database/pending_rewards_repo.rs");
    assert!(src.contains("AND first_seen_at >= TIMESTAMPTZ '2026-11-05 17:30:32+00'"));
    assert!(src.contains("WHERE wallet=$1 AND status='awaiting_block_proof'"));
    assert!(src.contains("get_awaiting_proof_outpoints"));
    assert!(!src.contains("DELETE FROM pending_rewards WHERE status='awaiting_block_proof'"));
}
