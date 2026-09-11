use crate::domain::entities::{MinedBlock, TrackedWallet};
use crate::domain::errors::AppError;
use crate::domain::models::LiveBlockEvent;
use crate::domain::models::{BotEventRecord, BotEventType, EventSeverity};
use crate::infrastructure::database::postgres_adapter::PostgresRepository;
use crate::infrastructure::node::kaspa_adapter::KaspaRpcAdapter;
use crate::network::analyze_dag::AnalyzeDagUseCase;
use dashmap::DashMap;
use std::collections::HashSet;
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

#[derive(Debug, Clone)]
pub struct WalletBalanceDetail {
    pub address: String,
    pub balance_sompi: u64,
    pub utxos: usize,
    pub is_online: bool,
}

#[derive(Debug, Clone)]
pub struct WalletBlocksDetail {
    pub address: String,
    pub blocks_1h: i64,
    pub blocks_1h_sompi: i64,
    pub blocks_24h: i64,
    pub blocks_24h_sompi: i64,
    pub blocks_7d: i64,
    pub blocks_7d_sompi: i64,
    pub lifetime_blocks: i64,
    pub lifetime_sompi: i64,
    pub daily_blocks: Vec<(String, i64, i64, Option<f64>)>,
    pub kas_price_usd: Option<f64>,
}

pub struct WalletQueriesUseCase {
    db: Arc<PostgresRepository>,
    node: Arc<KaspaRpcAdapter>,
}

impl WalletQueriesUseCase {
    pub fn new(db: Arc<PostgresRepository>, node: Arc<KaspaRpcAdapter>) -> Self {
        Self { db, node }
    }

    pub async fn get_list(&self, chat_id: i64) -> Result<Vec<String>, AppError> {
        let wallets = self.db.get_tracked_wallets_for_chat(chat_id).await?;

        Ok(wallets.into_iter().map(|wallet| wallet.address).collect())
    }

    pub async fn get_wallet_balances(
        &self,
        chat_id: i64,
    ) -> Result<Vec<WalletBalanceDetail>, AppError> {
        let wallets = self.get_list(chat_id).await?;
        let mut details = Vec::new();

        for wallet in wallets {
            let (balance_sompi, utxos) = match self.node.get_balance(&wallet).await {
                Ok(balance) => balance,
                Err(error) => {
                    let error_message = error.to_string();
                    let wallet_masked = crate::utils::format_short_wallet(&wallet);
                    let mut rpc_event =
                        BotEventRecord::new(BotEventType::RpcError, EventSeverity::Error);
                    rpc_event.wallet_masked = Some(&wallet_masked);
                    rpc_event.status = Some("wallet_balance_read_failed");
                    rpc_event.error_message = Some(&error_message);
                    rpc_event.metadata_json = r#"{"operation":"get_balance","fallback":"none"}"#;
                    let _ = self.db.record_bot_event_record(rpc_event).await;
                    tracing::error!(
                        wallet = %wallet_masked,
                        error = %crate::utils::sanitize_for_log(&error_message),
                        "[RPC ERROR] Required wallet balance read failed."
                    );
                    return Err(error);
                }
            };

            details.push(WalletBalanceDetail {
                address: wallet,
                balance_sompi,
                utxos,
                is_online: true,
            });
        }

        Ok(details)
    }

    async fn require_wallet_stats<T>(
        &self,
        wallet: &str,
        operation: &'static str,
        status: &'static str,
        metadata_json: &'static str,
        result: Result<T, AppError>,
    ) -> Result<T, AppError> {
        match result {
            Ok(value) => Ok(value),
            Err(error) => {
                let error_message = error.to_string();
                let wallet_masked = crate::utils::format_short_wallet(wallet);
                let mut db_event = BotEventRecord::new(BotEventType::DbError, EventSeverity::Error);
                db_event.wallet_masked = Some(&wallet_masked);
                db_event.status = Some(status);
                db_event.error_message = Some(&error_message);
                db_event.metadata_json = metadata_json;
                let _ = self.db.record_bot_event_record(db_event).await;
                tracing::error!(
                    operation,
                    wallet = %wallet_masked,
                    error = %crate::utils::sanitize_for_log(&error_message),
                    "[DATABASE ERROR] Required mining statistic read failed."
                );
                Err(error)
            }
        }
    }

