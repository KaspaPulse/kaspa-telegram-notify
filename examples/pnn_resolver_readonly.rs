// Secret-free read-only PNN mainnet smoke test; no bot startup.
use kaspa_consensus_core::network::NetworkId;
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_wrpc_client::client::ConnectOptions;
use kaspa_wrpc_client::{KaspaRpcClient, Resolver, WrpcEncoding};
use std::str::FromStr;
use std::time::Duration;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }
    anyhow::ensure!(
        rustls::crypto::CryptoProvider::get_default().is_some(),
        "TLS provider unavailable"
    );
    let network_id = NetworkId::from_str("mainnet")?;
    let rpc = KaspaRpcClient::new(
        WrpcEncoding::Borsh,
        None,
        Some(Resolver::default()),
        Some(network_id),
        None,
    )?;
    tokio::time::timeout(
        Duration::from_secs(30),
        rpc.connect(Some(ConnectOptions::blocking_fallback())),
    )
    .await??;
    tokio::time::timeout(Duration::from_secs(15), rpc.get_server_info()).await??;
    println!("PNN_BORSH_RESOLVER_MAINNET_RPC_PASS");
    Ok(())
}
