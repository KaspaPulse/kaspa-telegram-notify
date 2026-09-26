use kaspa_consensus_core::network::NetworkId;
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_wrpc_client::prelude::{
    ChannelConnection, ChannelType, KaspaRpcClient, Notification, Scope,
    VirtualDaaScoreChangedScope, WrpcEncoding,
};
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

#[tokio::test]
#[ignore = "requires an explicitly selected live Kaspa node"]
async fn live_latest_kaspa_wrpc_contract() {
    let url = std::env::var("KASPA_LIVE_WRPC_URL")
        .expect("KASPA_LIVE_WRPC_URL is required for the live Kaspa compatibility gate");
    let network_id = NetworkId::from_str("mainnet").expect("mainnet network id must parse");

    let client = Arc::new(
        KaspaRpcClient::new(
            WrpcEncoding::SerdeJson,
            Some(&url),
            None,
            Some(network_id),
            None,
        )
        .expect("Kaspa v2.1 wRPC client must construct"),
    );

    client
        .connect(Some(Default::default()))
        .await
        .expect("live Kaspa wRPC connection must succeed");

    let server_info = client
        .get_server_info()
        .await
        .expect("get_server_info must remain compatible");
    let expected_server_version = std::env::var("KASPA_EXPECTED_SERVER_VERSION")
        .expect("KASPA_EXPECTED_SERVER_VERSION is required for the live Kaspa compatibility gate");
    assert!(
        server_info
            .server_version
            .contains(&expected_server_version),
        "live node version {} does not contain expected version {}",
        server_info.server_version,
        expected_server_version
    );
    assert!(
        server_info.is_synced,
        "live Kaspa node must be synchronized"
    );
    assert!(
        server_info.has_utxo_index,
        "live Kaspa compatibility gate requires the UTXO index"
    );
    client
        .get_sync_status()
        .await
        .expect("get_sync_status must remain compatible");
    client
        .get_block_dag_info()
        .await
        .expect("get_block_dag_info must remain compatible");
    client
        .get_coin_supply()
        .await
        .expect("get_coin_supply must remain compatible");
    client
        .estimate_network_hashes_per_second(1000, None)
        .await
        .expect("estimate_network_hashes_per_second must remain compatible");
    client
        .get_utxos_by_addresses(Vec::new())
        .await
        .expect("empty get_utxos_by_addresses probe must remain compatible");

    let (sender, receiver) = async_channel::unbounded::<Notification>();
    let listener_id = client
        .rpc_api()
        .register_new_listener(ChannelConnection::new(
            "kaspa-pulse-live-compatibility",
            sender,
            ChannelType::Persistent,
        ));

    client
        .rpc_api()
        .start_notify(
            listener_id,
            Scope::VirtualDaaScoreChanged(VirtualDaaScoreChangedScope {}),
        )
        .await
        .expect("VirtualDaaScoreChanged subscription must register");

    let notification = tokio::time::timeout(Duration::from_secs(15), receiver.recv())
        .await
        .expect("live Kaspa node must emit a DAA-score notification within the gate timeout")
        .expect("notification channel must stay open");

    assert!(
        matches!(notification, Notification::VirtualDaaScoreChanged(_)),
        "subscription returned an unexpected notification variant: {notification:?}"
    );

    client
        .rpc_api()
        .unregister_listener(listener_id)
        .await
        .expect("listener unregister must succeed");
    client
        .disconnect()
        .await
        .expect("live Kaspa wRPC client must disconnect cleanly");
}
