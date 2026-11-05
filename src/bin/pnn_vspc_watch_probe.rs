use kaspa_consensus_core::network::NetworkId;
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_wrpc_client::{KaspaRpcClient, Resolver, WrpcEncoding};
use std::str::FromStr;
use std::time::{Duration, Instant};
#[tokio::main]
async fn main() {
    let result = probe().await;
    match result {
        Ok(()) => println!("OP017_VSPC_WATCH_PROBE_PASS"),
        Err(e) => {
            eprintln!("OP017_VSPC_WATCH_PROBE_FAIL={e}");
            std::process::exit(1);
        }
    }
}
async fn probe() -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let mainnet = NetworkId::from_str("mainnet")?;
    let client = KaspaRpcClient::new(
        WrpcEncoding::Borsh,
        None,
        Some(Resolver::default()),
        Some(mainnet),
        None,
    )?;
    tokio::time::timeout(Duration::from_secs(20), client.connect(None)).await??;
    let info = tokio::time::timeout(Duration::from_secs(15), client.get_block_dag_info()).await??;
    let mut cursor = info.sink;
    let mut total_added = 0usize;
    let mut total_accepted = 0usize;
    for iteration in 0..4 {
        let started = Instant::now();
        let response = tokio::time::timeout(
            Duration::from_secs(15),
            client.get_virtual_chain_from_block(cursor, true, Some(10)),
        )
        .await??;
        let added = response.added_chain_block_hashes.len();
        let removed = response.removed_chain_block_hashes.len();
        let accepted: usize = response
            .accepted_transaction_ids
            .iter()
            .map(|v| v.accepted_transaction_ids.len())
            .sum();
        total_added += added;
        total_accepted += accepted;
        println!(
            "OP017_VSPC_STEP={} added={} removed={} accepted={} latency_ms={}",
            iteration,
            added,
            removed,
            accepted,
            started.elapsed().as_millis()
        );
        if let Some(last) = response.added_chain_block_hashes.last() {
            cursor = *last;
        }
        if iteration < 3 {
            tokio::time::sleep(Duration::from_secs(5)).await;
        }
    }
    client.disconnect().await?;
    anyhow::ensure!(
        total_added > 0 && total_accepted > 0,
        "NO_DELTA_OR_ACCEPTED_IDS"
    );
    Ok(())
}
