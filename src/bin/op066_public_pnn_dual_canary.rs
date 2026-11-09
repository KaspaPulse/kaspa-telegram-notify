//! Read-only independent public PNN quality canary. Never uses Telegram/DB.
use kaspa_consensus_core::network::NetworkId;
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_wrpc_client::{KaspaRpcClient, Resolver, WrpcEncoding};
use std::{
    str::FromStr,
    time::{Duration, Instant},
};

async fn qualify(url: &str, net: NetworkId, label: &str) -> anyhow::Result<KaspaRpcClient> {
    let client = KaspaRpcClient::new(WrpcEncoding::Borsh, Some(url), None, Some(net), None)?;
    tokio::time::timeout(
        Duration::from_secs(12),
        client.connect(Some(
            kaspa_wrpc_client::client::ConnectOptions::blocking_fallback(),
        )),
    )
    .await??;
    let info = tokio::time::timeout(Duration::from_secs(7), client.get_server_info()).await??;
    let peers = tokio::time::timeout(Duration::from_secs(7), client.get_connected_peer_info())
        .await??
        .peer_info
        .len();
    anyhow::ensure!(
        info.network_id.to_string() == "mainnet"
            && info.is_synced
            && info.has_utxo_index
            && info.virtual_daa_score > 0
            && peers > 0,
        "{label} not eligible synced mainnet indexed public PNN"
    );
    println!(
        "OP066_{label}_QUALIFIED peers={peers} daa={} is_connected={}",
        info.virtual_daa_score,
        client.is_connected()
    );
    Ok(client)
}
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let network = NetworkId::from_str("mainnet")?;
    let resolver = Resolver::default();
    let start = Instant::now();
    let mut primary = None;
    let mut standby = None;
    for attempt in 0..14 {
        let result = tokio::time::timeout(
            Duration::from_secs(8),
            resolver.get_url(WrpcEncoding::Borsh, network),
        )
        .await;
        if let Ok(Ok(url)) = result {
            if primary.is_none() {
                primary = Some(url);
            } else if primary.as_deref() != Some(url.as_str()) {
                standby = Some(url);
                break;
            }
        }
        if attempt < 13 {
            tokio::time::sleep(Duration::from_millis(350)).await;
        }
    }
    let p = primary.ok_or_else(|| anyhow::anyhow!("no public PNN endpoint resolved"))?;
    let s = standby.ok_or_else(|| anyhow::anyhow!("no independent standby resolved"))?;
    anyhow::ensure!(p != s, "PNN endpoint collision");
    println!("OP066_DISTINCT_ENDPOINTS_RESOLVED=TRUE");
    let a = qualify(&p, network, "PRIMARY").await?;
    let b = qualify(&s, network, "STANDBY").await?;
    let mut p_fail = 0u32;
    let mut s_fail = 0u32;
    let mut p_checked = 0u32;
    for _ in 0..8 {
        if !a.is_connected() {
            p_fail += 1;
        }
        if !b.is_connected() {
            s_fail += 1;
        }
        let primary = tokio::time::timeout(Duration::from_secs(5), a.get_server_info()).await;
        let backup = tokio::time::timeout(Duration::from_secs(5), b.get_server_info()).await;
        if !matches!(primary, Ok(Ok(_))) {
            p_fail += 1;
        }
        if !matches!(backup, Ok(Ok(_))) {
            s_fail += 1;
        }
        p_checked += 1;
        tokio::time::sleep(Duration::from_secs(3)).await;
    }
    println!(
        "OP066_DUAL_PNN_CANARY samples={p_checked} primary_failures={p_fail} standby_failures={s_fail} elapsed_s={}",
        start.elapsed().as_secs()
    );
    a.disconnect().await?;
    b.disconnect().await?;
    anyhow::ensure!(
        p_fail == 0 && s_fail == 0,
        "public PNN canary unstable: fail closed"
    );
    println!("OP066_DUAL_PNN_CANARY_PASS_NO_DB_NO_SEND");
    Ok(())
}
