//! Live worker and VSPC must use EXACTLY one shared generation + DB fence.
#[test]
fn scanner_and_indexer_share_one_generation_registry() {
    let worker = include_str!("../src/presentation/telegram/workers/utxo_monitor.rs");
    assert!(worker.contains("let pnn_generation = five_way_sources.as_ref()"));
    assert!(worker.contains("let indexer_sources = pnn_generation.clone()"));
    assert!(worker.contains("generation.select(choice.active_index).await"));
    assert!(worker.contains("services.get(index).cloned()"));
    let indexer = include_str!("../src/network/vspc_proof.rs");
    assert!(indexer.contains("registry.selected().await"));
    assert!(indexer.contains("registry.fence(captured).await"));
    assert!(indexer.contains("_generation_fence"));
    assert!(indexer.contains(".apply_vspc_proof_delta("));
    assert!(!indexer.contains("pnn_pool_active_index()"));
}
#[test]
fn immutable_endpoints_and_outbox_replay_guards_remain_in_force() {
    let reg = include_str!("../src/infrastructure/node/source_registry.rs");
    assert!(reg.contains("selected_index"));
    assert!(reg.contains("tokio::sync::{RwLock, RwLockReadGuard}"));
    assert!(reg.contains("pub async fn replace_inactive("));
    assert!(reg.contains("replacement of current transactional writer prohibited"));
    assert!(reg.contains("source generation changed during public resolver discovery"));
    let w = include_str!("../src/presentation/telegram/workers/utxo_monitor.rs");
    assert!(w.contains("OP064_FIRST_ELIGIBLE_BLOCK_MS: u64 = 1793899832000"));
    let db = include_str!("../src/infrastructure/database/pending_rewards_repo.rs");
    assert!(db.contains("AND first_seen_at >= TIMESTAMPTZ '2026-11-05 17:30:32+00'"));
}
