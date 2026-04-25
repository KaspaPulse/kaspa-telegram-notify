use crate::domain::errors::AppError;
use crate::domain::models::BlockData;
use crate::infrastructure::node::kaspa_adapter::KaspaRpcAdapter;
use kaspa_hashes::Hash;
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_wrpc_client::KaspaRpcClient;
use std::collections::HashSet;
use std::sync::Arc;
use tokio::time::{sleep, Duration};

pub struct AnalyzeDagUseCase {
    pub node: Arc<KaspaRpcAdapter>,
}

impl AnalyzeDagUseCase {
    pub fn new(node: Arc<KaspaRpcAdapter>) -> Self {
        Self { node }
    }

    pub async fn get_pruning_block(&self, hash: &str) -> Option<BlockData> {
        self.node.get_block(hash).await.ok()
    }

    // 🚀 RESTORED ORIGINAL FORENSICS ALGORITHM
    pub async fn execute(
        &self,
        f_tx: &str,
        w_cl: &str,
        daa_score: u64,
        is_coinbase: bool,
    ) -> Result<(String, Vec<String>, String, String, u64), AppError> {
        let mut acc_block_hash = String::new();
        let mut actual_mined_blocks: Vec<String> = Vec::new();
        let mut extracted_nonce = String::new();
        let mut extracted_worker = String::new();
        let mut block_time_ms: u64 = 0;

        // Bypassing abstract traits to guarantee raw access to verbose_data for forensics
        let url =
            std::env::var("NODE_URL_01").unwrap_or_else(|_| "ws://127.0.0.1:16110".to_string());
        let rpc_cl = KaspaRpcClient::new(
            kaspa_wrpc_client::WrpcEncoding::SerdeJson,
            Some(&url),
            None,
            None,
            None,
        )
        .unwrap();
        let _ = rpc_cl.connect(None).await;

        let mut visited = HashSet::new();
        let mut current_hashes = match rpc_cl.get_block_dag_info().await {
            Ok(info) => info.tip_hashes,
            Err(_) => vec![],
        };

        for _attempt in 1..=800 {
            if current_hashes.is_empty() {
                break;
            }
            let mut next_hashes = vec![];
            for hash in &current_hashes {
                if !visited.insert(*hash) {
                    continue;
                }
                if let Ok(block) = rpc_cl.get_block(*hash, true).await {
                    let mut found_tx = false;
                    for tx in &block.transactions {
                        if let Some(tx_verb) = &tx.verbose_data {
                            if tx_verb.transaction_id.to_string() == f_tx {
                                found_tx = true;
                                break;
                            }
                        }
                    }
                    if found_tx {
                        acc_block_hash = hash.to_string();
                        block_time_ms = block.header.timestamp;
                        break;
                    }
                    if block.header.daa_score >= daa_score.saturating_sub(60) {
                        for level in &block.header.parents_by_level {
                            for p_hash in level {
                                next_hashes.push(*p_hash);
                            }
                        }
                    }
                }
            }
            if !acc_block_hash.is_empty() {
                break;
            }
            current_hashes = next_hashes;
            sleep(Duration::from_millis(5)).await;
        }

        if is_coinbase && !acc_block_hash.is_empty() {
            if let Ok(acc_hash_obj) = acc_block_hash.parse::<Hash>() {
                if let Ok(full_acc_block) = rpc_cl.get_block(acc_hash_obj, true).await {
                    let mut user_script_bytes: Vec<u8> = Vec::new();
                    if let Some(tx0) = full_acc_block.transactions.first() {
                        for out in &tx0.outputs {
                            if let Some(ov) = &out.verbose_data {
                                if ov.script_public_key_address.to_string() == w_cl {
                                    user_script_bytes = out.script_public_key.script().to_vec();
                                    break;
                                }
                            }
                        }
                    }
                    if !user_script_bytes.is_empty() {
                        if let Some(verbose) = &full_acc_block.verbose_data {
                            for blue_hash in &verbose.merge_set_blues_hashes {
                                if let Ok(blue_block) = rpc_cl.get_block(*blue_hash, true).await {
                                    if let Some(m_tx0) = blue_block.transactions.first() {
                                        // 🔍 THE REAL FORENSICS: Searching for user bytes in the payload
                                        if let Some(pos) = m_tx0
                                            .payload
                                            .windows(user_script_bytes.len())
                                            .position(|w| w == user_script_bytes.as_slice())
                                        {
                                            actual_mined_blocks.push(blue_hash.to_string());
                                            block_time_ms = blue_block.header.timestamp;
                                            if extracted_nonce.is_empty() {
                                                extracted_nonce =
                                                    blue_block.header.nonce.to_string();
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
        }

        let _ = rpc_cl.disconnect().await;
        Ok((
            acc_block_hash,
            actual_mined_blocks,
            extracted_nonce,
            extracted_worker,
            block_time_ms,
        ))
    }
}
