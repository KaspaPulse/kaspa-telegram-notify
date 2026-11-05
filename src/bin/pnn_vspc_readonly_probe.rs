use kaspa_consensus_core::network::NetworkId;
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_wrpc_client::{KaspaRpcClient, Resolver, WrpcEncoding};
use std::str::FromStr;
use std::time::{Duration, Instant};

#[tokio::main]
async fn main() {
    if let Err(error) = probe().await {
        eprintln!("OP005_PNN_STAGE_DIAGNOSTIC_FAIL={error:#}");
        std::process::exit(1);
    }
}

async fn probe() -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let network = NetworkId::from_str("mainnet")?;
    let start = Instant::now();

    eprintln!("OP005_PNN_STAGE=RESOLVE_PUBLIC_BORSH");
    let url = tokio::time::timeout(
        Duration::from_secs(12),
        Resolver::default().get_url(WrpcEncoding::Borsh, network),
    )
    .await
    .map_err(|_| anyhow::anyhow!("RESOLVER_TIMEOUT"))??;
    eprintln!("OP005_PNN_STAGE=RESOLVED url={url}");

    let client = KaspaRpcClient::new(WrpcEncoding::Borsh, Some(&url), None, Some(network), None)?;
    eprintln!("OP005_PNN_STAGE=CONNECT_BLOCKING_FALLBACK");
    tokio::time::timeout(
        Duration::from_secs(20),
        client.connect(Some(
            kaspa_wrpc_client::client::ConnectOptions::blocking_fallback(),
        )),
    )
    .await
    .map_err(|_| anyhow::anyhow!("CONNECT_TIMEOUT"))??;

    eprintln!("OP005_PNN_STAGE=BLOCKDAG_INFO");
    let info = tokio::time::timeout(Duration::from_secs(15), client.get_block_dag_info())
        .await
        .map_err(|_| anyhow::anyhow!("DAG_INFO_TIMEOUT"))??;

    eprintln!("OP005_PNN_STAGE=GET_SINK_BLOCK");
    let latest = tokio::time::timeout(Duration::from_secs(15), client.get_block(info.sink, false))
        .await
        .map_err(|_| anyhow::anyhow!("GET_SINK_TIMEOUT"))??;
    let parent = latest
        .verbose_data
        .as_ref()
        .ok_or_else(|| anyhow::anyhow!("NO_SELECTED_PARENT_METADATA"))?
        .selected_parent_hash;
    anyhow::ensure!(parent != kaspa_hashes::Hash::default(), "NO_PARENT");

    eprintln!("OP005_PNN_STAGE=VSPC_ACCEPTANCE_QUERY");
    let t = Instant::now();
    let response = tokio::time::timeout(
        Duration::from_secs(20),
        client.get_virtual_chain_from_block(parent, true, Some(2)),
    )
    .await
    .map_err(|_| anyhow::anyhow!("VSPC_TIMEOUT"))??;
    let accepted: usize = response
        .accepted_transaction_ids
        .iter()
        .map(|v| v.accepted_transaction_ids.len())
        .sum();
    println!(
        "OP005_PNN_VSPC_READONLY_PASS added={} removed={} accepted={} latency_ms={} total_ms={}",
        response.added_chain_block_hashes.len(),
        response.removed_chain_block_hashes.len(),
        accepted,
        t.elapsed().as_millis(),
        start.elapsed().as_millis()
    );
    client.disconnect().await?;
    Ok(())
}
