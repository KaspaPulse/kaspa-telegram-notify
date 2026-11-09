use crate::domain::entities::{MinedBlock, TrackedWallet, WalletRemovalOutcome};
use crate::domain::errors::AppError;
use crate::domain::models::LiveBlockEvent;
use crate::domain::models::{BotEventRecord, BotEventType, EventSeverity};
use crate::infrastructure::database::postgres_adapter::PostgresRepository;
use crate::infrastructure::node::kaspa_adapter::KaspaRpcAdapter;
use crate::network::analyze_dag::AnalyzeDagUseCase;
use dashmap::DashMap;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::{AtomicU64, Ordering};

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

    pub async fn remove_wallet(
        &self,
        address: &str,
        chat_id: i64,
    ) -> Result<WalletRemovalOutcome, AppError> {
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

#[derive(Default)]
struct WalletUtxoCache {
    wallets: DashMap<String, Arc<tokio::sync::Mutex<HashSet<String>>>>,
}

impl WalletUtxoCache {
    async fn lock(&self, wallet: &str) -> tokio::sync::OwnedMutexGuard<HashSet<String>> {
        // Drop the synchronous shard guard before awaiting wallet ownership.
        let wallet_state = self.wallets.entry(wallet.to_string()).or_default().clone();
        wallet_state.lock_owned().await
    }
}

pub struct UtxoMonitorService {
    node: Arc<KaspaRpcAdapter>,
    db: Arc<PostgresRepository>,
    analyzer: Arc<AnalyzeDagUseCase>,
    known_utxos: WalletUtxoCache,
    reward_analysis_gate: Arc<tokio::sync::Semaphore>,
}

fn parse_reward_analysis_max_concurrency(value: Option<&str>) -> usize {
    value
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(8)
        .clamp(1, 32) as usize
}

fn reward_analysis_max_concurrency() -> usize {
    let value = std::env::var("KASPA_REWARD_ANALYSIS_MAX_CONCURRENCY").ok();
    parse_reward_analysis_max_concurrency(value.as_deref())
}

fn parse_reward_analysis_timeout(value: Option<&str>) -> std::time::Duration {
    std::time::Duration::from_secs(
        value
            .and_then(|value| value.parse::<u64>().ok())
            .unwrap_or(10)
            .clamp(5, 120),
    )
}

fn reward_analysis_timeout() -> std::time::Duration {
    let value = std::env::var("KASPA_REWARD_ANALYSIS_TIMEOUT_SECS").ok();
    parse_reward_analysis_timeout(value.as_deref())
}

// OP070: A queued, serialized public archive proof may wait up to 25s for
// admission and up to 20s for the normal global cooldown before the 35s
// verified fetch. Reserve a bounded child timeout for the ALREADY-DURABLE
// newer Coinbase proof route only; preserve fast primary PNN fresh scans.
fn proof_route_timeout(
    configured: std::time::Duration,
    already_durable_coinbase: bool,
) -> std::time::Duration {
    if already_durable_coinbase {
        configured.max(std::time::Duration::from_secs(100))
    } else {
        configured
    }
}

// A get_balance() call repeats the identical get_utxos_by_addresses RPC already
// performed by the scanner. Reuse that verified snapshot once per wallet instead
// of issuing a redundant RPC for every reward while the PNN is congested.
fn checked_snapshot_balance_sompi(amounts: impl IntoIterator<Item = u64>) -> Option<u64> {
    amounts
        .into_iter()
        .try_fold(0u64, |sum, amount| sum.checked_add(amount))
}

// Constrain each wallet to a deterministic, bounded batch per scan. Successfully
// enqueued outpoints are marked seen atomically by commit_alert_outbox; deferred
// outpoints remain unmarked and are retried on subsequent scans.
fn parse_reward_batch_limit(value: Option<&str>) -> usize {
    value
        .and_then(|s| s.parse::<usize>().ok())
        .unwrap_or(12)
        .clamp(1, 32)
}

fn reward_batch_limit() -> usize {
    let value = std::env::var("KASPA_REWARD_BATCH_LIMIT").ok();
    parse_reward_batch_limit(value.as_deref())
}

fn merge_scan_task_status(completed: bool, task_succeeded: bool) -> bool {
    completed && task_succeeded
}

fn truncate_reward_batch<T>(items: &mut Vec<T>, limit: usize) -> usize {
    let deferred = items.len().saturating_sub(limit);
    items.truncate(limit);
    deferred
}

// A preexisting proof is historical even while its Coinbase UTXO is unspent.
fn route_existing_proof_to_historical(
    seen_before: bool,
    persisted: &HashSet<String>,
    outpoint: &str,
) -> bool {
    !seen_before && persisted.contains(outpoint)
}

// OP063: keep the live lane reserved after ALL historical candidates are loaded.
fn fair_reward_lane_counts(fresh: usize, historical: usize, limit: usize) -> (usize, usize) {
    let limit = limit.max(1);
    let reserved_fresh = fresh.min(limit.div_ceil(2));
    let historical_first = historical.min(limit - reserved_fresh);
    let extra_fresh = (fresh - reserved_fresh).min(limit - reserved_fresh - historical_first);
    let total_fresh = reserved_fresh + extra_fresh;
    let total_historical = historical_first
        + (historical - historical_first).min(limit - total_fresh - historical_first);
    (total_fresh, total_historical)
}

static LAST_PROOF_BACKLOG_LOG_TS: AtomicU64 = AtomicU64::new(0);
static LAST_REWARD_BATCH_LOG_TS: AtomicU64 = AtomicU64::new(0);

// No timer, async task, or sleep: just suppress duplicate WARN lines.
// Metrics retain event counts for the complete interval.
fn periodic_warning_due(last: &AtomicU64) -> bool {
    let now = crate::infrastructure::metrics::now_unix_secs();
    let previous = last.load(Ordering::Relaxed);
    if now.saturating_sub(previous) < 60 {
        return false;
    }
    last.compare_exchange(previous, now, Ordering::Relaxed, Ordering::Relaxed)
        .is_ok()
}

// A fixed, owner-approved one-time migration boundary. Never use a moving
// clock/DAA cutoff: doing so would silently suppress future rewards.
fn parse_live_reward_min_daa_score(value: Option<&str>) -> Option<u64> {
    value
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|score| *score > 0)
}

