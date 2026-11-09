//! Source-level negative regression contracts complement the failover state tests.
#[test]
fn read_only_pnn_health_cannot_force_disconnect() {
    let source = include_str!("../src/infrastructure/node/kaspa_adapter.rs");
    let health = source
        .split("pub async fn get_node_health(")
        .nth(1)
        .expect("health");
    let body = health
        .split("pub async fn get_block(")
        .next()
        .expect("body");
    assert!(
        !body.contains(".connect("),
        "a health query must never reconnect shared wRPC"
    );
    assert!(body.contains("!self.client.is_connected()"));
    assert!(body.contains("with_rpc_timeout"));
}
#[test]
fn connectivity_monitor_does_not_disconnect_a_healthy_shared_client() {
    let source = include_str!("../src/infrastructure/external_services/system.rs");
    assert!(source.contains("let initial_connected = if ctx.rpc.is_connected()"));
    assert!(source.contains("failed_attempts >= 3"));
    assert!(source.contains("(failed_attempts - 3) % 5 == 0"));
    assert!(source.contains("!ctx.rpc.is_connected()"));
    assert!(source.contains("inc_pnn_forced_recoveries"));
}
#[test]
fn disconnect_signal_does_not_unsubscribe_over_dead_websocket() {
    let source = include_str!("../src/infrastructure/node/subscription.rs");
    let disconnect = source
        .split("Ok(RpcState::Disconnected) => {")
        .nth(1)
        .expect("disconnect handler");
    let body = disconnect.split("Err(error) => {").next().expect("body");
    assert!(body.contains("listener_id = None"));
    assert!(body.contains("signal_sender.signal()"));
    assert!(!body.contains("unregister_listener(&self.client"));
}
#[test]
fn mainnet_standby_is_qualified_independently_and_single_reader_selected() {
    let main = include_str!("../src/main.rs");
    let standby = main
        .split("KASPA_PNN_DISTINCT_STANDBY")
        .nth(1)
        .expect("opt in");
    assert!(standby.contains("No distinct public PNN endpoint found"));
    assert!(standby.contains("info.is_synced"));
    assert!(standby.contains("info.has_utxo_index"));
    assert!(standby.contains("info.virtual_daa_score > 0"));
    assert!(standby.contains("info.network_id.to_string() == \"mainnet\""));
    let monitor = include_str!("../src/presentation/telegram/workers/utxo_monitor.rs");
    assert!(monitor.contains("failover.choose("));
    assert!(monitor.contains("Selection::Ready(UtxoNode::KasStandby) => standby_service.clone()"));
    assert!(monitor.contains("set_pnn_utxo_standby_selected"));
}
