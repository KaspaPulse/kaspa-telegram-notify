use crate::domain::errors::AppError;
use crate::infrastructure::database::postgres_adapter::PostgresRepository;
use crate::infrastructure::node::kaspa_adapter::KaspaRpcAdapter;
// ===== Migrated from wallet_management.rs =====
use crate::domain::entities::TrackedWallet;
use std::sync::Arc;

pub struct WalletManagementUseCase {
    db: Arc<PostgresRepository>,
}

impl WalletManagementUseCase {
    pub fn new(db: Arc<PostgresRepository>) -> Self {
        Self { db }
    }

    pub async fn add_wallet(&self, address: &str, chat_id: i64) -> Result<(), AppError> {
        let wallet = TrackedWallet {
            address: address.to_string(),
            chat_id,
        };
        self.db.add_tracked_wallet(wallet).await
    }

    pub async fn remove_wallet(&self, address: &str, chat_id: i64) -> Result<(), AppError> {
        self.db.remove_tracked_wallet(address, chat_id).await
    }
}

// ===== Migrated from wallet_queries.rs =====

pub struct WalletQueriesUseCase {
    db: Arc<PostgresRepository>,
    node: Arc<KaspaRpcAdapter>,
}

impl WalletQueriesUseCase {
    pub fn new(db: Arc<PostgresRepository>, node: Arc<KaspaRpcAdapter>) -> Self {
        Self { db, node }
    }

    pub async fn get_list(&self, chat_id: i64) -> Result<Vec<String>, AppError> {
        let wallets = self.db.get_all_tracked_wallets().await?;
        let user_wallets: Vec<String> = wallets
            .into_iter()
            .filter(|w| w.chat_id == chat_id)
            .map(|w| w.address)
            .collect();
        Ok(user_wallets)
    }

    pub async fn get_balance(&self, chat_id: i64) -> Result<(u64, usize), AppError> {
        let wallets = self.get_list(chat_id).await?;
        let mut total_bal = 0;
        let mut total_utxos = 0;
        for w in &wallets {
            if let Ok((bal, utxos)) = self.node.get_balance(w).await {
                total_bal += bal;
                total_utxos += utxos;
            }
        }
        Ok((total_bal, total_utxos))
    }

    pub async fn get_blocks_stats(
        &self,
        chat_id: i64,
    ) -> Result<(i64, i64, i64, Vec<(String, i64)>), AppError> {
        let wallets = self.get_list(chat_id).await?;
        let mut total_1h = 0;
        let mut total_24h = 0;
        let mut total_lifetime = 0;
        let mut daily_stats: std::collections::HashMap<String, i64> =
            std::collections::HashMap::new();
        for w in &wallets {
            if let Ok(b1) = self.db.get_blocks_count_1h(w).await {
                total_1h += b1;
            }
            if let Ok(b24) = self.db.get_blocks_count_24h(w).await {
                total_24h += b24;
            }
            if let Ok((life, _)) = self.db.get_lifetime_stats(w).await {
                total_lifetime += life;
            }
            if let Ok(daily) = self.db.get_daily_blocks(w).await {
                for (day, count) in daily {
                    *daily_stats.entry(day).or_insert(0) += count;
                }
            }
        }
        let mut daily_vec: Vec<(String, i64)> = daily_stats.into_iter().collect();
        daily_vec.sort_by(|a, b| b.0.cmp(&a.0)); // Sort newest first
        Ok((total_1h, total_24h, total_lifetime, daily_vec))
    }
}

// ===== Migrated from sync_wallet.rs =====
use crate::domain::entities::MinedBlock;
use std::collections::{HashSet, VecDeque};
use tracing::{info, warn};

pub struct SyncWalletUseCase {
    db: Arc<PostgresRepository>,
    node: Arc<KaspaRpcAdapter>,
}

impl SyncWalletUseCase {
    pub fn new(db: Arc<PostgresRepository>, node: Arc<KaspaRpcAdapter>) -> Self {
        Self { db, node }
    }