    pub async fn get_wallet_blocks_details(
        &self,
        chat_id: i64,
    ) -> Result<Vec<WalletBlocksDetail>, AppError> {
        let wallets = self.get_list(chat_id).await?;
        let mut details = Vec::new();

        for wallet in wallets {
            let stats_wallet_masked = crate::utils::format_short_wallet(&wallet);

            let blocks_1h = self
                .require_wallet_stats(
                    &wallet,
                    "get_blocks_count_1h",
                    "stats_1h_read_failed",
                    r#"{"operation":"get_blocks_count_1h","fallback":"none"}"#,
                    self.db.get_blocks_count_1h(&wallet).await,
                )
                .await?;
            let blocks_24h = self
                .require_wallet_stats(
                    &wallet,
                    "get_blocks_count_24h",
                    "stats_24h_read_failed",
                    r#"{"operation":"get_blocks_count_24h","fallback":"none"}"#,
                    self.db.get_blocks_count_24h(&wallet).await,
                )
                .await?;
            let blocks_7d = self
                .require_wallet_stats(
                    &wallet,
                    "get_blocks_count_7d",
                    "stats_7d_read_failed",
                    r#"{"operation":"get_blocks_count_7d","fallback":"none"}"#,
                    self.db.get_blocks_count_7d(&wallet).await,
                )
                .await?;
            let blocks_1h_sompi = self
                .require_wallet_stats(
                    &wallet,
                    "get_blocks_sum_sompi_1h",
                    "stats_1h_sompi_read_failed",
                    r#"{"operation":"get_blocks_sum_sompi_1h","fallback":"none"}"#,
                    self.db.get_blocks_sum_sompi_1h(&wallet).await,
                )
                .await?;
            let blocks_24h_sompi = self
                .require_wallet_stats(
                    &wallet,
                    "get_blocks_sum_sompi_24h",
                    "stats_24h_sompi_read_failed",
                    r#"{"operation":"get_blocks_sum_sompi_24h","fallback":"none"}"#,
                    self.db.get_blocks_sum_sompi_24h(&wallet).await,
                )
                .await?;
            let blocks_7d_sompi = self
                .require_wallet_stats(
                    &wallet,
                    "get_blocks_sum_sompi_7d",
                    "stats_7d_sompi_read_failed",
                    r#"{"operation":"get_blocks_sum_sompi_7d","fallback":"none"}"#,
                    self.db.get_blocks_sum_sompi_7d(&wallet).await,
                )
                .await?;
            let (lifetime_blocks, lifetime_sompi) = self
                .require_wallet_stats(
                    &wallet,
                    "get_lifetime_stats",
                    "lifetime_stats_read_failed",
                    r#"{"operation":"get_lifetime_stats","fallback":"none"}"#,
                    self.db.get_lifetime_stats(&wallet).await,
                )
                .await?;
            let daily_blocks_raw = self
                .require_wallet_stats(
                    &wallet,
                    "get_all_daily_blocks",
                    "daily_blocks_read_failed",
                    r#"{"operation":"get_all_daily_blocks","fallback":"none"}"#,
                    self.db.get_all_daily_blocks(&wallet).await,
                )
                .await?;

            let kas_price_usd = match self.db.get_latest_kas_price_usd().await {
                Ok(price) => price,
                Err(error) => {
                    tracing::warn!(
                        wallet = %stats_wallet_masked,
                        error = %crate::utils::sanitize_for_log(&error.to_string()),
                        "[DATABASE WARNING] Optional current KAS price enrichment unavailable."
                    );
                    None
                }
            };

            let day_keys: Vec<String> = daily_blocks_raw
                .iter()
                .map(|(day, _, _)| day.clone())
                .collect();
            let daily_price_map = match self.db.get_kas_price_usd_map_for_days(&day_keys).await {
                Ok(prices) => prices,
                Err(error) => {
                    tracing::warn!(
                        wallet = %stats_wallet_masked,
                        error = %crate::utils::sanitize_for_log(&error.to_string()),
                        "[DATABASE WARNING] Optional historical KAS price enrichment unavailable."
                    );
                    std::collections::HashMap::new()
                }
            };

            let daily_blocks = daily_blocks_raw
                .into_iter()
                .map(|(day, count, total_sompi)| {
                    let price_usd = daily_price_map.get(&day).copied();
                    (day, count, total_sompi, price_usd)
                })
                .collect();

            details.push(WalletBlocksDetail {
                address: wallet,
                blocks_1h,
                blocks_1h_sompi,
                blocks_24h,
                blocks_24h_sompi,
                blocks_7d,
                blocks_7d_sompi,
                lifetime_blocks,
                lifetime_sompi,
                daily_blocks,
                kas_price_usd,
            });
        }

        Ok(details)
    }
}

