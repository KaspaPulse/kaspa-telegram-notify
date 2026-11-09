//! OP075: read-only discovery and simultaneous DAA/VSPC admission for
//! public PNN replacement. The ONE transactional wallet scanner is the
//! only caller allowed to publish a source generation through SourceRegistry.
use super::daa_reference::{DaaSample, find_corroborated_reference, within_verified_daa_window};
use super::kaspa_adapter::KaspaRpcAdapter;
use kaspa_consensus_core::network::NetworkId;
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_wrpc_client::{KaspaRpcClient, Resolver, WrpcEncoding};
use std::{collections::HashSet, str::FromStr, sync::Arc, time::Duration};
use tokio::time::Instant;

pub struct RotationRequest {
    pub target: usize,
    pub generation: u64,
    pub expected_old: Arc<KaspaRpcAdapter>,
    pub excluded_urls: HashSet<String>,
}
pub struct RotationCandidate {
    pub target: usize,
    pub generation: u64,
    pub expected_old: Arc<KaspaRpcAdapter>,
    pub new_node: Arc<KaspaRpcAdapter>,
}

pub fn should_search_for_spare(
    eligible: bool,
    selected: Option<usize>,
    target: usize,
    guarded_reconnect_attempts: u32,
    source_unhealthy_probes: u32,
    cooldown_elapsed: bool,
    discovery_running: bool,
) -> bool {
    !eligible
        && selected != Some(target)
        && guarded_reconnect_attempts >= 3
        && source_unhealthy_probes >= 20
        && cooldown_elapsed
        && !discovery_running
}

/// Run separately from the scanner's wallet-scan task. Never accesses
/// production Telegram/PG. Resolver failures keep the CURRENT pinned source
/// and do not count as proof the replacement is safe.
pub async fn discover_public_spare(request: RotationRequest) -> Option<RotationCandidate> {
    let network = NetworkId::from_str("mainnet").ok()?;
    let resolver = Resolver::default();
    let mut excluded = request.excluded_urls.clone();
    let stop = Instant::now() + Duration::from_secs(100);
    for _ in 0..32 {
        if Instant::now() >= stop {
            break;
        }
        let url = match tokio::time::timeout(
            Duration::from_secs(6),
            resolver.get_url(WrpcEncoding::Borsh, network),
        )
        .await
        {
            Ok(Ok(url)) if excluded.insert(url.clone()) => url,
            _ => continue,
        };
        let client =
            match KaspaRpcClient::new(WrpcEncoding::Borsh, Some(&url), None, Some(network), None) {
                Ok(client) => Arc::new(client),
                Err(_) => continue,
            };
        let connected = matches!(
            tokio::time::timeout(
                Duration::from_secs(9),
                client.connect(Some(
                    kaspa_wrpc_client::client::ConnectOptions::blocking_fallback()
                )),
            )
            .await,
            Ok(Ok(_))
        );
        if !connected {
            let _ = tokio::time::timeout(Duration::from_secs(3), client.disconnect()).await;
            continue;
        }
        let proven = tokio::time::timeout(Duration::from_secs(11), async {
            let info = client.get_server_info().await?;
            if info.network_id.to_string() != "mainnet"
                || !info.is_synced
                || !info.has_utxo_index
                || info.virtual_daa_score == 0
            {
                return Ok::<bool, kaspa_rpc_core::RpcError>(false);
            }
            let peers = client.get_connected_peer_info().await?.peer_info.len();
            if peers == 0 {
                return Ok(false);
            }
            let dag = client.get_block_dag_info().await?;
            // A merely synced RPC is not enough for the durable acceptance
            // indexer. Prove actual VSPC read support before proposing swap.
            client
                .get_virtual_chain_from_block(dag.sink, true, Some(1))
                .await?;
            Ok(true)
        })
        .await;
        if matches!(proven, Ok(Ok(true))) {
            return Some(RotationCandidate {
                target: request.target,
                generation: request.generation,
                expected_old: request.expected_old,
                new_node: Arc::new(KaspaRpcAdapter::new_pinned(client, url)),
            });
        }
        let _ = tokio::time::timeout(Duration::from_secs(3), client.disconnect()).await;
    }
    None
}

