// Read-only mainnet wRPC readiness probe. No mining, DB, or Telegram access.
use kaspa_consensus_core::network::NetworkId;
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_wrpc_client::{KaspaRpcClient, WrpcEncoding};
use std::str::FromStr;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let endpoint = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "ws://127.0.0.1:18110".to_string());
    anyhow::ensure!(
        endpoint.starts_with("ws://127.0.0.1:") || endpoint.starts_with("ws://[::1]:"),
        "local-only probe; remote binding forbidden"
    );
    let expected = NetworkId::from_str("mainnet")?;
    let client = KaspaRpcClient::new(
        WrpcEncoding::SerdeJson,
        Some(&endpoint),
        None,
        Some(expected),
        None,
    )?;
    tokio::time::timeout(Duration::from_secs(8), client.connect(None)).await??;
    let info = tokio::time::timeout(Duration::from_secs(8), client.get_server_info()).await??;
    let peers = tokio::time::timeout(Duration::from_secs(8), client.get_connected_peer_info())
        .await?
        .ok()
        .map(|p| p.peer_info.len())
        .unwrap_or(0);
    let mainnet = info.network_id.to_string() == "mainnet";
    let ready = mainnet && info.is_synced && info.has_utxo_index && peers > 0;
    println!(
        "UTXO_PROBE_READY={} NETWORK={} SYNC={} UTXOINDEX={} PEERS={} DAA={} API={}.{} VERSION={}",
        ready,
        info.network_id,
        info.is_synced,
        info.has_utxo_index,
        peers,
        info.virtual_daa_score,
        info.rpc_api_version,
        info.rpc_api_revision,
        info.server_version,
    );
    anyhow::ensure!(ready, "not an eligible standby; deployment must HOLD");
    Ok(())
}
