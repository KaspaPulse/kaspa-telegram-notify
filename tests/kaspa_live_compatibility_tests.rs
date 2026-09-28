use kaspa_consensus_core::network::NetworkId;
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_wrpc_client::prelude::{
    ChannelConnection, ChannelType, KaspaRpcClient, Notification, Resolver, Scope,
    VirtualDaaScoreChangedScope, WrpcEncoding,
};
use serde_json::json;
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use std::time::Duration;

const PROVIDER_RETRIES: u32 = 3;
const RESOLVER_TIMEOUT: Duration = Duration::from_secs(10);
const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
const RPC_TIMEOUT: Duration = Duration::from_secs(15);
const OVERALL_TIMEOUT: Duration = Duration::from_secs(90);

async fn selected_live_target(network_id: NetworkId) -> (String, &'static str, WrpcEncoding) {
    if let Ok(value) = std::env::var("KASPA_LIVE_WRPC_URL") {
        let value = value.trim();
        if !value.is_empty() && !value.eq_ignore_ascii_case("resolver") {
            return (value.to_owned(), "explicit-json", WrpcEncoding::SerdeJson);
        }
    }

    for attempt in 1..=PROVIDER_RETRIES {
        match tokio::time::timeout(
            RESOLVER_TIMEOUT,
            Resolver::default().get_url(WrpcEncoding::Borsh, network_id),
        )
        .await
        {
            Ok(Ok(url)) => return (url, "public-resolver-borsh", WrpcEncoding::Borsh),
            Ok(Err(_)) | Err(_) if attempt < PROVIDER_RETRIES => {
                tokio::time::sleep(Duration::from_secs(1_u64 << (attempt - 1))).await;
            }
            Ok(Err(_)) | Err(_) => break,
        }
    }

    panic!("Kaspa public resolver did not return a Borsh wRPC endpoint after bounded retries");
}

async fn connect_live_client(
    url: &str,
    network_id: NetworkId,
    encoding: WrpcEncoding,
) -> Arc<KaspaRpcClient> {
    for attempt in 1..=PROVIDER_RETRIES {
        let client = Arc::new(
            KaspaRpcClient::new(encoding, Some(url), None, Some(network_id), None)
                .expect("Kaspa v2.1 wRPC client must construct"),
        );

        match tokio::time::timeout(CONNECT_TIMEOUT, client.connect(Some(Default::default()))).await
        {
            Ok(Ok(_)) => return client,
            Ok(Err(_)) | Err(_) if attempt < PROVIDER_RETRIES => {
                tokio::time::sleep(Duration::from_secs(1_u64 << (attempt - 1))).await;
            }
            Ok(Err(_)) | Err(_) => break,
        }
    }

    panic!("live Kaspa wRPC connection did not succeed after bounded retries");
}

