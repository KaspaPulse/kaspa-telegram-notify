use crate::domain::entities::TrackedWallet;
use crate::domain::models::{BotEventType, EventSeverity};
use crate::infrastructure::database::postgres_adapter::PostgresRepository;
use crate::infrastructure::node::kaspa_adapter::KaspaRpcAdapter;
use crate::infrastructure::node::subscription::{
    MonitoringMode, MonitoringSchedule, spawn_subscription_runtime,
};
use crate::infrastructure::telegram_delivery_queue::{
    AlertOutboxOutcome, AlertOutboxRequest, commit_alert_outbox,
};
use chrono::{TimeZone, Utc};
use std::collections::HashMap;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;
use teloxide::prelude::*;
use tokio::sync::Semaphore;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

use crate::network::analyze_dag::AnalyzeDagUseCase;
use crate::wallet::wallet_use_cases::UtxoMonitorService;

pub(crate) fn group_wallet_subscribers(wallets: Vec<TrackedWallet>) -> HashMap<String, Vec<i64>> {
    let mut recipients_by_wallet: HashMap<String, Vec<i64>> = HashMap::new();

    for wallet in wallets {
        recipients_by_wallet
            .entry(wallet.address)
            .or_default()
            .push(wallet.chat_id);
    }

    for chat_ids in recipients_by_wallet.values_mut() {
        chat_ids.sort_unstable();
        chat_ids.dedup();
    }

    recipients_by_wallet
}