#[derive(Debug)]
pub struct WalletUtxoScanResult {
    pub events: Vec<LiveBlockEvent>,
    pub completed_without_errors: bool,
}

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
    ) -> Result<WalletUtxoScanResult, AppError> {
        let utxos = self.node.get_utxos(wallet_address).await?;

        let min_reward_confirmations = std::env::var("MIN_REWARD_CONFIRMATIONS")
            .unwrap_or_else(|_| "10".to_string())
            .parse::<u64>()
            .unwrap_or(10)
            .clamp(1, 10_000);

        let virtual_daa_score = self.node.get_virtual_daa_score().await?;
        let mut completed_without_errors = true;

        let mut current_outpoints = HashSet::new();
        let mut current_outpoints_vec = Vec::new();
        let mut new_rewards = Vec::new();

        let mut known_db = match self.db.get_seen_utxos(wallet_address).await {
            Ok(value) => value,
            Err(e) => {
                let wallet_masked = crate::utils::format_short_wallet(wallet_address);
                let error_text = e.to_string();

                let mut db_error_event =
                    BotEventRecord::new(BotEventType::DbError, EventSeverity::Error);
                db_error_event.wallet_masked = Some(&wallet_masked);
                db_error_event.status = Some("seen_utxo_load_failed");
                db_error_event.error_message = Some(&error_text);
                db_error_event.metadata_json =
                    r#"{"operation":"get_seen_utxos","action":"abort_wallet_scan"}"#;

                let _ = self.db.record_bot_event_record(db_error_event).await;

                tracing::error!(
                    "[DATABASE ERROR] Failed to load seen UTXOs for wallet {}: {}",
                    wallet_masked,
                    error_text
                );

                return Err(e);
            }
        };
        let mut known_mem = self
            .known_utxos
            .entry(wallet_address.to_string())
            .or_default();

        if known_mem.is_empty() && !known_db.is_empty() {
            for outpoint in &known_db {
                known_mem.insert(outpoint.clone());
            }
        }

        let is_first_run = known_mem.is_empty() && known_db.is_empty();

        for utxo in utxos {
            current_outpoints.insert(utxo.outpoint.clone());

            let seen_before =
                known_mem.contains(&utxo.outpoint) || known_db.contains(&utxo.outpoint);

            let reward_status = crate::wallet::reward_confirmation::reward_confirmation_status(
                utxo.is_coinbase,
                utxo.block_daa_score,
                virtual_daa_score,
                min_reward_confirmations,
            );
            let reward_confirmations = reward_status.confirmations;

            let reward_decision = crate::wallet::reward_confirmation::reward_processing_decision(
                is_first_run,
                seen_before,
                utxo.is_coinbase,
                utxo.block_daa_score,
                virtual_daa_score,
                min_reward_confirmations,
            );

            let reward_is_confirmed = matches!(
                reward_decision,
                crate::wallet::reward_confirmation::RewardProcessingDecision::ProcessNow
                    | crate::wallet::reward_confirmation::RewardProcessingDecision::AlreadySeen
                    | crate::wallet::reward_confirmation::RewardProcessingDecision::FirstRunSnapshot
            );

            if !is_first_run && !seen_before {
                if !reward_is_confirmed {
                    if let Err(e) = self
                        .db
                        .upsert_pending_reward(
                            wallet_address,
                            &utxo,
                            virtual_daa_score,
                            reward_confirmations,
                            min_reward_confirmations,
                        )
                        .await
                    {
                        completed_without_errors = false;
                        let wallet_masked = crate::utils::format_short_wallet(wallet_address);
                        let txid_masked = crate::utils::format_short_wallet(&utxo.transaction_id);
                        let error_text = e.to_string();

                        let mut db_error_event =
                            BotEventRecord::new(BotEventType::DbError, EventSeverity::Error);
                        db_error_event.wallet_masked = Some(&wallet_masked);
                        db_error_event.txid_masked = Some(&txid_masked);
                        db_error_event.status = Some("pending_reward_upsert_failed");
                        db_error_event.error_message = Some(&error_text);
                        db_error_event.metadata_json = r#"{"operation":"upsert_pending_reward"}"#;

                        let _ = self.db.record_bot_event_record(db_error_event).await;

                        tracing::error!(
                            "[DATABASE ERROR] Failed to upsert pending reward for wallet {} tx {}: {}",
                            wallet_masked,
                            txid_masked,
                            error_text
                        );
                    }

                    tracing::debug!(
                        "[REWARD CONFIRMATION] Waiting before DAG analysis. wallet={} tx={} confirmations={}/{} reward_daa={} virtual_daa={}",
                        crate::utils::format_short_wallet(wallet_address),
                        crate::utils::format_short_wallet(&utxo.transaction_id),
                        reward_confirmations,
                        min_reward_confirmations,
                        utxo.block_daa_score,
                        virtual_daa_score
                    );

                    continue;
                }

                if let Err(e) = self
                    .db
                    .delete_pending_reward(wallet_address, &utxo.outpoint)
                    .await
                {
                    completed_without_errors = false;
                    tracing::warn!(
                        "[DATABASE WARNING] Failed to delete pending reward before processing. wallet={} tx={}: {}",
                        crate::utils::format_short_wallet(wallet_address),
                        crate::utils::format_short_wallet(&utxo.transaction_id),
                        e
                    );
                } else {
                    tracing::debug!(
                        "[REWARD CONFIRMATION] pending_reward_ready_for_processing wallet={} tx={} confirmations={}/{}",
                        crate::utils::format_short_wallet(wallet_address),
                        crate::utils::format_short_wallet(&utxo.transaction_id),
                        reward_confirmations,
                        min_reward_confirmations
                    );
                }

                new_rewards.push(utxo.clone());
            }

            if seen_before || is_first_run {
                current_outpoints_vec.push(utxo.outpoint.clone());
                known_mem.insert(utxo.outpoint.clone());
                known_db.insert(utxo.outpoint.clone());
            }
        }

        known_mem.retain(|outpoint| current_outpoints.contains(outpoint));

        if let Err(e) = self
            .db
            .upsert_seen_utxos(wallet_address, &current_outpoints_vec)
            .await
        {
            completed_without_errors = false;
            let wallet_masked = crate::utils::format_short_wallet(wallet_address);
            let error_text = e.to_string();

            let mut db_error_event =
                BotEventRecord::new(BotEventType::DbError, EventSeverity::Error);
            db_error_event.wallet_masked = Some(&wallet_masked);
            db_error_event.status = Some("seen_utxo_upsert_failed");
            db_error_event.error_message = Some(&error_text);

            let _ = self.db.record_bot_event_record(db_error_event).await;

            tracing::error!("[DATABASE ERROR] Failed to persist seen UTXOs: {}", e);
        }

        if let Err(e) = self
            .db
            .prune_seen_utxos(wallet_address, &current_outpoints_vec)
            .await
        {
            completed_without_errors = false;
            tracing::warn!("[DATABASE WARNING] Failed to prune seen UTXOs: {}", e);
        }

        if new_rewards.is_empty() {
            return Ok(WalletUtxoScanResult {
                events: Vec::new(),
                completed_without_errors,
            });
        }

        let mut join_set = tokio::task::JoinSet::new();

        for utxo in new_rewards {
            let analyzer = self.analyzer.clone();
            let db = self.db.clone();
            let node = self.node.clone();
            let wallet = wallet_address.to_string();

            join_set.spawn(async move {
                let mut task_succeeded = true;

                if utxo.is_coinbase {
                    let block = MinedBlock {
                        wallet_address: wallet.clone(),
                        outpoint: utxo.outpoint.clone(),
                        amount: utxo.amount as i64,
                        daa_score: utxo.block_daa_score,
                    };

                    if let Err(e) = db.record_mined_block(block).await {
                        task_succeeded = false;
                        let wallet_masked = crate::utils::format_short_wallet(&wallet);
                        let txid_masked = crate::utils::format_short_wallet(&utxo.transaction_id);
                        let error_text = e.to_string();

                        let mut db_error_event =
                            BotEventRecord::new(BotEventType::DbError, EventSeverity::Error);
                        db_error_event.wallet_masked = Some(&wallet_masked);
                        db_error_event.txid_masked = Some(&txid_masked);
                        db_error_event.status = Some("record_mined_block_failed");
                        db_error_event.error_message = Some(&error_text);

                        let _ = db.record_bot_event_record(db_error_event).await;

                        tracing::error!("[DATABASE ERROR] Failed to record mined block: {}", e);
                    }
                }

                let analysis = analyzer
                    .execute(
                        &utxo.transaction_id,
                        &wallet,
                        utxo.block_daa_score,
                        utxo.is_coinbase,
                    )
                    .await;

                let (acc_block_hash, actual_mined_blocks, _nonce, extracted_worker, block_time_ms) =
                    match analysis {
                        Ok(data) => data,
                        Err(e) => {
                            let wallet_masked = crate::utils::format_short_wallet(&wallet);
                            let txid_masked =
                                crate::utils::format_short_wallet(&utxo.transaction_id);
                            let error_text = e.to_string();

                            let mut rpc_error_event =
                                BotEventRecord::new(BotEventType::RpcError, EventSeverity::Error);
                            rpc_error_event.wallet_masked = Some(&wallet_masked);
                            rpc_error_event.txid_masked = Some(&txid_masked);
                            rpc_error_event.status = Some("dag_analysis_failed");
                            rpc_error_event.error_message = Some(&error_text);

                            let _ = db.record_bot_event_record(rpc_error_event).await;

                            tracing::error!(
                                "[DAG ERROR] Failed to analyze reward {} for {}: {}",
                                crate::utils::format_short_wallet(&utxo.transaction_id),
                                crate::utils::format_short_wallet(&wallet),
                                e
                            );

                            return (None, false);
                        }
                    };

                let mined_block_hash = actual_mined_blocks.first().cloned();
                let alert_key = crate::wallet::alert_dedup::build_alert_key(
                    mined_block_hash.as_deref(),
                    &utxo.transaction_id,
                );

                let live_balance = match node.get_balance(&wallet).await {
                    Ok((balance, _)) => balance,
                    Err(e) => {
                        task_succeeded = false;
                        let error_message = e.to_string();
                        let wallet_masked = crate::utils::format_short_wallet(&wallet);
                        let txid_masked_for_balance =
                            crate::utils::format_short_wallet(&utxo.transaction_id);

                        let mut rpc_event =
                            BotEventRecord::new(BotEventType::RpcError, EventSeverity::Error);
                        rpc_event.wallet_masked = Some(&wallet_masked);
                        rpc_event.txid_masked = Some(&txid_masked_for_balance);
                        rpc_event.status = Some("live_balance_failed");
                        rpc_event.error_message = Some(&error_message);
                        rpc_event.metadata_json =
                            r#"{"operation":"get_balance","fallback_balance_sompi":0}"#;

                        let _ = db.record_bot_event_record(rpc_event).await;

                        0
                    }
                };

                let event = LiveBlockEvent {
                    is_coinbase: utxo.is_coinbase,
                    wallet_address: wallet,
                    amount_kas: utxo.amount as f64 / 1e8,
                    live_balance_kas: live_balance as f64 / 1e8,
                    source_outpoint: utxo.outpoint,
                    alert_key,
                    tx_id: utxo.transaction_id,
                    block_time_ms,
                    acc_block_hash,
                    mined_block_hash,
                    extracted_worker: if extracted_worker.is_empty() {
                        None
                    } else {
                        Some(extracted_worker)
                    },
                    daa_score: utxo.block_daa_score,
                };

                (Some((block_time_ms, event)), task_succeeded)
            });
        }

        let mut sorted_events = Vec::new();

        while let Some(result) = join_set.join_next().await {
            match result {
                Ok((Some(data), task_succeeded)) => {
                    completed_without_errors &= task_succeeded;
                    sorted_events.push(data);
                }
                Ok((None, _)) => {
                    completed_without_errors = false;
                }
                Err(error) => {
                    completed_without_errors = false;
                    tracing::error!("[WORKER] Reward analysis task failed to join: {}", error);
                }
            }
        }

        sorted_events.sort_by_key(|(time, _)| *time);

        Ok(WalletUtxoScanResult {
            events: sorted_events.into_iter().map(|(_, event)| event).collect(),
            completed_without_errors,
        })
    }
}
