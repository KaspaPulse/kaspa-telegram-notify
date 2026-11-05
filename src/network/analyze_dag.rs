use crate::domain::errors::AppError;
use crate::domain::models::BlockData;
use crate::infrastructure::database::postgres_adapter::PostgresRepository;
use crate::infrastructure::node::kaspa_adapter::KaspaRpcAdapter;
use kaspa_hashes::Hash;
use kaspa_rpc_core::api::rpc::RpcApi;
use std::str::FromStr;

use std::collections::HashSet;
use std::sync::Arc;

// The UTXO's DAA score is an approximate acceptance point; allow a small
// ancestry cushion but never recurse into arbitrarily old history.
fn dag_block_in_reward_window(block_daa: u64, reward_daa: u64) -> bool {
    block_daa >= reward_daa.saturating_sub(120)
}

fn pruned_pnn_header(message: &str) -> bool {
    message.to_ascii_lowercase().contains("cannot find header")
}

// Public REST proof is independently verified against the Coinbase wallet script
// and the mined Merge Set Blue block. Its global semaphore and 30s cooldown
// prevent historic recovery from saturating live subscription processing.
async fn bounded_public_archive_coinbase(
    txid: &str,
    wallet: &str,
) -> Result<(String, Vec<String>, String, String, u64), AppError> {
    let proof = tokio::time::timeout(
        std::time::Duration::from_secs(35),
        crate::network::public_api_historical::historical_coinbase_proof(txid, wallet),
    )
    .await
    .map_err(|_| AppError::NodeError("Historical public proof timed out; retry deferred".into()))?
    .map_err(|error| {
        AppError::NodeError(format!(
            "Historical public proof unavailable; retry deferred: {error}"
        ))
    })?;
    match proof {
        Some(proof) => Ok((
            proof.accepting_hash,
            proof.mined_hashes,
            proof.nonce,
            proof.worker,
            proof.block_time_ms,
        )),
        None => Err(AppError::NodeError(
            "No complete historical public proof; retry deferred".into(),
        )),
    }
}

pub struct AnalyzeDagUseCase {
    pub node: Arc<KaspaRpcAdapter>,
    pub db: Option<Arc<PostgresRepository>>,
}

impl AnalyzeDagUseCase {
    pub fn new(node: Arc<KaspaRpcAdapter>) -> Self {
        Self { node, db: None }
    }
    pub fn with_db(node: Arc<KaspaRpcAdapter>, db: Arc<PostgresRepository>) -> Self {
        Self { node, db: Some(db) }
    }

    pub async fn get_pruning_block(&self, hash: &str) -> Option<BlockData> {
        self.node.get_block(hash).await.ok()
    }

    // 🚀 RESTORED ORIGINAL DETAILS ALGORITHM
    pub async fn execute(
        &self,
        f_tx: &str,
        w_cl: &str,
        daa_score: u64,
        is_coinbase: bool,
    ) -> Result<(String, Vec<String>, String, String, u64), AppError> {
        self.execute_with_archive_recovery(f_tx, w_cl, daa_score, is_coinbase, false)
            .await
    }

