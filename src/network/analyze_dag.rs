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

/// OP084: A VSPC acceptance block is NOT necessarily the block that
/// contains the accepted Coinbase. Never attribute its OWN Coinbase to an
/// unrelated accepted transaction. Such a case needs independently verified
/// accepted-TX -> containing-block binding through the bounded REST path.
fn pnn_coinbase_transaction_matches(requested: &str, actual: Option<&str>) -> bool {
    requested.len() == 64
        && requested.bytes().all(|b| b.is_ascii_hexdigit())
        && actual.is_some_and(|id| id.len() == 64 && id.eq_ignore_ascii_case(requested))
}

/// Bind a canonical Coinbase miner script AND version to one exact indexed
/// accepted Coinbase payout. No scanning arbitrary payload offsets.
fn pnn_bound_miner_extra<'a>(
    payload: &'a [u8],
    payout_script: &[u8],
    payout_version: u16,
    payout_amount: u64,
) -> Option<&'a [u8]> {
    if payout_amount == 0 || payload.get(16..18)? != payout_version.to_le_bytes() {
        return None;
    }
    crate::network::coinbase_payload::canonical_miner_extra(payload, payout_script)
}

fn pnn_printable_worker(extra: &[u8]) -> Option<String> {
    let worker: String = extra
        .iter()
        .copied()
        .filter(|b| (32..=126).contains(b))
        .take(90)
        .map(char::from)
        .collect();
    if worker.is_empty() {
        None
    } else {
        Some(worker)
    }
}

/// A self-red payout is ONLY the unique LAST output and ONLY after every
/// merge-set blue Coinbase output was bound to its exact mined blue block.
fn pnn_self_red_shape(
    blue_count: usize,
    red_count: usize,
    output_count: usize,
    wallet_output_positions: &[usize],
    blue_outputs_verified: bool,
) -> bool {
    blue_outputs_verified
        && blue_count > 0
        && blue_count <= 32
        && red_count > 0
        && red_count <= 32
        && output_count == blue_count.saturating_add(1)
        && wallet_output_positions == [blue_count]
}

/// Keep primary fresh-wallet scan bounded exactly as before, while the
/// already-DURABLE newer proof backlog receives enough time to survive the
/// existing FIFO gate, cooldown, and multi-block verified REST requests.
/// The wallet task's established 100-second ceiling still caps this route.
fn archival_proof_timeout(already_durable_coinbase: bool) -> std::time::Duration {
    std::time::Duration::from_secs(if already_durable_coinbase { 85 } else { 35 })
}

/// Records a real PNN acceptance/containing split even when the surrounding
/// wallet task times out and drops this future. No TXIDs or wallet addresses.
struct PnnSplitRestLatency {
    started: std::time::Instant,
    completed: Option<bool>,
}
impl PnnSplitRestLatency {
    fn new() -> Self {
        Self {
            started: std::time::Instant::now(),
            completed: None,
        }
    }
}
impl Drop for PnnSplitRestLatency {
    fn drop(&mut self) {
        crate::infrastructure::metrics::observe_pnn_split_rest_latency(
            self.started.elapsed().as_millis().min(u64::MAX as u128) as u64,
            self.completed,
        );
    }
}

async fn bounded_pnn_split_archive_coinbase(
    txid: &str,
    wallet: &str,
) -> Result<(String, Vec<String>, String, String, u64), AppError> {
    let mut latency = PnnSplitRestLatency::new();
    let result = bounded_public_archive_coinbase(txid, wallet, false).await;
    // Full proof is required for Result::Ok; error or task cancellation
    // never enables reward delivery or historical replay.
    latency.completed = Some(result.is_ok());
    result
}

