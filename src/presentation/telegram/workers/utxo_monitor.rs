use crate::domain::entities::TrackedWallet;
use crate::domain::models::{BotEventType, EventSeverity};
use crate::infrastructure::database::postgres_adapter::PostgresRepository;
use crate::infrastructure::node::failover::{
    FailoverPolicy, NodeObservation, Selection, UtxoFailover, UtxoNode,
};
use crate::infrastructure::node::kaspa_adapter::KaspaRpcAdapter;
use crate::infrastructure::node::subscription::{
    MonitoringMode, MonitoringSchedule, spawn_subscription_runtime,
};
use crate::infrastructure::telegram_delivery_queue::{
    AlertOutboxOutcome, AlertOutboxRequest, commit_alert_outbox_localized,
};
use chrono::{TimeZone, Utc};
use kaspa_rpc_core::api::rpc::RpcApi;
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

/// Read-only, bounded preflight for UTXO scanner. Never changes the underlying node.
async fn monitor_node_observation(node: &KaspaRpcAdapter) -> Option<NodeObservation> {
    if !node.client.is_connected()
        && !matches!(
            tokio::time::timeout(Duration::from_secs(5), node.connect(false)).await,
            Ok(Ok(()))
        )
    {
        return None;
    }
    let server = tokio::time::timeout(Duration::from_secs(5), node.get_server_info())
        .await
        .ok()?
        .ok()?;
    let peers = tokio::time::timeout(
        Duration::from_secs(5),
        node.client.get_connected_peer_info(),
    )
    .await
    .ok()?
    .ok()?
    .peer_info
    .len();
    Some(NodeObservation {
        is_mainnet: server.network_id.to_string() == "mainnet",
        is_synced: server.is_synced,
        has_utxo_index: server.has_utxo_index,
        connected_peers: peers,
        virtual_daa_score: server.virtual_daa_score,
    })
}

