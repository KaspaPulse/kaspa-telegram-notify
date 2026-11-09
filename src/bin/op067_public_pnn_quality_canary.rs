//! OP067: independent read-only public PNN diversity and quality probe.
use kaspa_consensus_core::network::NetworkId;
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_wrpc_client::{KaspaRpcClient, Resolver, WrpcEncoding};
use std::{
    collections::HashSet,
    str::FromStr,
    time::{Duration, Instant},
};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let network = NetworkId::from_str("mainnet")?;
    let resolver = Resolver::default();
    let deadline = Instant::now() + Duration::from_secs(60);
    let mut unique = HashSet::new();
    let mut urls = Vec::new();
    for _ in 0..70 {
        if Instant::now() >= deadline || urls.len() >= 10 {
            break;
        }
        if let Ok(Ok(url)) = tokio::time::timeout(
            Duration::from_secs(6),
            resolver.get_url(WrpcEncoding::Borsh, network),
        )
        .await
            && unique.insert(url.clone())
        {
            urls.push(url);
        }
    }
    println!("OP067_DISCOVERED_DISTINCT_ENDPOINTS {}", urls.len());
    let mut healthy = Vec::new();
    for url in urls {
        let start = Instant::now();
        let client =
            KaspaRpcClient::new(WrpcEncoding::Borsh, Some(&url), None, Some(network), None)?;
        let connected = tokio::time::timeout(
            Duration::from_secs(9),
            client.connect(Some(
                kaspa_wrpc_client::client::ConnectOptions::blocking_fallback(),
            )),
        )
        .await;
        if !matches!(connected, Ok(Ok(_))) {
            println!("OP067_REJECT_CONNECT");
            continue;
        }
        let health = tokio::time::timeout(Duration::from_secs(6), async {
            let info = client.get_server_info().await?;
            let peers = client.get_connected_peer_info().await?.peer_info.len();
            Ok::<_, kaspa_rpc_core::RpcError>((info, peers))
        })
        .await;
        match health {
            Ok(Ok((info, peers)))
                if info.network_id.to_string() == "mainnet"
                    && info.is_synced
                    && info.has_utxo_index
                    && info.virtual_daa_score > 0
                    && peers > 0 =>
            {
                println!(
                    "OP067_QUALIFIED rank={} peers={} daa={} latency_ms={}",
                    healthy.len() + 1,
                    peers,
                    info.virtual_daa_score,
                    start.elapsed().as_millis()
                );
                healthy.push((client, info.virtual_daa_score));
            }
            _ => {
                println!("OP067_REJECT_HEALTH");
                let _ = client.disconnect().await;
            }
        }
        if healthy.len() >= 5 {
            break;
        }
    }
    anyhow::ensure!(healthy.len() >= 5, "OP067_FEWER_THAN_5_QUALIFIED_SOURCES");
    let newest = healthy
        .iter()
        .map(|(_, daa)| *daa)
        .max()
        .unwrap_or_default();
    anyhow::ensure!(
        healthy
            .iter()
            .all(|(_, daa)| newest.saturating_sub(*daa) <= 64),
        "OP067_STALE_SOURCE"
    );
    for round in 0..4 {
        for (index, (node, _)) in healthy.iter().enumerate() {
            let test = tokio::time::timeout(Duration::from_secs(5), node.get_server_info()).await;
            anyhow::ensure!(
                matches!(test, Ok(Ok(_))),
                "OP067_SOURCE_FAILED rank={} round={}",
                index + 1,
                round
            );
        }
        tokio::time::sleep(Duration::from_millis(900)).await;
    }
    println!("OP067_FIVE_PUBLIC_PNN_HEALTHY_PASS NO_DB_NO_TELEGRAM");
    for (node, _) in healthy {
        let _ = node.disconnect().await;
    }
    Ok(())
}