pub fn start_utxo_monitor(
    _bot: Bot,
    node: Arc<KaspaRpcAdapter>,
    db: Arc<PostgresRepository>,
    live_sync_enabled: Arc<AtomicBool>,
    token: CancellationToken,
) {
    let analyzer = Arc::new(AnalyzeDagUseCase::new(node.clone()));
    let utxo_service = Arc::new(UtxoMonitorService::new(node.clone(), db.clone(), analyzer));
    let semaphore = Arc::new(Semaphore::new(10));
    let mode = MonitoringMode::from_env();
    let poll_interval = Duration::from_secs(crate::infrastructure::resilience::runtime::env_u64(
        "KASPA_MONITOR_POLL_INTERVAL_SECS",
        10,
    ));
    let reconciliation_interval =
        Duration::from_secs(crate::infrastructure::resilience::runtime::env_u64(
            "KASPA_MONITOR_RECONCILIATION_INTERVAL_SECS",
            60,
        ));
    let subscription_minimum_interval =
        Duration::from_secs(crate::infrastructure::resilience::runtime::env_u64(
            "KASPA_SUBSCRIPTION_MIN_SCAN_INTERVAL_SECS",
            2,
        ));
    let (mut schedule, signal_sender, lifecycle) = MonitoringSchedule::new(
        mode,
        poll_interval,
        reconciliation_interval,
        subscription_minimum_interval,
    );

    spawn_subscription_runtime(node.client.clone(), signal_sender, lifecycle, token.clone());

    crate::infrastructure::resilience::runtime::spawn_resilient("utxo_monitor_task", async move {
        info!("🚀 [WORKER] UTXO monitor started.");

        info!(
            "[WORKER] UTXO monitor mode={} poll={}s reconciliation={}s.",
            mode.as_str(),
            poll_interval.as_secs(),
            reconciliation_interval.as_secs()
        );

        loop {
            crate::infrastructure::resilience::runtime::task_stage("waiting_scan_trigger");
            let Some(trigger) = schedule.next_trigger(&token).await else {
                info!("[WORKER] UTXO monitor shutdown requested.");
                break;
            };

            if !live_sync_enabled.load(Ordering::Relaxed) {
                tracing::debug!(
                    "[WORKER] UTXO scan skipped while live monitoring is paused. trigger={}",
                    trigger.as_str()
                );
                continue;
            }

            crate::infrastructure::observability::record_scan_trigger(trigger);
            tracing::debug!("[WORKER] UTXO scan triggered by {}.", trigger.as_str());

            crate::infrastructure::resilience::runtime::task_stage("rpc_node_health");

            match node.get_node_health().await {
                Ok((true, _)) => {
                    crate::infrastructure::observability::set_node_connected(true);
                }
                Ok((false, _)) | Err(_) => {
                    crate::infrastructure::observability::set_node_connected(false);
                    continue;
                }
            }

            crate::infrastructure::resilience::runtime::task_stage("db_tracked_wallets");

            let wallets = match db.get_all_tracked_wallets().await {
                Ok(wallets) => wallets,
                Err(error) => {
                    error!("[DATABASE ERROR] Failed to fetch wallets: {}", error);
                    continue;
                }
            };

            if wallets.is_empty() {
                mark_scan_success();
                continue;
            }

            let recipients_by_wallet = group_wallet_subscribers(wallets);
            let mut join_set =
                crate::infrastructure::resilience::runtime::OwnedTaskSet::new("utxo_wallet_scan");

            for (wallet_address, chat_ids) in recipients_by_wallet {
                let semaphore = semaphore.clone();
                let service = utxo_service.clone();
                let db = db.clone();

                join_set.spawn(async move {
                    let _permit = match semaphore.acquire_owned().await {
                        Ok(permit) => permit,
                        Err(error) => {
                            error!(
                                "Failed to acquire a UTXO scan permit for {}: {}",
                                wallet_address, error
                            );
                            return false;
                        }
                    };

                    let scan_result = match service.check_wallet_utxos(&wallet_address).await {
                        Ok(scan_result) => scan_result,
                        Err(error) => {
                            error!("Failed to check UTXOs for {}: {}", wallet_address, error);
                            return false;
                        }
                    };
                    let mut wallet_scan_succeeded = scan_result.completed_without_errors;

                    for event in scan_result.events {
                        let log_time = if event.block_time_ms > 0 {
                            Utc.timestamp_millis_opt(event.block_time_ms as i64)
                                .single()
                                .map(|datetime| datetime.format("%H:%M:%S.%3f").to_string())
                                .unwrap_or_else(|| "Unknown".to_string())
                        } else {
                            "Real-time".to_string()
                        };

                        let message = crate::presentation::telegram::formatting::events_formatter::format_live_event(&event);
                        let wallet_masked = crate::utils::format_short_wallet(&event.wallet_address);
                        let txid_masked = crate::utils::format_short_wallet(&event.tx_id);
                        let block_masked = event
                            .mined_block_hash
                            .as_ref()
                            .map(|hash| crate::utils::format_short_wallet(hash));

                        info!(
                            "💎 [LIVE BLOCK] | Amount: +{:.4} KAS | Wallet: {} | Time: {} | Recipients: {}",
                            event.amount_kas,
                            wallet_masked,
                            log_time,
                            chat_ids.len()
                        );

                        let request = AlertOutboxRequest {
                            wallet: &event.wallet_address,
                            source_outpoint: &event.source_outpoint,
                            alert_key: &event.alert_key,
                            message_html: &message,
                            chat_ids: &chat_ids,
                            wallet_masked: Some(&wallet_masked),
                            txid_masked: Some(&txid_masked),
                            block_hash_masked: block_masked.as_deref(),
                            amount_kas: Some(event.amount_kas),
                            daa_score: Some(event.daa_score as i64),
                        };

                        match commit_alert_outbox(&db.pool, request).await {
                            Ok(AlertOutboxOutcome::Enqueued { recipients }) => {
                                crate::infrastructure::metrics::mark_alert_detected();

                                info!(
                                    "📥 [ALERT OUTBOX COMMITTED] Wallet: {} | Recipients: {}",
                                    wallet_masked,
                                    recipients
                                );

                                let _ = db
                                    .record_bot_event_typed(
                                        BotEventType::AlertDetected,
                                        EventSeverity::Info,
                                        None,
                                        None,
                                        None,
                                        None,
                                        Some(&wallet_masked),
                                        Some(&txid_masked),
                                        block_masked.as_deref(),
                                        Some("outbox_committed"),
                                        None,
                                        None,
                                        &format!(
                                            r#"{{"amount_kas":{},"recipients":{},"daa_score":{},"outpoint":"{}"}}"#,
                                            event.amount_kas,
                                            recipients,
                                            event.daa_score,
                                            crate::utils::format_short_wallet(&event.source_outpoint)
                                        ),
                                    )
                                    .await;
                            }
                            Ok(AlertOutboxOutcome::Reconciled { recipients }) => {
                                warn!(
                                    "[ALERT OUTBOX RECONCILED] Wallet: {} | Restored recipients: {}",
                                    wallet_masked,
                                    recipients
                                );
                            }
                            Ok(AlertOutboxOutcome::Duplicate) => {
                                info!(
                                    "[ALERT DUPLICATE SKIPPED] Wallet: {} | TX: {}",
                                    wallet_masked,
                                    txid_masked
                                );

                                let _ = db
                                    .record_bot_event_typed(
                                        BotEventType::AlertDuplicateSkipped,
                                        EventSeverity::Info,
                                        None,
                                        None,
                                        None,
                                        None,
                                        Some(&wallet_masked),
                                        Some(&txid_masked),
                                        block_masked.as_deref(),
                                        Some("duplicate_skipped"),
                                        None,
                                        None,
                                        "{}",
                                    )
                                    .await;
                            }
                            Ok(AlertOutboxOutcome::Suppressed { recipients }) => {
                                for _ in 0..recipients {
                                    crate::infrastructure::metrics::inc_alerts_suppressed();
                                }

                                info!(
                                    "🔕 [ALERT SUPPRESSED] Wallet: {} | Recipients: {}",
                                    wallet_masked,
                                    recipients
                                );

                                let _ = db
                                    .record_bot_event_typed(
                                        BotEventType::AlertDeliverySuppressed,
                                        EventSeverity::Info,
                                        None,
                                        None,
                                        None,
                                        None,
                                        Some(&wallet_masked),
                                        Some(&txid_masked),
                                        block_masked.as_deref(),
                                        Some("suppressed"),
                                        None,
                                        None,
                                        &format!(
                                            r#"{{"amount_kas":{},"recipients":{},"daa_score":{},"reason":"alert_delivery_disabled"}}"#,
                                            event.amount_kas,
                                            recipients,
                                            event.daa_score
                                        ),
                                    )
                                    .await;
                            }
                            Err(error) => {
                                wallet_scan_succeeded = false;
                                crate::infrastructure::metrics::inc_db_errors();
                                crate::infrastructure::observability::increment_outbox_failures();
                                let error_text = error.to_string();

                                error!(
                                    "[ALERT OUTBOX] Atomic commit failed. Wallet: {} | TX: {} | Error: {}. The event will be retried on the next scan.",
                                    wallet_masked,
                                    txid_masked,
                                    error_text
                                );

                                let _ = db
                                    .record_bot_event_typed(
                                        BotEventType::DbError,
                                        EventSeverity::Error,
                                        None,
                                        None,
                                        None,
                                        None,
                                        Some(&wallet_masked),
                                        Some(&txid_masked),
                                        block_masked.as_deref(),
                                        Some("alert_outbox_commit_failed"),
                                        Some(&error_text),
                                        None,
                                        r#"{"operation":"commit_alert_outbox","action":"retry_next_scan"}"#,
                                    )
                                    .await;
                            }
                        }
                    }

                    wallet_scan_succeeded
                });
            }

            let mut wallet_scan_outcomes = Vec::new();
            crate::infrastructure::resilience::runtime::task_stage("joining_wallet_scans");
            while let Some(result) = join_set.join_next().await {
                match result {
                    Ok(succeeded) => wallet_scan_outcomes.push(succeeded),
                    Err(error) => {
                        error!("[WORKER] UTXO wallet scan task failed to join: {}", error);
                        wallet_scan_outcomes.push(false);
                    }
                }
            }

            if all_wallet_scan_tasks_succeeded(&wallet_scan_outcomes) {
                mark_scan_success();
            } else {
                warn!(
                    "[WORKER] UTXO scan completed with wallet failures; successful-scan freshness was not advanced."
                );
            }
        }
    });
}