// Public REST proof is independently verified against the Coinbase wallet script
// and the mined Merge Set Blue block. Its global semaphore and 30s cooldown
// prevent historic recovery from saturating live subscription processing.
async fn bounded_public_archive_coinbase(
    txid: &str,
    wallet: &str,
    already_durable_coinbase: bool,
) -> Result<(String, Vec<String>, String, String, u64), AppError> {
    let proof = tokio::time::timeout(
        archival_proof_timeout(already_durable_coinbase),
        crate::network::public_api_historical::historical_coinbase_proof(txid, wallet),
    )
    .await
    .map_err(|_| AppError::NodeError("Historical public proof timed out; retry deferred".into()))?
    .map_err(|error| {
        crate::infrastructure::metrics::record_archive_fetch_error_classified(&error);
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
            return bounded_public_archive_coinbase(f_tx, w_cl, true).await;
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
            let acc_hash_obj = acc_block_hash.parse::<Hash>().map_err(|_| {
                AppError::NodeError("Invalid PNN accepting block hash; retry deferred".into())
            })?;
            let full_acc_block = match rpc_cl.get_block(acc_hash_obj, true).await {
                Ok(block) => block,
                Err(error) => {
                    if pruned_pnn_header(&error.to_string()) {
                        return bounded_public_archive_coinbase(f_tx, w_cl, false).await;
                    }
                    return Err(AppError::NodeError(
                        "PNN accepting block unavailable; retry deferred".into(),
                    ));
                }
            };
            // The first Coinbase in the ACCEPTING block can belong to a
            // different miner's CONTAINING block. Never claim its blue or red
            // rewards unless both the canonical block and TXID match. An
            // independent REST proof handles the accepted/containing split.
            let same_block = full_acc_block
                .verbose_data
                .as_ref()
                .is_some_and(|v| v.hash == acc_hash_obj);
            let observed_txid = full_acc_block
                .transactions
                .first()
                .and_then(|tx| tx.verbose_data.as_ref())
                .map(|info| info.transaction_id.to_string());
            if !same_block || !pnn_coinbase_transaction_matches(f_tx, observed_txid.as_deref()) {
                crate::infrastructure::metrics::record_pnn_coinbase_route(
                    crate::infrastructure::metrics::PnnCoinbaseRoute::AcceptingContainingSplitFallback,
                );
                return bounded_pnn_split_archive_coinbase(f_tx, w_cl).await;
            }
            let Some(tx0) = full_acc_block.transactions.first() else {
                return bounded_public_archive_coinbase(f_tx, w_cl, false).await;
            };
            let Some(verbose) = full_acc_block.verbose_data.as_ref() else {
                return bounded_public_archive_coinbase(f_tx, w_cl, false).await;
            };
            let blues = &verbose.merge_set_blues_hashes;
            if blues.is_empty() || blues.len() > 32 {
                return bounded_public_archive_coinbase(f_tx, w_cl, false).await;
            }
            let mut every_blue_payout_bound = true;
            // A single wallet can appear in several actual blue outputs,
            // but self-red attribution requires exactly ONE final wallet
            // output. Keep the complete ordered evidence independently.
            let wallet_positions: Vec<usize> = tx0
                .outputs
                .iter()
                .enumerate()
                .filter_map(|(index, output)| {
                    output
                        .verbose_data
                        .as_ref()
                        .is_some_and(|data| data.script_public_key_address.to_string() == w_cl)
                        .then_some(index)
                })
                .collect();
            for (blue_index, blue_hash) in blues.iter().enumerate() {
                let blue_block = match rpc_cl.get_block(*blue_hash, true).await {
                    Ok(block) => block,
                    Err(_) => {
                        blue_block_fetch_errors += 1;
                        every_blue_payout_bound = false;
                        continue;
                    }
                };
                if !blue_block
                    .verbose_data
                    .as_ref()
                    .is_some_and(|data| data.hash == *blue_hash)
                {
                    blue_block_fetch_errors += 1;
                    every_blue_payout_bound = false;
                    continue;
                }
                let Some(payout) = tx0.outputs.get(blue_index) else {
                    every_blue_payout_bound = false;
                    continue;
                };
                let Some(blue_coinbase) = blue_block.transactions.first() else {
                    every_blue_payout_bound = false;
                    continue;
                };
                let Some(extra) = pnn_bound_miner_extra(
                    &blue_coinbase.payload,
                    payout.script_public_key.script(),
                    payout.script_public_key.version(),
                    payout.value,
                ) else {
                    every_blue_payout_bound = false;
                    continue;
                };
                // The blue reward is credited to a wallet only when that
                // INDEXED output is actually the wallet's own payout.
                if !wallet_positions.contains(&blue_index) {
                    continue;
                }
                let Some(worker) = pnn_printable_worker(extra) else {
                    continue;
                };
                if blue_block.header.timestamp == 0 {
                    continue;
                }
                actual_mined_blocks.push(blue_hash.to_string());
                if extracted_nonce.is_empty() {
                    block_time_ms = blue_block.header.timestamp;
                    extracted_nonce = blue_block.header.nonce.to_string();
                    extracted_worker = worker;
                }
            }
            // An incomplete blue-to-payout map cannot authorize a native
            // PNN proof, even if one individual miner script matched.
            // Independently verify through the bounded accepted REST path.
            if !every_blue_payout_bound {
                crate::infrastructure::metrics::record_pnn_coinbase_route(
                    crate::infrastructure::metrics::PnnCoinbaseRoute::BlueMappingFallback,
                );
                return bounded_public_archive_coinbase(f_tx, w_cl, false).await;
            }
            let mut native_self_red = false;
            if actual_mined_blocks.is_empty()
                && blue_block_fetch_errors == 0
                && pnn_self_red_shape(
                    blues.len(),
                    verbose.merge_set_reds_hashes.len(),
                    tx0.outputs.len(),
                    &wallet_positions,
                    every_blue_payout_bound,
                )
                && let Some(last) = tx0.outputs.last()
                && let Some(extra) = pnn_bound_miner_extra(
                    &tx0.payload,
                    last.script_public_key.script(),
                    last.script_public_key.version(),
                    last.value,
                )
                && let Some(worker) = pnn_printable_worker(extra)
                && full_acc_block.header.timestamp > 0
            {
                // The mined self-red block is the already verified
                // containing==accepting block. Never invent a red hash.
                actual_mined_blocks.push(acc_hash_obj.to_string());
                extracted_nonce = full_acc_block.header.nonce.to_string();
                extracted_worker = worker;
                block_time_ms = full_acc_block.header.timestamp;
                native_self_red = true;
            }
            if actual_mined_blocks.is_empty() {
                crate::infrastructure::metrics::record_pnn_coinbase_route(
                    crate::infrastructure::metrics::PnnCoinbaseRoute::NativeNoMinerFallback,
                );
                // The bound, full REST accepted->containing proof covers
                // other valid self-red payouts and pruned PNN data.
                // No Telegram event can be created on partial PNN proof.
                return bounded_public_archive_coinbase(f_tx, w_cl, false).await;
            }
            crate::infrastructure::metrics::record_pnn_coinbase_route(if native_self_red {
                crate::infrastructure::metrics::PnnCoinbaseRoute::NativeSelfRedVerified
            } else {
                crate::infrastructure::metrics::PnnCoinbaseRoute::NativeBlueVerified
            });
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

#[cfg(test)]
mod op084_pnn_acceptance_and_miner_proof_contract_tests {
    use super::*;
    fn payload(script: &[u8], version: u16, extra: &[u8]) -> Vec<u8> {
        let mut result = vec![0u8; 16];
        result.extend_from_slice(&version.to_le_bytes());
        result.push(script.len() as u8);
        result.extend_from_slice(script);
        result.extend_from_slice(extra);
        result
    }

    #[test]
    fn refuses_accepting_blocks_own_coinbase_if_txid_is_not_requested() {
        let tx = "a".repeat(64);
        assert!(pnn_coinbase_transaction_matches(
            &tx,
            Some(&tx.to_uppercase())
        ));
        assert!(!pnn_coinbase_transaction_matches(
            &tx,
            Some(&"b".repeat(64))
        ));
        assert!(!pnn_coinbase_transaction_matches(&tx, None));
        assert!(!pnn_coinbase_transaction_matches("not-a-tx", Some(&tx)));
        assert!(!pnn_coinbase_transaction_matches(&tx, Some("not-a-tx")));
    }

    #[test]
    fn strictly_binds_blue_coinbase_script_version_amount_and_worker() {
        let script = [0xab, 0xcd, 0x12];
        let bytes = payload(&script, 0, b"WorkerA");
        let extra = pnn_bound_miner_extra(&bytes, &script, 0, 10).unwrap();
        assert_eq!(pnn_printable_worker(extra).as_deref(), Some("WorkerA"));
        assert!(pnn_bound_miner_extra(&bytes, &script, 1, 10).is_none());
        assert!(pnn_bound_miner_extra(&bytes, &[0xff], 0, 10).is_none());
        assert!(pnn_bound_miner_extra(&bytes, &script, 0, 0).is_none());
        assert!(pnn_bound_miner_extra(&bytes[..18], &script, 0, 10).is_none());
        assert!(pnn_printable_worker(b"\x01\x02").is_none());
    }

    #[test]
    fn self_red_requires_complete_blue_mapping_and_unique_last_output() {
        assert!(pnn_self_red_shape(4, 3, 5, &[4], true));
        assert!(!pnn_self_red_shape(4, 0, 5, &[4], true));
        assert!(!pnn_self_red_shape(4, 3, 5, &[0, 4], true));
        assert!(!pnn_self_red_shape(4, 3, 4, &[3], true));
        assert!(!pnn_self_red_shape(4, 3, 6, &[5], true));
        assert!(!pnn_self_red_shape(4, 3, 5, &[4], false));
        assert!(!pnn_self_red_shape(0, 3, 1, &[0], true));
        assert!(!pnn_self_red_shape(33, 3, 34, &[33], true));
        assert!(!pnn_self_red_shape(4, 33, 5, &[4], true));
    }
}

#[cfg(test)]
mod op085_archive_latency_budget_contract_tests {
    use super::*;
    #[test]
    fn fresh_proof_timeout_stays_35s_durable_gets_85s_within_100s_wallet_guard() {
        assert_eq!(
            archival_proof_timeout(false),
            std::time::Duration::from_secs(35)
        );
        assert_eq!(
            archival_proof_timeout(true),
            std::time::Duration::from_secs(85)
        );
        assert!(archival_proof_timeout(true) < std::time::Duration::from_secs(100));
        assert!(archival_proof_timeout(true) > std::time::Duration::from_secs(25 + 20 + 35));
    }
    #[test]
    fn split_guard_tracks_cancel_without_bypassing_proof_result() {
        let guard = PnnSplitRestLatency::new();
        assert!(guard.completed.is_none());
        drop(guard); // cancellation: no alert, only aggregate telemetry
        let mut guard = PnnSplitRestLatency::new();
        guard.completed = Some(false); // deferred
        drop(guard);
        let mut guard = PnnSplitRestLatency::new();
        guard.completed = Some(true); // only full verified proof
        drop(guard);
    }
}
