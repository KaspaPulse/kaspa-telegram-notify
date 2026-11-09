//! Regression for actual OP067 guarded-release failure: primary URL absent.
#[test]
fn five_public_reward_sources_do_not_depend_on_primary_wrpc_url() {
    let code = include_str!("../src/main.rs");
    let five = code
        .split("KASPA_PNN_FIVE_WAY_POOL")
        .nth(1)
        .expect("five way block")
        .split("} else if std::env::var(\"KASPA_PNN_DISTINCT_STANDBY\")")
        .next()
        .unwrap();
    assert!(five.contains("let mut seen_urls: HashSet<String> = HashSet::new();"));
    assert!(five.contains("Vec::with_capacity(5)"));
    assert!(!five.contains("node_provider.client.url()"));
    assert!(!five.contains("Primary public PNN URL unavailable"));
    assert!(five.contains("sources.len() == 5"));
}
#[test]
fn primary_health_cannot_override_selected_independent_reward_node() {
    let code = include_str!("../src/infrastructure/external_services/system.rs");
    assert!(code.contains("if !has_reward_alternate"));
    assert!(code.contains("set_node_connected(false)"));
    let monitor = include_str!("../src/presentation/telegram/workers/utxo_monitor.rs");
    assert!(monitor.contains("set_node_connected(true)"));
    assert!(monitor.contains("set_pnn_pool_active_index(choice.active_index)"));
}