fn write_external_evidence(value: &serde_json::Value) {
    let Ok(path) = std::env::var("EXTERNAL_CONTRACT_EVIDENCE_PATH") else {
        return;
    };
    let path = PathBuf::from(path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("external evidence directory must be creatable");
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(value).expect("external evidence must serialize");
    let mut file = File::create(&tmp).expect("external evidence temp file must be creatable");
    file.write_all(&bytes)
        .expect("external evidence temp file must be writable");
    file.write_all(b"\n")
        .expect("external evidence newline must be writable");
    file.sync_all()
        .expect("external evidence temp file must fsync");
    fs::rename(&tmp, &path).expect("external evidence rename must succeed");
}

#[tokio::test]
#[ignore = "requires a live Kaspa mainnet provider or the public resolver"]
async fn live_latest_kaspa_wrpc_contract() {
    let _ = rustls::crypto::ring::default_provider().install_default();
    tokio::time::timeout(OVERALL_TIMEOUT, live_latest_kaspa_wrpc_contract_inner())
        .await
        .expect("live Kaspa contract exceeded the overall timeout");
}

async fn live_latest_kaspa_wrpc_contract_inner() {
    let network_id = NetworkId::from_str("mainnet").expect("mainnet network id must parse");
    let (url, endpoint_mode, encoding) = selected_live_target(network_id).await;
    let client = connect_live_client(&url, network_id, encoding).await;

    let server_info = tokio::time::timeout(RPC_TIMEOUT, client.get_server_info())
        .await
        .expect("get_server_info timed out")
        .expect("get_server_info must remain compatible");
    if let Ok(expected_server_version) = std::env::var("KASPA_EXPECTED_SERVER_VERSION") {
        let expected_server_version = expected_server_version.trim();
        if !expected_server_version.is_empty() {
            assert!(
                server_info.server_version.contains(expected_server_version),
                "live node version does not contain the expected server version"
            );
        }
    }
    assert!(
        !server_info.server_version.trim().is_empty(),
        "live Kaspa node must report a non-empty server version"
    );
    assert!(
        server_info.is_synced,
        "live Kaspa node must be synchronized"
    );
    assert!(
        server_info.has_utxo_index,
        "live Kaspa compatibility gate requires the UTXO index"
    );

    tokio::time::timeout(RPC_TIMEOUT, client.get_sync_status())
        .await
        .expect("get_sync_status timed out")
        .expect("get_sync_status must remain compatible");
    tokio::time::timeout(RPC_TIMEOUT, client.get_block_dag_info())
        .await
        .expect("get_block_dag_info timed out")
        .expect("get_block_dag_info must remain compatible");
    tokio::time::timeout(RPC_TIMEOUT, client.get_coin_supply())
        .await
        .expect("get_coin_supply timed out")
        .expect("get_coin_supply must remain compatible");
    tokio::time::timeout(
        RPC_TIMEOUT,
        client.estimate_network_hashes_per_second(1000, None),
    )
    .await
    .expect("estimate_network_hashes_per_second timed out")
    .expect("estimate_network_hashes_per_second must remain compatible");
    tokio::time::timeout(RPC_TIMEOUT, client.get_utxos_by_addresses(Vec::new()))
        .await
        .expect("get_utxos_by_addresses timed out")
        .expect("empty get_utxos_by_addresses probe must remain compatible");

    let (sender, receiver) = async_channel::unbounded::<Notification>();
    let listener_id = client
        .rpc_api()
        .register_new_listener(ChannelConnection::new(
            "kaspa-pulse-live-compatibility",
            sender,
            ChannelType::Persistent,
        ));

    tokio::time::timeout(
        RPC_TIMEOUT,
        client.rpc_api().start_notify(
            listener_id,
            Scope::VirtualDaaScoreChanged(VirtualDaaScoreChangedScope {}),
        ),
    )
    .await
    .expect("VirtualDaaScoreChanged subscription registration timed out")
    .expect("VirtualDaaScoreChanged subscription must register");

    let notification = tokio::time::timeout(Duration::from_secs(20), receiver.recv())
        .await
        .expect("live Kaspa node must emit a DAA-score notification within the gate timeout")
        .expect("notification channel must stay open");

    assert!(
        matches!(notification, Notification::VirtualDaaScoreChanged(_)),
        "subscription returned an unexpected notification variant: {notification:?}"
    );

    tokio::time::timeout(
        RPC_TIMEOUT,
        client.rpc_api().unregister_listener(listener_id),
    )
    .await
    .expect("listener unregister timed out")
    .expect("listener unregister must succeed");
    tokio::time::timeout(RPC_TIMEOUT, client.disconnect())
        .await
        .expect("live Kaspa disconnect timed out")
        .expect("live Kaspa wRPC client must disconnect cleanly");

    println!(
        "KASPA_EXTERNAL_CONTRACT=PASS endpoint_mode={endpoint_mode} server_version={}",
        server_info.server_version
    );

    write_external_evidence(&json!({
        "schema_version": "1.0.0",
        "provider": "kaspa-mainnet-wrpc",
        "result": "PASS",
        "tested_sha": std::env::var("GITHUB_SHA").unwrap_or_else(|_| "local".to_owned()),
        "endpoint_mode": endpoint_mode,
        "server_version": server_info.server_version,
        "is_synced": server_info.is_synced,
        "has_utxo_index": server_info.has_utxo_index,
        "contracts": [
            "getServerInfo",
            "getSyncStatus",
            "getBlockDagInfo",
            "getCoinSupply",
            "estimateNetworkHashesPerSecond",
            "getUtxosByAddresses",
            "VirtualDaaScoreChanged"
        ]
    }));
}