pub fn start_utxo_monitor(
    _bot: Bot,
    node: Arc<KaspaRpcAdapter>,
    // Optional node used exclusively by the wallet UTXO scan and reward outbox.
    standby: Option<Arc<KaspaRpcAdapter>>,
    db: Arc<PostgresRepository>,
    live_sync_enabled: Arc<AtomicBool>,
    token: CancellationToken,
    price_context: Arc<crate::domain::models::AppContext>,
) {
    let analyzer = Arc::new(AnalyzeDagUseCase::with_db(node.clone(), db.clone()));
    let utxo_service = Arc::new(UtxoMonitorService::new(node.clone(), db.clone(), analyzer));
    let standby_service = standby.as_ref().map(|alternate| {
        Arc::new(UtxoMonitorService::new(
            alternate.clone(),
            db.clone(),
            Arc::new(AnalyzeDagUseCase::with_db(alternate.clone(), db.clone())),
        ))
    });
    // Production-owned in-process VSPC indexer: one bounded PNN read per
    // interval. Never creates a Windows Task Scheduler/cron/timer unit.
    let indexer_node = node.clone();
    let indexer_db = db.clone();
    let indexer_cancel = token.clone();
    crate::infrastructure::resilience::runtime::spawn_resilient("pnn_vspc_indexer", async move {
        crate::network::vspc_proof::run_indexer(indexer_node, indexer_db, indexer_cancel).await;
    });
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
        let mut failover = UtxoFailover::new(FailoverPolicy::default());
        let started = tokio::time::Instant::now();
        info!("🚀 [WORKER] UTXO monitor started.");

        info!(
            "[WORKER] UTXO monitor mode={} poll={}s reconciliation={}s.",
            mode.as_str(),
            poll_interval.as_secs(),
            reconciliation_interval.as_secs()
        );

        loop {
            crate::infrastructure::resilience::runtime::task_stage("waiting_scan_trigger");
            let Some(trigger) = schedule
                .next_trigger_with_forced_poll(&token, standby.is_some())
                .await
            else {
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

            let selected_service = if let Some(standby_node) = standby.as_ref() {
                let primary_status = monitor_node_observation(&node).await;
                // Health-check the standby only if the primary is failing or we
                // already operate on the standby, avoiding extra steady-state RPC load.
                let need_standby = failover.should_probe_standby(primary_status);
                let same_endpoint = node.client.url().is_some_and(|primary_url| {
                    standby_node.client.url().as_deref() == Some(primary_url.as_str())
                });
                let standby_status = if need_standby && !same_endpoint {
                    monitor_node_observation(standby_node).await
                } else {
                    if same_endpoint {
                        warn!("[PNN STANDBY] URL collision, refusing alternate source.");
                    }
                    None
                };
                let decision = failover.choose(started.elapsed(), primary_status, standby_status);
                if let Some((old, new)) = decision.transition {
                    info!(from = ?old, to = ?new,
                        "[UTXO FAILOVER] Selected a single eligible public PNN UTXO source.");
                }
                match decision.selection {
                    Selection::Ready(UtxoNode::DnsPrimary) => Some(utxo_service.clone()),
                    Selection::Ready(UtxoNode::KasStandby) => standby_service.clone(),
                    Selection::Pause => None,
                }
            } else {
                // Default behavior is intentionally unchanged until the isolated
                // standby connection has passed private transport and E2E gates.
                match node.get_node_health().await {
                    Ok((true, _)) => Some(utxo_service.clone()),
                    Ok((false, _)) | Err(_) => None,
                }
            };
            let Some(selected_service) = selected_service else {
                crate::infrastructure::observability::set_node_connected(false);
                continue;
            };
            crate::infrastructure::observability::set_node_connected(true);

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
            // The scan owns its children: aborting or timing out this JoinSet
            // also cancels their nested reward-analysis tasks, avoiding an
            // unbounded health/failback stall under a slow DAG RPC.
            let mut join_set = tokio::task::JoinSet::new();

            for (wallet_address, chat_ids) in recipients_by_wallet {
                let semaphore = semaphore.clone();
                let service = selected_service.clone();
                let db = db.clone();
                let price_context = price_context.clone();

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

                        let price_usd = price_context.current_kas_usd_price().await;
                        let message = crate::presentation::telegram::formatting::events_formatter::format_live_event(&event, price_usd);
                        let message_ar = crate::presentation::telegram::formatting::events_formatter::format_live_event_lang(
                            &event,
                            price_usd,
                            crate::presentation::telegram::menus::Language::Ar,
                        );
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
                            txid_masked: Some(&txid_masked),
                            block_hash_masked: block_masked.as_deref(),
                            amount_kas: Some(event.amount_kas),
                            daa_score: Some(event.daa_score as i64),
                        };

                        let outbox_result = commit_alert_outbox_localized(
                            &db.pool,
                            request,
                            Some(&message_ar),
                        ).await;
                        let can_clear_proof = matches!(
                            &outbox_result,
                            Ok(AlertOutboxOutcome::Enqueued { .. })
                            | Ok(AlertOutboxOutcome::Reconciled { .. })
                            | Ok(AlertOutboxOutcome::Duplicate)
                            | Ok(AlertOutboxOutcome::Suppressed { .. })
                            | Ok(AlertOutboxOutcome::NoCurrentRecipients)
                        );
                        match outbox_result {
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
                            Ok(AlertOutboxOutcome::NoCurrentRecipients) => {
                                info!(wallet = %wallet_masked,
                                    "[STALE ALERT SKIPPED] Original recipients no longer subscribe; no outbox state was written.");
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
                        // The outbox transaction already persists the seen outpoint.
                        // Failure to clear this row is safe: repeated scans skip seen
                        // outpoints and the event-key constraint prevents replay.
                        if can_clear_proof && event.is_coinbase
                            && let Err(error) = db.delete_pending_reward(
                                &event.wallet_address, &event.source_outpoint
                            ).await
                        {
                            warn!(
                                "[PROOF BACKLOG] Committed alert but stale proof row cleanup failed: {}",
                                error
                            );
                        }
                    }

                    wallet_scan_succeeded
                });
            }

            let mut wallet_scan_outcomes = Vec::new();
            crate::infrastructure::resilience::runtime::task_stage("joining_wallet_scans");
            let timeout_secs =
                bounded_wallet_scan_secs(crate::infrastructure::resilience::runtime::env_u64(
                    "KASPA_WALLET_SCAN_TIMEOUT_SECS",
                    120,
                ));
            let drained = tokio::time::timeout(Duration::from_secs(timeout_secs), async {
                while let Some(result) = join_set.join_next().await {
                    match result {
                        Ok(succeeded) => wallet_scan_outcomes.push(succeeded),
                        Err(error) => {
                            error!("[WORKER] UTXO wallet scan task failed to join: {}", error);
                            wallet_scan_outcomes.push(false);
                        }
                    }
                }
            })
            .await;
            if drained.is_err() {
                // Cancel unfinished wallet scans and nested DAG work. Never claim
                // freshness for an incomplete scan; the next tick rechecks both
                // node health states and allows qualified failback.
                join_set.abort_all();
                wallet_scan_outcomes.push(false);
                warn!(
                    "[WORKER] UTXO scan timed out after {}s; aborted unfinished wallets before rechecking node health.",
                    timeout_secs
                );
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

fn bounded_wallet_scan_secs(value: u64) -> u64 {
    value.clamp(30, 600)
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
    fn wallet_scan_deadline_is_strictly_bounded() {
        assert_eq!(bounded_wallet_scan_secs(0), 30);
        assert_eq!(bounded_wallet_scan_secs(120), 120);
        assert_eq!(bounded_wallet_scan_secs(9_999), 600);
    }

    #[tokio::test]
    async fn aborted_wallet_scan_drops_the_inflight_worker() {
        use std::sync::atomic::{AtomicBool, Ordering};
        struct DropProbe(Arc<AtomicBool>);
        impl Drop for DropProbe {
            fn drop(&mut self) {
                self.0.store(true, Ordering::SeqCst);
            }
        }
        let dropped = Arc::new(AtomicBool::new(false));
        let probe = dropped.clone();
        let (started_tx, started_rx) = tokio::sync::oneshot::channel::<()>();
        let mut tasks = tokio::task::JoinSet::new();
        tasks.spawn(async move {
            let _guard = DropProbe(probe);
            let _ = started_tx.send(());
            std::future::pending::<()>().await;
            true
        });
        started_rx.await.expect("wallet worker should start");
        tasks.abort_all();
        let joined = tokio::time::timeout(Duration::from_secs(2), tasks.join_next())
            .await
            .expect("cancellation must finish promptly")
            .expect("one wallet task must be observed");
        assert!(joined.is_err());
        assert!(dropped.load(Ordering::SeqCst));
    }

    #[test]
    fn successful_scan_requires_every_wallet_task_to_succeed() {
        assert!(all_wallet_scan_tasks_succeeded(&[]));
        assert!(all_wallet_scan_tasks_succeeded(&[true, true]));
        assert!(!all_wallet_scan_tasks_succeeded(&[true, false]));
        assert!(!all_wallet_scan_tasks_succeeded(&[false]));
    }
}