fn all_wallet_scan_tasks_succeeded(outcomes: &[bool]) -> bool {
    outcomes.iter().all(|succeeded| *succeeded)
}

fn mark_scan_success() {
    let now = crate::infrastructure::metrics::now_unix_secs();
    crate::infrastructure::metrics::mark_utxo_scan();
    crate::infrastructure::observability::mark_successful_scan(now);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn wallet(address: &str, chat_id: i64) -> TrackedWallet {
        TrackedWallet {
            address: address.to_string(),
            chat_id,
        }
    }

    #[test]
    fn groups_same_wallet_for_multiple_chats() {
        let grouped = group_wallet_subscribers(vec![
            wallet("kaspa:wallet_a", 484901117),
            wallet("kaspa:wallet_a", 1307244272),
            wallet("kaspa:wallet_a", 1792588801),
        ]);

        let subscribers = grouped.get("kaspa:wallet_a").expect("wallet_a exists");

        assert_eq!(subscribers, &vec![484901117, 1307244272, 1792588801]);
    }

    #[test]
    fn deduplicates_duplicate_chat_ids_for_same_wallet() {
        let grouped = group_wallet_subscribers(vec![
            wallet("kaspa:wallet_a", 484901117),
            wallet("kaspa:wallet_a", 484901117),
            wallet("kaspa:wallet_a", 1307244272),
        ]);

        let subscribers = grouped.get("kaspa:wallet_a").expect("wallet_a exists");

        assert_eq!(subscribers, &vec![484901117, 1307244272]);
    }

    #[test]
    fn keeps_different_wallets_separate() {
        let grouped = group_wallet_subscribers(vec![
            wallet("kaspa:wallet_a", 1),
            wallet("kaspa:wallet_b", 2),
            wallet("kaspa:wallet_a", 3),
        ]);

        assert_eq!(grouped.get("kaspa:wallet_a"), Some(&vec![1, 3]));
        assert_eq!(grouped.get("kaspa:wallet_b"), Some(&vec![2]));
    }

    #[test]
    fn successful_scan_requires_every_wallet_task_to_succeed() {
        assert!(all_wallet_scan_tasks_succeeded(&[]));
        assert!(all_wallet_scan_tasks_succeeded(&[true, true]));
        assert!(!all_wallet_scan_tasks_succeeded(&[true, false]));
        assert!(!all_wallet_scan_tasks_succeeded(&[false]));
    }
}