fn live_reward_min_daa_score() -> Option<u64> {
    let value = std::env::var("KASPA_LIVE_REWARD_MIN_DAA_SCORE").ok();
    parse_live_reward_min_daa_score(value.as_deref())
}

// A verified on-chain UTXO has its own transaction identity. If archival DAG
// enrichment fails, never invent a mined/accepting block or worker name.
// The outbox still deduplicates by original wallet outpoint.
fn unresolved_dag_analysis() -> (String, Vec<String>, String, String, u64) {
    (String::new(), Vec::new(), String::new(), String::new(), 0)
}

fn valid_proof_hash(hash: &str) -> bool {
    hash.len() == 64 && hash.bytes().all(|byte| byte.is_ascii_hexdigit())
}

/// All user-requested block fields must be independently present before a
/// confirmed Coinbase UTXO can become a Telegram notification.
fn complete_mining_proof(
    is_coinbase: bool,
    accepting: &str,
    mined: Option<&str>,
    worker: &str,
    block_time_ms: u64,
) -> bool {
    !is_coinbase
        || (valid_proof_hash(accepting)
            && mined.is_some_and(valid_proof_hash)
            && !worker.trim().is_empty()
            && block_time_ms > 0)
}

fn new_reward_analysis_task_set<T: Send + 'static>() -> tokio::task::JoinSet<T> {
    tokio::task::JoinSet::new()
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
            known_utxos: WalletUtxoCache::default(),
            reward_analysis_gate: Arc::new(tokio::sync::Semaphore::new(
                reward_analysis_max_concurrency(),
            )),
        }
    }

    pub async fn check_wallet_utxos(
        &self,
        wallet_address: &str,
    ) -> Result<WalletUtxoScanResult, AppError> {
        crate::infrastructure::resilience::runtime::task_stage("rpc_get_utxos");
        let utxos = self.node.get_utxos(wallet_address).await?;
        let snapshot_balance_sompi =
            checked_snapshot_balance_sompi(utxos.iter().map(|utxo| utxo.amount))
                .ok_or_else(|| AppError::Internal("UTXO snapshot balance overflow".to_string()))?;

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

        crate::infrastructure::resilience::runtime::task_stage("db_get_seen_utxos");

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
        // One read-only snapshot per wallet scan; old unspent Coinbase rows
        // must go through the same bounded proof lane as old spent rows.
        let persisted_proof_outpoints =
            self.db.get_awaiting_proof_outpoints(wallet_address).await?;
        crate::infrastructure::resilience::runtime::task_stage("wallet_cache_lock");
        let mut known_mem = self.known_utxos.lock(wallet_address).await;
        crate::infrastructure::resilience::runtime::task_stage("reward_state_db_work");

        if known_mem.is_empty() && !known_db.is_empty() {
            for outpoint in &known_db {
                known_mem.insert(outpoint.clone());
            }
        }

        let is_first_run = known_mem.is_empty() && known_db.is_empty();
        let historical_cutover_daa = live_reward_min_daa_score();

        for utxo in utxos {
            current_outpoints.insert(utxo.outpoint.clone());

            let seen_before =
                known_mem.contains(&utxo.outpoint) || known_db.contains(&utxo.outpoint);
            if route_existing_proof_to_historical(
                seen_before,
                &persisted_proof_outpoints,
                &utxo.outpoint,
            ) {
                // Already durable: do not claim or re-upsert here; recovery lane
                // selects it fairly after fresh work is reserved.
                continue;
            }

            // Cutover authorized by owner: historical (pre-cutover) outputs
            // must not be re-analyzed forever. Persist their outpoints as seen
            // while keeping mined history, prior alerts, and dedup untouched.
            // Newly accepted UTXOs above the *fixed* cutoff are never skipped.
            if !seen_before
                && !is_first_run
                && historical_cutover_daa.is_some_and(|cutoff| utxo.block_daa_score <= cutoff)
            {
                current_outpoints_vec.push(utxo.outpoint.clone());
                known_mem.insert(utxo.outpoint.clone());
                known_db.insert(utxo.outpoint.clone());
                continue;
            }

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

                // The awaiting-proof row is written before contacting PNN.
                // This survives WSL restarts and allows retries if the UTXO
                // has been spent while enrichment was delayed.
                if utxo.is_coinbase {
                    match self
                        .db
                        .upsert_awaiting_block_proof(
                            wallet_address,
                            &utxo,
                            virtual_daa_score,
                            reward_confirmations,
                            min_reward_confirmations,
                        )
                        .await
                    {
                        Ok(true) => {}
                        Ok(false) => continue, // Already awaiting proof; bounded retry backoff
                        Err(error) => {
                            completed_without_errors = false;
                            tracing::error!(
                                "[PROOF BACKLOG] Could not durably save coinbase wallet={} tx={}: {}",
                                crate::utils::format_short_wallet(wallet_address),
                                crate::utils::format_short_wallet(&utxo.transaction_id),
                                error
                            );
                            continue;
                        }
                    }
                } else if let Err(error) = self
                    .db
                    .delete_pending_reward(wallet_address, &utxo.outpoint)
                    .await
                {
                    completed_without_errors = false;
                    tracing::warn!("[PROOF BACKLOG] Transfer pending cleanup failed: {}", error);
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

        crate::infrastructure::resilience::runtime::task_stage("db_upsert_seen_utxos");
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

        crate::infrastructure::resilience::runtime::task_stage("db_prune_seen_utxos");
        if let Err(e) = self
            .db
            .prune_seen_utxos(wallet_address, &current_outpoints_vec)
            .await
        {
            completed_without_errors = false;
            tracing::warn!("[DATABASE WARNING] Failed to prune seen UTXOs: {}", e);
        }

        // Fairness: reserve half of each bounded batch for old/spent proof
        // attempts. The other half processes newly confirmed rewards.
        let live_budget = reward_batch_limit().div_ceil(2);
        // Coinbase rewards already have a durable awaiting_block_proof record.
        // Postponing proof enrichment is NOT a failed wallet scan. Transfers
        // without a durable backlog remain a genuine incomplete scan.
        let unpersisted_deferred = new_rewards
            .iter()
            .skip(live_budget)
            .any(|reward| !reward.is_coinbase);
        if truncate_reward_batch(&mut new_rewards, live_budget) > 0 && unpersisted_deferred {
            completed_without_errors = false;
        }
        // Only strict-proof rows are eligible. Legacy pending rewards stay untouched.
        // Critically, DO NOT claim historical rows before allocating processing slots.
        let proof_backlog = self.db.get_awaiting_block_proof(wallet_address).await?;
        let mut historical_candidates = Vec::new();
        for reward in proof_backlog {
            if !known_mem.contains(&reward.outpoint)
                && !known_db.contains(&reward.outpoint)
                && !new_rewards
                    .iter()
                    .any(|new| new.outpoint == reward.outpoint)
            {
                historical_candidates.push(reward);
            }
        }
        // Fresh lane is ordered internally by DAA; archived lane preserves the
        // database's oldest-last-check order, independent of fresh lane.
        new_rewards.sort_unstable_by(|a, b| {
            a.block_daa_score
                .cmp(&b.block_daa_score)
                .then_with(|| a.outpoint.cmp(&b.outpoint))
        });
        let (fresh_count, historical_count) = fair_reward_lane_counts(
            new_rewards.len(),
            historical_candidates.len(),
            reward_batch_limit(),
        );
        let unpersisted_deferred = new_rewards
            .iter()
            .skip(fresh_count)
            .any(|reward| !reward.is_coinbase);
        let deferred = new_rewards.len().saturating_sub(fresh_count)
            + historical_candidates.len().saturating_sub(historical_count);
        new_rewards.truncate(fresh_count);
        historical_candidates.truncate(historical_count);

        let mut recovered_outpoints = std::collections::HashSet::new();
        for reward in historical_candidates {
            if self
                .db
                .claim_awaiting_block_proof(wallet_address, &reward.outpoint)
                .await?
            {
                recovered_outpoints.insert(reward.outpoint.clone());
                new_rewards.push(reward);
            }
        }

        if new_rewards.is_empty() {
            return Ok(WalletUtxoScanResult {
                events: Vec::new(),
                completed_without_errors,
            });
        }
        if deferred > 0 {
            completed_without_errors &= !unpersisted_deferred;
            crate::infrastructure::metrics::record_reward_batch_deferred(deferred as u64);
            if periodic_warning_due(&LAST_REWARD_BATCH_LOG_TS) {
                tracing::warn!(
                    wallet = %crate::utils::format_short_wallet(wallet_address),
                    processing = new_rewards.len(),
                    deferred,
                    "OP063 [REWARD BATCH] Deferred slots, fair live lane guaranteed; counters are in /metrics."
                );
            }
        }

        // Reward analysis is a child of this wallet scan. Keep the children
        // locally owned so a wallet-scan timeout/drop aborts them immediately
        // instead of detaching RPC/DB work under the global task supervisor.
        let mut join_set = new_reward_analysis_task_set();
        let reward_analysis_deadline = reward_analysis_timeout();

        for utxo in new_rewards {
            let recovered_proof_backlog = recovered_outpoints.contains(&utxo.outpoint);
            let analyzer = self.analyzer.clone();
            let db = self.db.clone();
            let wallet = wallet_address.to_string();
            let reward_analysis_gate = self.reward_analysis_gate.clone();

            join_set.spawn(async move {
                let _reward_analysis_permit = match reward_analysis_gate.acquire_owned().await {
                    Ok(permit) => permit,
                    Err(error) => {
                        tracing::error!(
                            "[WORKER] Reward analysis concurrency gate closed unexpectedly: {}",
                            error
                        );
                        return (None, false);
                    }
                };
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

                let route_deadline = proof_route_timeout(
                    reward_analysis_deadline,
                    recovered_proof_backlog && utxo.is_coinbase,
                );
                let analysis = tokio::time::timeout(
                    route_deadline,
                    analyzer.execute_with_archive_recovery(
                        &utxo.transaction_id,
                        &wallet,
                        utxo.block_daa_score,
                        utxo.is_coinbase,
                        recovered_proof_backlog,
                    ),
                )
                .await;

                let (acc_block_hash, actual_mined_blocks, _nonce, extracted_worker, block_time_ms) =
                    match analysis {
                        Ok(Ok(data)) => data,
                        Ok(Err(e)) if e.to_string().contains("retry deferred") => {
                            tracing::debug!(
                                "[HISTORICAL PROOF] Bounded archive lookup deferred; proof remains in PostgreSQL"
                            );
                            unresolved_dag_analysis()
                        }
                        Ok(Err(e)) => {
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

                            unresolved_dag_analysis()
                        }
                        Err(_) => {
                            let wallet_masked = crate::utils::format_short_wallet(&wallet);
                            let txid_masked =
                                crate::utils::format_short_wallet(&utxo.transaction_id);
                            let error_text = format!(
                                "DAG analysis timed out after {} seconds",
                                route_deadline.as_secs()
                            );

                            let mut rpc_error_event =
                                BotEventRecord::new(BotEventType::RpcError, EventSeverity::Error);
                            rpc_error_event.wallet_masked = Some(&wallet_masked);
                            rpc_error_event.txid_masked = Some(&txid_masked);
                            rpc_error_event.status = Some("dag_analysis_timeout");
                            rpc_error_event.error_message = Some(&error_text);

                            let _ = db.record_bot_event_record(rpc_error_event).await;

                            tracing::error!(
                                "[DAG TIMEOUT] Reward analysis exceeded {}s. tx={} wallet={}",
                                route_deadline.as_secs(),
                                txid_masked,
                                wallet_masked
                            );

                            unresolved_dag_analysis()
                        }
                    };

                let mined_block_hash = actual_mined_blocks.first().cloned();
                if !complete_mining_proof(
                    utxo.is_coinbase, &acc_block_hash,
                    mined_block_hash.as_deref(), &extracted_worker, block_time_ms
                ) {
                    crate::infrastructure::metrics::record_proof_incomplete();
                    if periodic_warning_due(&LAST_PROOF_BACKLOG_LOG_TS) {
                        tracing::warn!(
                            "[PROOF BACKLOG] Incomplete verified proof, no Telegram alert enqueued; see /metrics. wallet={} tx={}",
                            crate::utils::format_short_wallet(&wallet),
                            crate::utils::format_short_wallet(&utxo.transaction_id),
                        );
                    }
                    // Not a failed scan. Deferred alerts remain in PostgreSQL
                    // and are resolved by subsequent in-process scan triggers.
                    return (None, true);
                }
                let alert_key = crate::wallet::alert_dedup::build_alert_key(
                    mined_block_hash.as_deref(),
                    &utxo.transaction_id,
                );

                let live_balance = snapshot_balance_sompi;

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
                Ok((None, task_succeeded)) => {
                    // A successfully persisted, intentionally deferred proof is
                    // not a scanner failure. DB/RPC errors flagged by the task
                    // still fail closed through task_succeeded.
                    completed_without_errors =
                        merge_scan_task_status(completed_without_errors, task_succeeded);
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

#[cfg(test)]
mod lifecycle_tests {
    use super::*;
    use std::sync::{
        atomic::{AtomicBool, AtomicUsize, Ordering},
        mpsc,
    };
    use std::time::Duration;

    struct AbortProbe(Arc<AtomicBool>);

    impl Drop for AbortProbe {
        fn drop(&mut self) {
            self.0.store(true, Ordering::SeqCst);
        }
    }

    // The owner runs on a separate runtime so the pre-fix starvation can be
    // released and all test threads joined without killing a process.
    #[test]
    fn utxo_cache_contention_does_not_starve_runtime_health_or_cancellation() {
        let cache = Arc::new(WalletUtxoCache::default());
        for n in 0..512 {
            cache
                .wallets
                .entry(format!("synthetic_wallet_{n}"))
                .or_default();
        }
        let owner_key = "synthetic_wallet_0".to_string();
        let collisions = {
            let _held = cache.wallets.get_mut(&owner_key).unwrap();
            (1..512)
                .map(|n| format!("synthetic_wallet_{n}"))
                .filter(|key| cache.wallets.try_get(key).is_locked())
                .take(2)
                .collect::<Vec<_>>()
        };
        assert_eq!(
            collisions.len(),
            2,
            "fixture must exercise the same DashMap shard"
        );
        let (held_tx, held_rx) = mpsc::channel();
        let (release_tx, release_rx) = tokio::sync::oneshot::channel();
        let owner_cache = cache.clone();
        let owner = std::thread::spawn(move || {
            tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
                .unwrap()
                .block_on(async move {
                    let _held = owner_cache.lock(&owner_key).await;
                    held_tx.send(()).unwrap();
                    let _ = release_rx.await;
                });
        });
        held_rx.recv_timeout(Duration::from_secs(2)).unwrap();

        let attempted = Arc::new(AtomicUsize::new(0));
        let (probe_tx, probe_rx) = tokio::sync::oneshot::channel();
        let (healthy_tx, healthy_rx) = mpsc::channel();
        let attempts = attempted.clone();
        let runtime = std::thread::spawn(move || {
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .unwrap()
                .block_on(async move {
                    let health = tokio::spawn(async move {
                        let _ = probe_rx.await;
                        healthy_tx.send(()).unwrap();
                    });
                    let mut scans = Vec::new();
                    for key in collisions {
                        let cache = cache.clone();
                        let attempts = attempts.clone();
                        scans.push(tokio::spawn(async move {
                            attempts.fetch_add(1, Ordering::SeqCst);
                            let _wallet_state = cache.lock(&key).await;
                        }));
                    }
                    for scan in scans {
                        scan.await.unwrap();
                    }
                    health.await.unwrap();
                });
        });
        let deadline = std::time::Instant::now() + Duration::from_secs(2);
        while attempted.load(Ordering::SeqCst) != 2 && std::time::Instant::now() < deadline {
            std::thread::yield_now();
        }
        assert_eq!(attempted.load(Ordering::SeqCst), 2);
        probe_tx.send(()).unwrap();
        let responsive = healthy_rx.recv_timeout(Duration::from_millis(250)).is_ok();
        eprintln!(
            "cache ownership: owner=awaiting independent release; contenders=2 distinct wallets/same shard; health_responsive={responsive}"
        );
        release_tx.send(()).unwrap();
        owner.join().unwrap();
        runtime.join().unwrap();
        assert!(
            responsive,
            "UTXO cache lock blocked both Tokio workers and the health/cancellation executor"
        );
    }

    #[test]
    fn op063_existing_unspent_proof_cannot_reenter_fresh_reward_lane() {
        let existing = std::collections::HashSet::from(["old-outpoint".to_string()]);
        assert!(route_existing_proof_to_historical(
            false,
            &existing,
            "old-outpoint"
        ));
        assert!(!route_existing_proof_to_historical(
            true,
            &existing,
            "old-outpoint"
        ));
        assert!(!route_existing_proof_to_historical(
            false,
            &existing,
            "brand-new-outpoint"
        ));
        // The live scan redirects any persisted proof row even while it
        // remains in the node's UTXO set; previously the same row re-entered
        // as a fresh reward after the 45s backoff.
    }

    #[test]
    fn op063_live_lane_survives_historical_daa_sort_and_claims_only_selected() {
        assert_eq!(fair_reward_lane_counts(5, 6, 2), (1, 1));
        assert_eq!(fair_reward_lane_counts(1, 72, 2), (1, 1));
        assert_eq!(fair_reward_lane_counts(2, 6, 1), (1, 0));
        assert_eq!(fair_reward_lane_counts(0, 72, 2), (0, 2));
        assert_eq!(fair_reward_lane_counts(7, 0, 2), (2, 0));
        assert_eq!(fair_reward_lane_counts(1, 0, 2), (1, 0));
        assert_eq!(fair_reward_lane_counts(10, 3, 3), (2, 1));
        assert_eq!(fair_reward_lane_counts(0, 0, 2), (0, 0));
    }

    #[test]
    fn deferred_durable_proof_does_not_falsely_mark_scan_failed() {
        assert!(merge_scan_task_status(true, true));
        assert!(!merge_scan_task_status(true, false));
        assert!(!merge_scan_task_status(false, true));
        // Even if no Telegram message was produced, persisted proof is a
        // successful scan for connectivity/readiness purposes.
        let mut saved_proofs = vec![1, 2, 3, 4];
        let deferred = truncate_reward_batch(&mut saved_proofs, 2);
        assert_eq!(deferred, 2);
        assert!(merge_scan_task_status(true, true));
    }

    #[test]
    fn strict_mining_proof_refuses_every_incomplete_coinbase_event() {
        let a = "a".repeat(64);
        let b = "b".repeat(64);
        assert!(!complete_mining_proof(true, "", None, "", 0));
        assert!(!complete_mining_proof(true, &a, Some(&b), "", 12345));
        assert!(!complete_mining_proof(true, &a, None, "worker", 12345));
        assert!(!complete_mining_proof(true, &a, Some(&b), "worker", 0));
        assert!(!complete_mining_proof(
            true,
            "not-a-hash",
            Some(&b),
            "worker",
            12345
        ));
        assert!(complete_mining_proof(true, &a, Some(&b), "worker-1", 12345));
        assert!(complete_mining_proof(false, "", None, "", 0));
    }

    #[test]
    fn reward_analysis_concurrency_is_bounded_and_has_safe_default() {
        assert_eq!(parse_reward_analysis_max_concurrency(None), 8);
        assert_eq!(parse_reward_analysis_max_concurrency(Some("0")), 1);
        assert_eq!(parse_reward_analysis_max_concurrency(Some("1")), 1);
        assert_eq!(parse_reward_analysis_max_concurrency(Some("8")), 8);
        assert_eq!(parse_reward_analysis_max_concurrency(Some("32")), 32);
        assert_eq!(parse_reward_analysis_max_concurrency(Some("999")), 32);
        assert_eq!(parse_reward_analysis_max_concurrency(Some("invalid")), 8);
    }

    #[test]
    fn reward_analysis_timeout_is_bounded_and_has_safe_default() {
        assert_eq!(parse_reward_analysis_timeout(None), Duration::from_secs(10));
        assert_eq!(
            parse_reward_analysis_timeout(Some("0")),
            Duration::from_secs(5)
        );
        assert_eq!(
            parse_reward_analysis_timeout(Some("30")),
            Duration::from_secs(30)
        );
        assert_eq!(
            parse_reward_analysis_timeout(Some("999")),
            Duration::from_secs(120)
        );
        assert_eq!(
            parse_reward_analysis_timeout(Some("invalid")),
            Duration::from_secs(10)
        );
    }

    #[test]
    fn verified_utxo_snapshot_balance_matches_adapter_sum_without_repeated_rpc() {
        assert_eq!(checked_snapshot_balance_sompi([100, 200, 300]), Some(600));
        assert_eq!(checked_snapshot_balance_sompi([]), Some(0));
        assert_eq!(checked_snapshot_balance_sompi([u64::MAX, 1]), None);
    }

    #[test]
    fn bounded_reward_batch_defers_without_losing_unprocessed_items() {
        assert_eq!(parse_reward_batch_limit(None), 12);
        assert_eq!(parse_reward_batch_limit(Some("0")), 1);
        assert_eq!(parse_reward_batch_limit(Some("999")), 32);
        assert_eq!(parse_reward_batch_limit(Some("invalid")), 12);
        let mut rewards: Vec<usize> = (1..=29).collect();
        assert_eq!(truncate_reward_batch(&mut rewards, 12), 17);
        assert_eq!(rewards, (1..=12).collect::<Vec<_>>());
        assert_eq!(truncate_reward_batch(&mut rewards, 12), 0);
    }

    #[tokio::test]
    async fn reward_analysis_children_are_aborted_with_wallet_scan_owner() {
        let dropped = Arc::new(AtomicBool::new(false));
        let probe = dropped.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel();

        let mut tasks = new_reward_analysis_task_set();
        tasks.spawn(async move {
            let _probe = AbortProbe(probe);
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
        });

        started_rx.await.unwrap();
        drop(tasks);

        tokio::time::timeout(Duration::from_secs(1), async {
            while !dropped.load(Ordering::SeqCst) {
                tokio::task::yield_now().await;
            }
        })
        .await
        .expect("dropping wallet-owned reward tasks must abort blocked children");
    }
}

#[cfg(test)]
mod historical_reward_cutover_tests {
    use super::parse_live_reward_min_daa_score;

    #[test]
    fn cutover_is_fixed_and_requires_valid_positive_daa() {
        assert_eq!(parse_live_reward_min_daa_score(None), None);
        assert_eq!(parse_live_reward_min_daa_score(Some("")), None);
        assert_eq!(parse_live_reward_min_daa_score(Some("0")), None);
        assert_eq!(
            parse_live_reward_min_daa_score(Some("579682005")),
            Some(579682005)
        );
        assert_eq!(
            parse_live_reward_min_daa_score(Some("not-an-integer")),
            None
        );
    }
}

#[cfg(test)]
mod dag_payout_fallback_tests {
    use super::unresolved_dag_analysis;

    #[test]
    fn unverified_block_details_are_never_fabricated() {
        let (accepting, mined, nonce, worker, time) = unresolved_dag_analysis();
        assert!(accepting.is_empty());
        assert!(mined.is_empty());
        assert!(nonce.is_empty());
        assert!(worker.is_empty());
        assert_eq!(time, 0);
    }
}

#[cfg(test)]
mod op070_proof_route_deadline_tests {
    use super::*;
    use std::time::Duration;
    #[test]
    fn already_durable_coinbase_waits_for_fair_archive_admission() {
        assert_eq!(
            proof_route_timeout(Duration::from_secs(45), true),
            Duration::from_secs(100)
        );
        assert_eq!(
            proof_route_timeout(Duration::from_secs(120), true),
            Duration::from_secs(120)
        );
    }
    #[test]
    fn fresh_reward_and_transfer_deadlines_are_not_widened() {
        assert_eq!(
            proof_route_timeout(Duration::from_secs(45), false),
            Duration::from_secs(45)
        );
        assert_eq!(
            proof_route_timeout(Duration::from_secs(10), false),
            Duration::from_secs(10)
        );
    }
}
