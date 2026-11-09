//! OP070 independent, read-only real public-PNN failover proof.
//! Every connection belongs to this canary. Never accesses Telegram or PG.
use kaspa_consensus_core::network::NetworkId;
use kaspa_pulse::infrastructure::node::quality_selector::{Quality, QualitySelector};
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
    let end = Instant::now() + Duration::from_secs(85);
    let mut urls = HashSet::new();
    let mut clients = Vec::new();
    for _ in 0..80 {
        if clients.len() == 5 || Instant::now() > end {
            break;
        }
        let url = match tokio::time::timeout(
            Duration::from_secs(6),
            resolver.get_url(WrpcEncoding::Borsh, network),
        )
        .await
        {
            Ok(Ok(url)) if urls.insert(url.clone()) => url,
            _ => continue,
        };
        let client =
            KaspaRpcClient::new(WrpcEncoding::Borsh, Some(&url), None, Some(network), None)?;
        if !matches!(
            tokio::time::timeout(
                Duration::from_secs(8),
                client.connect(Some(
                    kaspa_wrpc_client::client::ConnectOptions::blocking_fallback()
                ))
            )
            .await,
            Ok(Ok(_))
        ) {
            continue;
        }
        let check = tokio::time::timeout(Duration::from_secs(5), async {
            let info = client.get_server_info().await?;
            let peers = client.get_connected_peer_info().await?.peer_info.len();
            Ok::<_, kaspa_rpc_core::RpcError>((info, peers))
        })
        .await;
        match check {
            Ok(Ok((info, peers)))
                if info.network_id.to_string() == "mainnet"
                    && info.is_synced
                    && info.has_utxo_index
                    && peers > 0
                    && info.virtual_daa_score > 0 =>
            {
                clients.push(client);
            }
            _ => {
                let _ = client.disconnect().await;
            }
        }
    }
    anyhow::ensure!(clients.len() == 5, "FIVE_PUBLIC_SOURCES_UNAVAILABLE");
    let mut health = Vec::new();
    for client in &clients {
        let started = Instant::now();
        let (info, peers) = tokio::time::timeout(Duration::from_secs(5), async {
            Ok::<_, kaspa_rpc_core::RpcError>((
                client.get_server_info().await?,
                client.get_connected_peer_info().await?.peer_info.len(),
            ))
        })
        .await??;
        health.push(Some(Quality {
            is_mainnet: info.network_id.to_string() == "mainnet",
            is_synced: info.is_synced,
            has_utxo_index: info.has_utxo_index,
            peers,
            virtual_daa: info.virtual_daa_score,
            latency_ms: started.elapsed().as_millis() as u64,
        }));
    }
    let mut selector = QualitySelector::new(5);
    let t = Duration::from_secs(0);
    let initial = selector.choose(t, &health);
    anyhow::ensure!(
        initial.eligible_count == 5 && initial.active_index.is_some(),
        "INITIAL_QUALITY_UNHEALTHY"
    );
    println!(
        "OP070_FIVE_REAL_PUBLIC_PNN_HEALTHY initial_rank={}",
        initial.active_index.unwrap() + 1
    );
    let mut previous = initial.active_index.unwrap();
    for lost in 1..=5 {
        // Only shut down THIS canary's independent client. Production
        // websocket, transactional source, and miner nodes are untouched.
        let _ = clients[previous].disconnect().await;
        health[previous] = None;
        let next = selector.choose(Duration::from_secs(lost as u64), &health);
        if lost < 5 {
            let current = next
                .active_index
                .ok_or_else(|| anyhow::anyhow!("NO_QUALIFIED_FAILOVER"))?;
            anyhow::ensure!(current != previous, "STUCK_ON_CLOSED_SOURCE");
            anyhow::ensure!(
                next.transition == Some((previous, current)),
                "FAILOVER_NOT_FENCED"
            );
            println!(
                "OP070_INDEPENDENT_PNN_CANARY_FAILOVER step={} next_rank={} remaining={}",
                lost,
                current + 1,
                next.eligible_count
            );
            previous = current;
        } else {
            anyhow::ensure!(
                next.active_index.is_none() && next.eligible_count == 0,
                "EXHAUSTED_POOL_DID_NOT_FAIL_CLOSED"
            );
            println!("OP070_INDEPENDENT_PNN_CANARY_FAIL_CLOSED_NO_SOURCE");
        }
    }
    println!("OP070_REAL_FIVE_SOURCE_FAILOVER_CANARY_PASS_NO_DB_NO_TELEGRAM_NO_MINERS");
    Ok(())
}
