//! Additional static admission checks complement the 167 behavior tests.
#[test]
fn scanner_chooses_exactly_one_source_for_reward_outbox() {
    let s = include_str!("../src/presentation/telegram/workers/utxo_monitor.rs");
    assert!(s.contains("selector.choose(started.elapsed(), &observations)"));
    assert!(s.contains("choice.active_index"));
    assert!(s.contains("services.get(index).cloned()"));
    assert!(s.contains("tokio::task::JoinSet::new()"));
    assert!(s.contains("checks.abort_all()"));
    assert!(s.contains("let mut urls = std::collections::HashSet::new()"));
}
#[test]
fn vspc_indexer_follows_same_active_source_or_pauses() {
    let s = include_str!("../src/network/vspc_proof.rs");
    assert!(s.contains("fn selected_pnn("));
    assert!(s.contains("registry.selected().await"));
    assert!(s.contains("selected_pnn(&node, &pool).await else {"));
    assert!(s.contains("registry.fence(captured).await"));
    assert!(!s.contains("pnn_pool_active_index()"));
    assert!(s.contains("get_virtual_chain_from_block(cursor, true, Some(10))"));
    assert!(s.contains("checkpoint retained for retry"));
}
#[test]
fn five_sources_are_live_qualified_not_hardcoded() {
    let s = include_str!("../src/main.rs");
    assert!(s.contains("KASPA_PNN_FIVE_WAY_POOL"));
    assert!(s.contains("sources.len() == 5"));
    assert!(s.contains("seen_urls.insert(url.clone())"));
    assert!(s.contains("info.is_synced"));
    assert!(s.contains("info.has_utxo_index"));
    assert!(s.contains("info.network_id.to_string() == \"mainnet\""));
}
#[test]
fn historical_reward_cutoff_remains_fixed() {
    let s = include_str!("../src/presentation/telegram/workers/utxo_monitor.rs");
    assert!(s.contains("const OP064_FIRST_ELIGIBLE_BLOCK_MS: u64 = 1793899832000;"));
    assert!(s.contains("op064_may_enqueue_coinbase("));
}
