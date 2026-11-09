//! OP075 independent public PNN live test: all connections belong to this
//! canary. No PostgreSQL, Telegram, mining device, production wallet or
//! production WebSocket touched. Simulates a dead INACTIVE source zero.
use anyhow::{Context, ensure};
use kaspa_consensus_core::network::NetworkId;
use kaspa_pulse::infrastructure::node::kaspa_adapter::KaspaRpcAdapter;
use kaspa_pulse::infrastructure::node::public_pnn_rotation::{
    RotationRequest, discover_public_spare, qualified_coeval_spare,
};
use kaspa_pulse::infrastructure::node::source_registry::SourceRegistry;
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_wrpc_client::{KaspaRpcClient, Resolver, WrpcEncoding};
use std::{collections::HashSet, str::FromStr, sync::Arc, time::Duration};

async fn indexed_public_source(node: &KaspaRpcAdapter) -> bool {
    matches!(
        tokio::time::timeout(Duration::from_secs(9), async {
            let info = node.client.get_server_info().await?;
            if info.network_id.to_string() != "mainnet"
                || !info.is_synced
                || !info.has_utxo_index
                || info.virtual_daa_score == 0
            {
                return Ok::<bool, kaspa_rpc_core::RpcError>(false);
            }
            let peers = node.client.get_connected_peer_info().await?.peer_info.len();
            if peers == 0 {
                return Ok(false);
            }
            let dag = node.client.get_block_dag_info().await?;
            node.client
                .get_virtual_chain_from_block(dag.sink, true, Some(1))
                .await?;
            Ok(true)
        })
        .await,
        Ok(Ok(true))
    )
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let _ = rustls::crypto::ring::default_provider().install_default();
    let network = NetworkId::from_str("mainnet")?;
    let resolver = Resolver::default();
    let mut urls = HashSet::new();
    let mut nodes = Vec::<Arc<KaspaRpcAdapter>>::new();
    let deadline = tokio::time::Instant::now() + Duration::from_secs(120);
    for _ in 0..70 {
        if nodes.len() == 5 || tokio::time::Instant::now() > deadline {
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
        let client = Arc::new(KaspaRpcClient::new(
            WrpcEncoding::Borsh,
            Some(&url),
            None,
            Some(network),
            None,
        )?);
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
            let _ = client.disconnect().await;
            continue;
        }
        let node = Arc::new(KaspaRpcAdapter::new_pinned(client, url));
        if indexed_public_source(&node).await {
            nodes.push(node);
        } else {
            let _ = node.client.disconnect().await;
        }
    }
    ensure!(
        nodes.len() == 5,
        "FIVE_DISTINCT_PUBLIC_VSPC_SOURCES_NOT_AVAILABLE"
    );
    let registry = Arc::new(SourceRegistry::new(nodes.clone()));
    let epoch = registry
        .select(Some(2))
        .await
        .map_err(|e| anyhow::anyhow!(e))?;
    let selected = registry.selected().await.context("writer qualified")?;
    let expected_old = nodes[0].clone();
    let request = RotationRequest {
        target: 0,
        generation: epoch,
        expected_old: expected_old.clone(),
        excluded_urls: urls,
    };
    // Exactly the same bounded public source discovery path as the
    // OP075 production candidate; no forced mining-service fault.
    let candidate = tokio::time::timeout(Duration::from_secs(115), discover_public_spare(request))
        .await
        .context("public spare discovery timed out")?
        .context("no VSPC-capable spare publicly discoverable")?;
    ensure!(
        candidate.target == 0 && candidate.generation == epoch,
        "spare capture invalid"
    );
    // The canary disconnects only its OWN idle source0. The selected rank3
    // and VSPC indexed source remain open and untouched.
    expected_old.client.disconnect().await?;
    ensure!(
        !expected_old.client.is_connected(),
        "idle canary source remained connected"
    );
    ensure!(
        qualified_coeval_spare(&nodes, &candidate.new_node).await,
        "spare DAA/VSPC coeval network quorum failed closed"
    );
    let hold = registry
        .fence(&selected)
        .await
        .context("VSPC old-epoch read fence")?;
    let for_task = registry.clone();
    let swap = tokio::spawn(async move {
        for_task
            .replace_inactive(
                candidate.generation,
                candidate.target,
                &candidate.expected_old,
                candidate.new_node.clone(),
                true,
            )
            .await
    });
    tokio::task::yield_now().await;
    ensure!(
        !swap.is_finished(),
        "replacement violated in-flight VSPC fence"
    );
    drop(hold);
    let outcome = tokio::time::timeout(Duration::from_secs(5), swap)
        .await
        .context("waiting for VSPC-fenced swap")?
        .context("source replacement join task failed")?;
    let new_epoch = outcome.map_err(|e| anyhow::anyhow!(e))?;
    ensure!(new_epoch == epoch + 1, "source epoch did not advance once");
    ensure!(
        registry.fence(&selected).await.is_none(),
        "stale old-epoch VSPC read was incorrectly accepted"
    );
    let (seen_epoch, writer, pool) = registry.snapshot().await;
    ensure!(
        seen_epoch == new_epoch && writer == Some(2),
        "selected reward writer changed"
    );
    ensure!(
        Arc::ptr_eq(&pool[2], &nodes[2]),
        "selected wallet source was replaced"
    );
    ensure!(
        !Arc::ptr_eq(&pool[0], &nodes[0]),
        "dead inactive slot was not replaced"
    );
    ensure!(
        pool[0].current_or_pinned_url().is_some(),
        "stable pinned URL disappeared"
    );
    println!("OP075_SIX_PUBLIC_PNN_VSPC_READONLY_QUALIFICATION_PASS");
    println!("OP075_SELECTED_WALLET_SOURCE_PRESERVED_INDEX_3_PASS");
    println!("OP075_DURABLE_VSPC_FENCE_REJECTS_STALE_GENERATION_PASS");
    println!("OP075_INACTIVE_SOURCE_ZERO_SWAP_EPOCH_ATOMIC_PASS");
    println!("OP075_NO_PRODUCTION_SOCKET_OR_DB_OR_TELEGRAM_OR_MINER_TOUCHED_PASS");
    for node in pool {
        let _ = tokio::time::timeout(Duration::from_secs(3), node.client.disconnect()).await;
    }
    Ok(())
}