async fn sample_public_daa(
    index: usize,
    node: Arc<KaspaRpcAdapter>,
    zero: Instant,
) -> Option<DaaSample> {
    if !node.client.is_connected() {
        return None;
    }
    let response = tokio::time::timeout(Duration::from_secs(4), async {
        let server = node.client.get_server_info().await?;
        let peers = node.client.get_connected_peer_info().await?.peer_info.len();
        Ok::<_, kaspa_rpc_core::RpcError>((server, peers))
    })
    .await
    .ok()?
    .ok()?;
    let (server, peers) = response;
    Some(DaaSample {
        source_id: index,
        observed_at_mono_ms: zero.elapsed().as_millis() as u64,
        daa: server.virtual_daa_score,
        mainnet: server.network_id.to_string() == "mainnet",
        synced: server.is_synced,
        indexed: server.has_utxo_index,
        peers,
    })
}

/// Resample ALL surviving public endpoints and candidate at the SAME time.
/// A hundred-second sequential discovery is NOT a valid 64-DAA comparison.
/// The independently corroborated tip needs >=3 distinct qualified URLs.
/// A single falsely high node cannot veto correctly synced peers.
pub async fn qualified_coeval_spare(
    pool: &[Arc<KaspaRpcAdapter>],
    spare: &Arc<KaspaRpcAdapter>,
) -> bool {
    if pool.len() != 5 || !spare.client.is_connected() {
        return false;
    }
    let Some(url) = spare.current_or_pinned_url() else {
        return false;
    };
    let mut urls = HashSet::new();
    for node in pool {
        let Some(endpoint) = node.current_or_pinned_url() else {
            return false;
        };
        if !urls.insert(endpoint) {
            return false;
        }
    }
    if !urls.insert(url) {
        return false;
    }
    let zero = Instant::now();
    let mut work = tokio::task::JoinSet::new();
    for (index, node) in pool.iter().chain(std::iter::once(spare)).enumerate() {
        let node = node.clone();
        work.spawn(sample_public_daa(index, node, zero));
    }
    let mut samples = Vec::with_capacity(6);
    let _ = tokio::time::timeout(Duration::from_secs(7), async {
        while let Some(done) = work.join_next().await {
            if let Ok(Some(sample)) = done {
                samples.push(sample);
            }
        }
    })
    .await;
    work.abort_all();
    let Some(candidate) = samples.iter().copied().find(|s| s.source_id == 5) else {
        return false;
    };
    match find_corroborated_reference(&samples) {
        Ok(reference) => within_verified_daa_window(&reference, candidate),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::should_search_for_spare;
    #[test]
    fn requires_repeated_reconnect_failures_and_only_inactive_slot() {
        assert!(!should_search_for_spare(
            false,
            Some(0),
            0,
            9,
            99,
            true,
            false
        ));
        assert!(!should_search_for_spare(
            false,
            Some(0),
            1,
            2,
            99,
            true,
            false
        ));
        assert!(!should_search_for_spare(
            false,
            Some(0),
            1,
            4,
            19,
            true,
            false
        ));
        assert!(!should_search_for_spare(
            false,
            Some(0),
            1,
            4,
            20,
            false,
            false
        ));
        assert!(!should_search_for_spare(
            false,
            Some(0),
            1,
            4,
            20,
            true,
            true
        ));
        assert!(!should_search_for_spare(
            true,
            Some(0),
            1,
            8,
            99,
            true,
            false
        ));
        assert!(should_search_for_spare(
            false,
            Some(0),
            1,
            3,
            20,
            true,
            false
        ));
        assert!(should_search_for_spare(false, None, 0, 3, 20, true, false));
    }
}
