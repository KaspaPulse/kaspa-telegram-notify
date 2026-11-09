//! Small incremental accepted-transaction index against public mainnet PNN.
//! No full BlockDAG scans, no new OS scheduler, no unverified block hashes.
use crate::infrastructure::database::postgres_adapter::PostgresRepository;
use crate::infrastructure::node::kaspa_adapter::KaspaRpcAdapter;
use kaspa_hashes::Hash;
use kaspa_rpc_core::api::rpc::RpcApi;
use std::collections::{HashMap, HashSet};
use std::str::FromStr;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio_util::sync::CancellationToken;

fn valid_hash(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn apply_delta_memory(
    cache: &mut HashMap<String, (String, Instant)>,
    removed: &HashSet<String>,
    accepted: &[(String, String)],
    now: Instant,
) {
    cache.retain(|_, (hash, seen)| {
        !removed.contains(hash) && now.duration_since(*seen) < Duration::from_secs(900)
    });
    for (txid, hash) in accepted {
        if valid_hash(txid) && valid_hash(hash) {
            cache.insert(txid.to_string(), (hash.to_string(), now));
        }
    }
}
/// Persistent checkpoint and matched Coinbase metadata are written in one DB transaction.
/// A one-minute rewind is used on restart so records that arrived immediately
/// after a VSPC delta are not lost across a WSL interruption.
async fn selected_pnn(
    primary: &Arc<KaspaRpcAdapter>,
    registry: &Option<Arc<crate::infrastructure::node::source_registry::SourceRegistry>>,
) -> Option<(
    Arc<KaspaRpcAdapter>,
    Option<crate::infrastructure::node::source_registry::SelectedPnn>,
)> {
    match registry {
        Some(registry) => {
            let selected = registry.selected().await?;
            Some((selected.source.clone(), Some(selected)))
        }
        None => Some((primary.clone(), None)),
    }
}

pub async fn run_indexer(
    node: Arc<KaspaRpcAdapter>,
    pool: Option<Arc<crate::infrastructure::node::source_registry::SourceRegistry>>,
    db: Arc<PostgresRepository>,
    cancellation: CancellationToken,
) {
    let mut cache = HashMap::<String, (String, Instant)>::new();
    // A public PNN may be unavailable when WSL boots. This is a persistent,
    // cancellation-aware in-process retry, not a one-shot startup exit.
    let saved_rewind = loop {
        if cancellation.is_cancelled() {
            return;
        }
        match db.get_setting("KASPA_PNN_VSPC_REWIND_CURSOR", "").await {
            Ok(value) => break value,
            Err(error) => tracing::warn!(
                "[PNN VSPC] Checkpoint database unavailable; retaining durable progress: {}",
                error
            ),
        }
        tokio::select! {
            _ = cancellation.cancelled()=>return,
            _ = tokio::time::sleep(Duration::from_secs(5))=>{}
        }
    };
    let mut cursor = if valid_hash(&saved_rewind) {
        match Hash::from_str(&saved_rewind) {
            Ok(hash) => hash,
            Err(error) => {
                tracing::error!(
                    "[PNN VSPC] Corrupt saved cursor; refusing to skip history: {}",
                    error
                );
                return;
            }
        }
    } else {
        loop {
            if cancellation.is_cancelled() {
                return;
            }
            let Some((selected_node, _selection)) = selected_pnn(&node, &pool).await else {
                tokio::select! {
                    _ = cancellation.cancelled()=>return,
                    _ = tokio::time::sleep(Duration::from_secs(2))=>{}
                }
                continue;
            };
            match tokio::time::timeout(
                Duration::from_secs(12),
                selected_node.client.get_block_dag_info(),
            )
            .await
            {
                Ok(Ok(info)) => {
                    tracing::info!(
                        "[PNN VSPC] Starting index at current sink; never fabricating historical proof."
                    );
                    break info.sink;
                }
                _ => tracing::warn!("[PNN VSPC] Public PNN not ready; startup seed will retry."),
            }
            tokio::select! {
                _ = cancellation.cancelled()=>return,
                _ = tokio::time::sleep(Duration::from_secs(5))=>{}
            }
        }
    };
    let mut interval = tokio::time::interval(Duration::from_secs(5));
    interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    let mut last_rewind = Instant::now();
    let mut rewind_hash = cursor;
    loop {
        tokio::select! {
            _ = cancellation.cancelled()=>break,
            _ = interval.tick()=>{}
        }
        // OP067: acceptance indexer follows the EXACT healthy source
        // selected by the single-writer UTXO scanner. When no source is
        // eligible, hold the durable cursor instead of reading stale PNN.
        let Some((selected_node, captured_selection)) = selected_pnn(&node, &pool).await else {
            continue;
        };
        if !selected_node.client.is_connected() {
            continue;
        }
        let response = tokio::time::timeout(
            Duration::from_secs(12),
            selected_node
                .client
                .get_virtual_chain_from_block(cursor, true, Some(10)),
        )
        .await;
        let delta = match response {
            Ok(Ok(delta)) => delta,
            Ok(Err(error)) => {
                tracing::warn!(
                    "[PNN VSPC] RPC unavailable; checkpoint retained for retry: {}",
                    error
                );
                continue;
            }
            Err(_) => {
                tracing::warn!("[PNN VSPC] RPC timeout; checkpoint retained for retry.");
                continue;
            }
        };
        if delta
            .accepted_transaction_ids
            .iter()
            .map(|group| group.accepted_transaction_ids.len())
            .sum::<usize>()
            > 20_000
        {
            tracing::warn!("[PNN VSPC] Oversized response refused; cursor remains unchanged.");
            continue;
        }
        let added: HashSet<String> = delta
            .added_chain_block_hashes
            .iter()
            .map(ToString::to_string)
            .collect();
        let removed: HashSet<String> = delta
            .removed_chain_block_hashes
            .iter()
            .map(ToString::to_string)
            .collect();
        let mut accepted = Vec::<(String, String)>::new();
        let mut valid = true;
        for group in &delta.accepted_transaction_ids {
            let block = group.accepting_block_hash.to_string();
            if !added.contains(&block) || !valid_hash(&block) {
                valid = false;
                break;
            }
            for txid in &group.accepted_transaction_ids {
                accepted.push((txid.to_string(), block.clone()));
            }
        }
        if !valid {
            tracing::warn!(
                "[PNN VSPC] Non-canonical acceptance block in response; refusing delta."
            );
            continue;
        }
        let mut next_cursor = cursor;
        if let Some(last) = delta.added_chain_block_hashes.last() {
            next_cursor = *last;
        }
        if removed.contains(&next_cursor.to_string()) {
            tracing::warn!("[PNN VSPC] Cursor removed without replacement; fail-closed.");
            continue;
        }
        // Update cache only after the canonical response has passed validation.
        let now = Instant::now();
        let mut next_cache = cache.clone();
        apply_delta_memory(&mut next_cache, &removed, &accepted, now);
        // Rewind checkpoint advances only once per minute. This is not an OS task.
        let new_rewind = if last_rewind.elapsed() >= Duration::from_secs(60) {
            Some(rewind_hash.to_string())
        } else {
            None
        };
        let matches: Vec<(String, String)> = next_cache
            .iter()
            .map(|(txid, (block, _))| (txid.clone(), block.clone()))
            .collect();
        // Keep the source selection + generation stable through the
        // WHOLE DB checkpoint transaction. No other reader or source
        // replacement may invalidate an accepted VSPC delta mid-commit.
        let _generation_fence = if let (Some(registry), Some(captured)) =
            (pool.as_ref(), captured_selection.as_ref())
        {
            let Some(fence) = registry.fence(captured).await else {
                tracing::warn!(
                    "[PNN VSPC] Source generation changed during RPC; discarding response, checkpoint unchanged."
                );
                continue;
            };
            Some(fence)
        } else {
            None
        };
        if let Err(error) = db
            .apply_vspc_proof_delta(
                &matches,
                &removed.iter().cloned().collect::<Vec<_>>(),
                &next_cursor.to_string(),
                new_rewind.as_deref(),
            )
            .await
        {
            tracing::warn!(
                "[PNN VSPC] Atomic metadata/checkpoint update failed: {}",
                error
            );
            continue;
        }
        if new_rewind.is_some() {
            rewind_hash = cursor;
            last_rewind = Instant::now();
        }
        cursor = next_cursor;
        cache = next_cache;
    }
}
#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn accepted_tx_index_reorg_rolls_back_and_stays_bounded() {
        let mut cache = HashMap::new();
        let now = Instant::now();
        let txid = "1".repeat(64);
        let old = "a".repeat(64);
        let new = "b".repeat(64);
        apply_delta_memory(
            &mut cache,
            &HashSet::new(),
            &[(txid.clone(), old.clone())],
            now,
        );
        assert_eq!(cache.get(&txid).unwrap().0, old);
        apply_delta_memory(
            &mut cache,
            &HashSet::from([old]),
            &[(txid.clone(), new.clone())],
            now,
        );
        assert_eq!(cache.get(&txid).unwrap().0, new);
        apply_delta_memory(&mut cache, &HashSet::from([new]), &[], now);
        assert!(!cache.contains_key(&txid));
        assert!(!valid_hash("not-a-real-hash"));
    }
}