    /// An already persisted proof backlog uses the bounded, external archival
    /// route; fresh rewards keep the existing PNN-first path unchanged.
    pub async fn execute_with_archive_recovery(
        &self,
        f_tx: &str,
        w_cl: &str,
        daa_score: u64,
        is_coinbase: bool,
        recovered_proof_backlog: bool,
    ) -> Result<(String, Vec<String>, String, String, u64), AppError> {
        if recovered_proof_backlog && is_coinbase {
            return bounded_public_archive_coinbase(f_tx, w_cl).await;
        }
        let mut acc_block_hash = String::new();
        let mut actual_mined_blocks: Vec<String> = Vec::new();
        let mut extracted_nonce = String::new();
        let mut extracted_worker = String::new();
        let mut block_time_ms: u64 = 0;
        let mut blue_block_fetch_errors: usize = 0;

        // Bypassing abstract traits to guarantee raw access to verbose_data for details
        let rpc_cl = self.node.client.clone();

        // The old implementation expanded *every* parent at every level from all
        // virtual tips. On a high-throughput BlockDAG, even a few hundred rounds
        // cause unbounded fanout and RPC saturation.
        //
        // Follow the GHOSTDAG selected-parent chain using the node-provided
        // selected_parent_hash, never assuming parents_by_level[0][0] is selected.
        // Only a confirmed transaction ID match can identify an acceptance block.
        // Fast, verified proof from the canonical PNN virtual-selected-parent
        // chain index (accepted transaction ID -> accepting chain block hash).
        // When missing, retain the existing bounded DAG fallback.
        let indexed = if let Some(db) = self.db.as_ref() {
            db.get_pending_accepting_hash(w_cl, f_tx).await?
        } else {
            None
        };
        // Fall back to the public REST indexer only when the durable PNN-VSPC
        // proof is missing. Its hash is just an acceptance candidate: mined
        // block metadata still requires an independent PNN BlockDAG lookup.
        let rest_acceptance = if indexed.is_none() && is_coinbase {
            crate::network::public_api_acceptance::acceptance_hash(f_tx).await
        } else {
            None
        };
        if let Some(hash) = indexed.or(rest_acceptance) {
            let parsed = Hash::from_str(&hash).map_err(|_| {
                AppError::NodeError("Invalid persisted acceptance proof hash".into())
            })?;
            acc_block_hash = parsed.to_string();
        } else {
            let info = rpc_cl.get_block_dag_info().await.map_err(|e| {
                AppError::NodeError(format!("DAG tip lookup failed for tx {}: {}", f_tx, e))
            })?;
            let mut current_hash = info.sink;
            let mut visited = HashSet::new();
            let mut checked = 0_usize;
            const MAX_CHAIN_BLOCKS: usize = 512;

            while checked < MAX_CHAIN_BLOCKS && visited.insert(current_hash) {
                let block = rpc_cl.get_block(current_hash, true).await.map_err(|e| {
                    AppError::NodeError(format!(
                        "Selected-parent DAG block fetch failed for tx {} after {} blocks: {}",
                        f_tx, checked, e
                    ))
                })?;
                checked += 1;

                // Once the selected chain has passed the accepted UTXO's DAA window,
                // an older block cannot be a valid acceptance block for this payout.
                if !dag_block_in_reward_window(block.header.daa_score, daa_score) {
                    break;
                }

                if block.transactions.iter().any(|tx| {
                    tx.verbose_data
                        .as_ref()
                        .is_some_and(|details| details.transaction_id.to_string() == f_tx)
                }) {
                    acc_block_hash = current_hash.to_string();
                    block_time_ms = block.header.timestamp;
                    break;
                }

                let Some(verbose) = block.verbose_data.as_ref() else {
                    return Err(AppError::NodeError(format!(
                        "Selected-parent metadata unavailable for tx {}",
                        f_tx
                    )));
                };
                if verbose.selected_parent_hash == Hash::default() {
                    break;
                }
                current_hash = verbose.selected_parent_hash;
            }

            if acc_block_hash.is_empty() {
                return Err(AppError::NodeError(format!(
                    "Acceptance transaction {} not found on bounded selected-parent chain (checked {} blocks); no fabricated mined-block details",
                    f_tx, checked
                )));
            }
        }
        if is_coinbase && !acc_block_hash.is_empty() {
            let acc_hash_obj = acc_block_hash.parse::<Hash>().map_err(|e| {
                AppError::NodeError(format!(
                    "Acceptance block hash parse failed. acc_block_hash={} tx={}: {}",
                    acc_block_hash, f_tx, e
                ))
            })?;
            {
                let full_acc_block = match rpc_cl.get_block(acc_hash_obj, true).await {
                    Ok(block) => block,
                    Err(e) => {
                        // A public pruned PNN node may know acceptance via the
                        // indexer but no longer retain that historical header.
                        // In that one case use independently verified, rate-limited
                        // REST proof. Never guess a mined block or send an alert.
                        if pruned_pnn_header(&e.to_string()) {
                            return bounded_public_archive_coinbase(f_tx, w_cl).await;
                        }
                        return Err(AppError::NodeError(format!(
                            "Acceptance block fetch failed while extracting mined block details. acc_block_hash={} tx={}: {}",
                            acc_block_hash, f_tx, e
                        )));
                    }
                };
                {
                    let mut user_script_bytes: Vec<u8> = Vec::new();
                    if let Some(tx0) = full_acc_block.transactions.first() {
                        for out in &tx0.outputs {
                            if let Some(ov) = &out.verbose_data
                                && ov.script_public_key_address.to_string() == w_cl
                            {
                                user_script_bytes = out.script_public_key.script().to_vec();
                                break;
                            }
                        }
                    }
                    if !user_script_bytes.is_empty()
                        && let Some(verbose) = &full_acc_block.verbose_data
                    {
                        for blue_hash in &verbose.merge_set_blues_hashes {
                            let blue_block = match rpc_cl.get_block(*blue_hash, true).await {
                                Ok(block) => block,
                                Err(e) => {
                                    blue_block_fetch_errors += 1;

                                    tracing::warn!(
                                        "[DAG ANALYSIS] Failed to fetch blue block while extracting mined block details. blue_hash={} tx={}: {}",
                                        blue_hash,
                                        f_tx,
                                        e
                                    );

                                    continue;
                                }
                            };
                            {
                                if let Some(m_tx0) = blue_block.transactions.first() {
                                    // 🔍 THE REAL DETAILS: Searching for user bytes in the payload
                                    if let Some(pos) = m_tx0
                                        .payload
                                        .windows(user_script_bytes.len())
                                        .position(|w| w == user_script_bytes.as_slice())
                                    {
                                        actual_mined_blocks.push(blue_hash.to_string());
                                        block_time_ms = blue_block.header.timestamp;
                                        if extracted_nonce.is_empty() {
                                            extracted_nonce = blue_block.header.nonce.to_string();
                                            let extra_data =
                                                &m_tx0.payload[pos + user_script_bytes.len()..];
                                            // 🔍 Extracting Worker ASCII
                                            extracted_worker = extra_data
                                                .iter()
                                                .filter(|&&c| (32..=126).contains(&c))
                                                .map(|&c| c as char)
                                                .collect();
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
        }
        if is_coinbase
            && !acc_block_hash.is_empty()
            && actual_mined_blocks.is_empty()
            && blue_block_fetch_errors > 0
        {
            return Err(AppError::NodeError(format!(
                "Blue block fetch failed during mined block extraction. acc_block_hash={} tx={} failed_blue_blocks={}",
                acc_block_hash, f_tx, blue_block_fetch_errors
            )));
        }
        // Dependency Injection: Connection is shared, do not disconnect here.
        Ok((
            acc_block_hash,
            actual_mined_blocks,
            extracted_nonce,
            extracted_worker,
            block_time_ms,
        ))
    }
}

#[cfg(test)]
mod bounded_dag_tests {
    use super::{dag_block_in_reward_window, pruned_pnn_header};

    #[test]
    fn only_pruned_header_errors_trigger_archive_route() {
        assert!(pruned_pnn_header(
            "RPC remote error -> cannot find header deadbeef"
        ));
        assert!(pruned_pnn_header("CANNOT FIND HEADER from pruned node"));
        assert!(!pruned_pnn_header("deadline has elapsed"));
        assert!(!pruned_pnn_header("permission denied"));
        assert!(!pruned_pnn_header("invalid acceptance hash"));
    }

    #[test]
    fn does_not_scan_dag_below_reward_ancestry_window() {
        assert!(!dag_block_in_reward_window(1_000, 1_121));
        assert!(dag_block_in_reward_window(1_001, 1_121));
        assert!(dag_block_in_reward_window(1_121, 1_121));
    }

    #[test]
    fn daa_window_never_underflows() {
        assert!(dag_block_in_reward_window(0, 0));
        assert!(dag_block_in_reward_window(0, 100));
        assert!(!dag_block_in_reward_window(0, 121));
    }
}