    pub async fn execute(&self, wallet_address: &str) -> Result<(), AppError> {
        info!("🔍 [REVERSE SCAN] Initiated for wallet: {}", wallet_address);

        let tip_hashes = self.node.get_tip_hashes().await?;
        let pruning_point = self.node.get_pruning_point().await?;
        let last_checkpoint = self
            .db
            .get_sync_checkpoint(wallet_address)
            .await
            .unwrap_or(0);
        let virtual_daa = self.node.get_virtual_daa_score().await?;

        info!(
            "📊 [SYNC STATS] Checkpoint Score: {} | Pruning Point Hash: {}",
            last_checkpoint, pruning_point
        );

        let mut queue = VecDeque::from(tip_hashes);
        let mut visited = HashSet::new();
        let mut scanned_count = 0;
        let mut recovered_count = 0;

        while let Some(current_hash) = queue.pop_front() {
            if !visited.insert(current_hash.clone()) || current_hash == pruning_point {
                continue;
            }

            if let Ok(block) = self.node.get_block(&current_hash).await {
                scanned_count += 1;

                if scanned_count > 100_000 {
                    warn!(
                        "⚠️ [SYNC LIMIT] Reached maximum scan depth for {}. Stopping.",
                        wallet_address
                    );
                    break;
                }

                if scanned_count % 1000 == 0 {
                    tokio::task::yield_now().await;
                    info!(
                        "⏳ [PROGRESS] Scanned {} blocks... current DAA: {}",
                        scanned_count, block.daa_score
                    );
                }

                if block.daa_score <= last_checkpoint {
                    continue;
                }

                // 🧠 The Heavy Scanning is cleanly delegated to the Infrastructure Adapter (node.scan_block_for_reward)
                if let Ok(rewards) = self
                    .node
                    .scan_block_for_reward(&current_hash, wallet_address)
                    .await
                {
                    for (outpoint, amount) in rewards {
                        let mined_block = MinedBlock {
                            wallet_address: wallet_address.to_string(),
                            outpoint,
                            amount,
                            daa_score: block.daa_score,
                        };
                        if self.db.record_mined_block(mined_block).await.is_ok() {
                            recovered_count += 1;
                            info!(
                                "✅ [RECOVERY SUCCESS] | Amount: {:.2} KAS | DAA: {}",
                                (amount as f64 / 1e8),
                                block.daa_score
                            );
                        }
                    }
                }

                for parent in block.parents {
                    if !visited.contains(&parent) {
                        queue.push_back(parent);
                    }
                }
            }
        }

        let _ = self
            .db
            .update_sync_checkpoint(wallet_address, virtual_daa)
            .await;
        info!(
            "🏁 [SYNC FINISHED] Scanned: {} | Recovered: {}",
            scanned_count, recovered_count
        );
        Ok(())
    }
}

// --- Merged from monitor_utxos.rs ---

// --- Restored UtxoMonitorService (Deep Scan) ---

use crate::domain::models::LiveBlockEvent;
use crate::network::analyze_dag::AnalyzeDagUseCase;
use dashmap::DashMap;

pub struct UtxoMonitorService {
    node: Arc<KaspaRpcAdapter>,
    db: Arc<PostgresRepository>,
    analyzer: Arc<AnalyzeDagUseCase>,
    known_utxos: DashMap<String, HashSet<String>>,
}

impl UtxoMonitorService {
    pub fn new(
        node: Arc<KaspaRpcAdapter>,
        db: Arc<PostgresRepository>,
        analyzer: Arc<AnalyzeDagUseCase>,
    ) -> Self {
        Self {
            node,
            db,
            analyzer,
            known_utxos: DashMap::new(),
        }
    }

    pub async fn check_wallet_utxos(
        &self,
        wallet_address: &str,
    ) -> Result<Vec<LiveBlockEvent>, AppError> {
        let utxos = self.node.get_utxos(wallet_address).await?;
        let mut current_outpoints = HashSet::new();
        let mut new_rewards = Vec::new();
        let mut known = self
            .known_utxos
            .entry(wallet_address.to_string())
            .or_default();
        let is_first_run = known.is_empty();

        for u in utxos {
            current_outpoints.insert(u.outpoint.clone());
            if !is_first_run && !known.contains(&u.outpoint) {
                new_rewards.push(u.clone());
                known.insert(u.outpoint.clone());
            } else if is_first_run {
                known.insert(u.outpoint.clone());
            }
        }
        known.retain(|k| current_outpoints.contains(k));
        if new_rewards.is_empty() {
            return Ok(vec![]);
        }

        let mut join_set = tokio::task::JoinSet::new();
        for u in new_rewards {
            let analyzer = self.analyzer.clone();
            let db = self.db.clone();
            let node = self.node.clone();
            let w_cl = wallet_address.to_string();

            join_set.spawn(async move {
                if u.is_coinbase {
                    let block = crate::domain::entities::MinedBlock {
                        wallet_address: w_cl.clone(),
                        outpoint: u.outpoint.clone(),
                        amount: (u.amount as i64),
                        daa_score: u.block_daa_score,
                    };
                    let _ = db.record_mined_block(block).await;
                }

                let (acc_block_hash, actual_mined_blocks, _nonce, extracted_worker, block_time_ms) =
                    analyzer
                        .execute(&u.transaction_id, &w_cl, u.block_daa_score, u.is_coinbase)
                        .await
                        .unwrap_or_default();

                let live_bal = node.get_balance(&w_cl).await.map(|(b, _)| b).unwrap_or(0);

                let event = LiveBlockEvent {
                    is_coinbase: u.is_coinbase,
                    wallet_address: w_cl,
                    amount_kas: ((u.amount as i64) as f64 / 1e8),
                    live_balance_kas: (live_bal as f64 / 1e8),
                    tx_id: u.transaction_id,
                    block_time_ms,
                    acc_block_hash,
                    mined_block_hash: actual_mined_blocks.first().cloned(),
                    extracted_worker: if !extracted_worker.is_empty() {
                        Some(extracted_worker)
                    } else {
                        None
                    },
                    daa_score: u.block_daa_score,
                };
                (block_time_ms, event)
            });
        }

        let mut sorted_events = Vec::new();
        while let Some(res) = join_set.join_next().await {
            if let Ok(data) = res {
                sorted_events.push(data);
            }
        }
        sorted_events.sort_by_key(|(time, _)| *time);

        Ok(sorted_events.into_iter().map(|(_, e)| e).collect())
    }
}
