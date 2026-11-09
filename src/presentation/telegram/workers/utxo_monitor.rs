use crate::domain::entities::TrackedWallet;
use crate::domain::models::{BotEventType, EventSeverity};
use crate::infrastructure::database::postgres_adapter::PostgresRepository;
use crate::infrastructure::node::failover::{
    FailoverPolicy, NodeObservation, Selection, UtxoFailover, UtxoNode,
};
use crate::infrastructure::node::kaspa_adapter::KaspaRpcAdapter;
use crate::infrastructure::node::public_pnn_rotation::{
    RotationCandidate, RotationRequest, discover_public_spare, qualified_coeval_spare,
    should_search_for_spare,
};
use crate::infrastructure::node::quality_selector::{Quality, QualitySelector};
use crate::infrastructure::node::subscription::{
    MonitoringMode, MonitoringSchedule, spawn_selected_pnn_subscription_runtime,
    spawn_subscription_runtime,
};
use crate::infrastructure::telegram_delivery_queue::{
    AlertOutboxOutcome, AlertOutboxRequest, commit_alert_outbox_localized,
};
use chrono::{TimeZone, Utc};
use kaspa_rpc_core::api::rpc::RpcApi;
use std::collections::{HashMap, HashSet, VecDeque};
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

/// OP067: read-only, connection-preserving quality sample. The wRPC
/// client itself owns recovery and must never be disconnected by the
/// health scanner. Fresh observations are compared within the same scan.
async fn observe_public_pnn_quality(node: &KaspaRpcAdapter) -> Option<Quality> {
    if !node.client.is_connected() {
        return None;
    }
    let started = tokio::time::Instant::now();
    let server = tokio::time::timeout(Duration::from_secs(3), node.get_server_info())
        .await
        .ok()?
        .ok()?;
    let peers = tokio::time::timeout(
        Duration::from_secs(3),
        node.client.get_connected_peer_info(),
    )
    .await
    .ok()?
    .ok()?
    .peer_info
    .len();
    Some(Quality {
        is_mainnet: server.network_id.to_string() == "mainnet",
        is_synced: server.is_synced,
        has_utxo_index: server.has_utxo_index,
        peers,
        virtual_daa: server.virtual_daa_score,
        latency_ms: started.elapsed().as_millis() as u64,
    })
}

// OP071: only reconnect an UNQUALIFIED public source. A live RPC
// connection can still have stale DAA or be unsynced, so a successful
// observation alone is not enough to establish availability.
fn should_recover_unqualified_pnn(
    eligible: bool,
    connected: bool,
    unhealthy_checks: u32,
    cooldown_elapsed: bool,
) -> bool {
    !eligible && cooldown_elapsed && (!connected || unhealthy_checks >= 10)
}

// OP064 fixed owner-approved historical reward boundary (2026-11-05 17:30:32 UTC).
// Applies at the LAST delivery gate, irrespective of how a UTXO enters a scan.
// Old ledger rows remain untouched; no historical Telegram replay is possible.
const OP064_FIRST_ELIGIBLE_BLOCK_MS: u64 = 1793899832000;

fn op064_may_enqueue_coinbase(is_coinbase: bool, mined_block_time_ms: u64) -> bool {
    !is_coinbase || mined_block_time_ms >= OP064_FIRST_ELIGIBLE_BLOCK_MS
}

pub struct PnnSourceCandidates {
    pub standby: Option<Arc<KaspaRpcAdapter>>,
    pub five_way_sources: Option<Vec<Arc<KaspaRpcAdapter>>>,
}

pub fn start_utxo_monitor(
    _bot: Bot,
    node: Arc<KaspaRpcAdapter>,
    sources: PnnSourceCandidates,
    db: Arc<PostgresRepository>,
    live_sync_enabled: Arc<AtomicBool>,
    token: CancellationToken,
    price_context: Arc<crate::domain::models::AppContext>,
) {
    let PnnSourceCandidates {
        standby,
        five_way_sources,
    } = sources;
    let analyzer = Arc::new(AnalyzeDagUseCase::with_db(node.clone(), db.clone()));
    let utxo_service = Arc::new(UtxoMonitorService::new(node.clone(), db.clone(), analyzer));
    let standby_service = standby.as_ref().map(|alternate| {
        Arc::new(UtxoMonitorService::new(
            alternate.clone(),
            db.clone(),
            Arc::new(AnalyzeDagUseCase::with_db(alternate.clone(), db.clone())),
        ))
    });
    // Exactly ONE selected source produces reward outbox writes per scan.
    // Read-only health probes over other connections never process wallets.
    let pool_services = five_way_sources.as_ref().map(|sources| {
        sources
            .iter()
            .map(|source| {
                Arc::new(UtxoMonitorService::new(
                    source.clone(),
                    db.clone(),
                    Arc::new(AnalyzeDagUseCase::with_db(source.clone(), db.clone())),
                ))
            })
            .collect::<Vec<_>>()
    });
    // OP074: One authoritative source selection/generation is shared by
    // the transactional wallet writer and the durable VSPC indexer.
    // The per-source UTXO services and pinned endpoints remain IMMUTABLE
    // in this phase; dynamic URL replacement is deliberately not wired.
    let pnn_generation = five_way_sources.as_ref().map(|sources| {
        Arc::new(crate::infrastructure::node::source_registry::SourceRegistry::new(sources.clone()))
    });
    // Production-owned in-process VSPC indexer: one bounded PNN read per
    // interval. Never creates a Windows Task Scheduler/cron/timer unit.
    let indexer_node = node.clone();
    let indexer_sources = pnn_generation.clone();
    let indexer_db = db.clone();
    let indexer_cancel = token.clone();
    crate::infrastructure::resilience::runtime::spawn_resilient("pnn_vspc_indexer", async move {
        crate::network::vspc_proof::run_indexer(
            indexer_node,
            indexer_sources,
            indexer_db,
            indexer_cancel,
        )
        .await;
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

    // OP089: subscription now follows the SAME selected qualified source
    // and generation as the sole wallet writer and VSPC. The unrelated
    // primary wRPC is never the event trigger in five-source mode.
    if let Some(selected_sources) = pnn_generation.as_ref() {
        spawn_selected_pnn_subscription_runtime(
            selected_sources.clone(),
            signal_sender,
            lifecycle,
            token.clone(),
        );
    } else {
        spawn_subscription_runtime(node.client.clone(), signal_sender, lifecycle, token.clone());
    }

    crate::infrastructure::resilience::runtime::spawn_resilient("utxo_monitor_task", async move {
        // These two vectors are owned exclusively by the SINGLE wallet
        // scanner. Replacing one inactive endpoint also replaces its
        // UtxoMonitorService before a later selection can use that slot.
        let mut five_way_sources = five_way_sources;
        let mut pool_services = pool_services;
        let dynamic_rotation = std::env::var("KASPA_PNN_DYNAMIC_ROTATION").as_deref() == Ok("1");
        crate::infrastructure::metrics::set_pnn_dynamic_rotation_enabled(dynamic_rotation);

        let mut discovery_job: Option<tokio::task::JoinHandle<Option<RotationCandidate>>> = None;
        let mut last_discovery: Option<tokio::time::Instant> = None;
        let mut retired_endpoints = VecDeque::<(String, tokio::time::Instant)>::new();
        let mut failover = UtxoFailover::new(FailoverPolicy::default());
        let mut quality_selector = five_way_sources
            .as_ref()
            .map(|sources| QualitySelector::new(sources.len()));
        let mut recovery_last_attempt = five_way_sources
            .as_ref()
            .map(|sources| vec![None::<tokio::time::Instant>; sources.len()]);
        let mut unhealthy_streak = five_way_sources
            .as_ref()
            .map(|sources| vec![0u32; sources.len()]);
        let mut failed_reconnects = five_way_sources
            .as_ref()
            .map(|sources| vec![0u32; sources.len()]);
        // Track one guarded reconnect task per source, so swapping a dead
        // endpoint can abort its old in-flight transport reconnection.
        let mut recovery_jobs = five_way_sources.as_ref().map(|sources| {
            (0..sources.len())
                .map(|_| None::<tokio::task::JoinHandle<()>>)
                .collect::<Vec<_>>()
        });
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
                .next_trigger_with_forced_poll(
                    &token,
                    standby.is_some() || five_way_sources.is_some(),
                )
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

            // Discovery does ONLY bounded public read-only RPC in a
            // separate task. The transactional scanner alone decides
            // whether a freshly re-qualified candidate may atomically
            // replace one INACTIVE slot under the observed generation.
            if discovery_job
                .as_ref()
                .is_some_and(tokio::task::JoinHandle::is_finished)
                && let Some(job) = discovery_job.take()
            {
                let mut admitted = false;
                let mut rejection_reason =
                    crate::infrastructure::metrics::PnnSpareRejectReason::DiscoveryUnavailable;
                if let Ok(Some(proposal)) = job.await {
                    rejection_reason =
                        crate::infrastructure::metrics::PnnSpareRejectReason::StaleOrRecovered;
                    if let (
                        Some(sources),
                        Some(services),
                        Some(registry),
                        Some(unhealthy),
                        Some(reconnects),
                        Some(last_recovery),
                    ) = (
                        five_way_sources.as_mut(),
                        pool_services.as_mut(),
                        pnn_generation.as_ref(),
                        unhealthy_streak.as_mut(),
                        failed_reconnects.as_mut(),
                        recovery_last_attempt.as_mut(),
                    ) {
                        let slot = proposal.target;
                        let still_bad = slot < sources.len()
                            && unhealthy[slot] >= 10
                            && Arc::ptr_eq(&sources[slot], &proposal.expected_old);
                        let coeval_qualified = if still_bad {
                            qualified_coeval_spare(sources, &proposal.new_node).await
                        } else {
                            false
                        };
                        if still_bad && coeval_qualified {
                            let candidate = proposal.new_node.clone();
                            let replacement_service = Arc::new(UtxoMonitorService::new(
                                candidate.clone(),
                                db.clone(),
                                Arc::new(AnalyzeDagUseCase::with_db(candidate.clone(), db.clone())),
                            ));
                            match registry
                                .replace_inactive(
                                    proposal.generation,
                                    slot,
                                    &sources[slot],
                                    candidate.clone(),
                                    true,
                                )
                                .await
                            {
                                Ok(epoch) => {
                                    let old = std::mem::replace(&mut sources[slot], candidate);
                                    services[slot] = replacement_service;
                                    if let Some(url) = old.current_or_pinned_url() {
                                        retired_endpoints
                                            .push_back((url, tokio::time::Instant::now()));
                                    }
                                    unhealthy[slot] = 0;
                                    reconnects[slot] = 0;
                                    last_recovery[slot] = None;
                                    admitted = true;
                                    crate::infrastructure::metrics::inc_pnn_spare_swap_success();
                                    crate::infrastructure::metrics::set_pnn_source_generation(
                                        epoch,
                                    );
                                    info!(
                                        slot = slot + 1,
                                        source_generation = epoch,
                                        "[PNN ROTATION] Replaced an inactive degraded PNN with independently VSPC-qualified coeval DAA source."
                                    );
                                    // Abort any old reconnect before retiring
                                    // the old adapter: do not let a task revive
                                    // the now-outdated upstream endpoint.
                                    if let Some(jobs) = recovery_jobs.as_mut()
                                        && let Some(job) = jobs[slot].take()
                                    {
                                        job.abort();
                                    }
                                    // The old adapter is never the active wallet
                                    // writer, and VSPC uses the same generation.
                                    tokio::spawn(async move {
                                        let _ = tokio::time::timeout(
                                            Duration::from_secs(5),
                                            old.client.disconnect(),
                                        )
                                        .await;
                                    });
                                }
                                Err(reason) => {
                                    rejection_reason =
                                        crate::infrastructure::metrics::PnnSpareRejectReason::RegistryFence;
                                    tracing::warn!(
                                        reason,
                                        "[PNN ROTATION] Refused stale, duplicate or writer-active replacement."
                                    );
                                }
                            }
                        } else if still_bad {
                            rejection_reason =
                                crate::infrastructure::metrics::PnnSpareRejectReason::CoevalUnqualified;
                        }
                    }
                    if !admitted {
                        crate::infrastructure::metrics::inc_pnn_spare_rejected(rejection_reason);
                        let candidate = proposal.new_node;
                        tokio::spawn(async move {
                            let _ = tokio::time::timeout(
                                Duration::from_secs(5),
                                candidate.client.disconnect(),
                            )
                            .await;
                        });
                    }
                } else {
                    crate::infrastructure::metrics::inc_pnn_spare_rejected(rejection_reason);
                }
            }

            crate::infrastructure::observability::record_scan_trigger(trigger);
            tracing::debug!("[WORKER] UTXO scan triggered by {}.", trigger.as_str());

            crate::infrastructure::resilience::runtime::task_stage("rpc_node_health");

            let selected_service = if let (
                Some(sources),
                Some(services),
                Some(selector),
                Some(last_recovery),
                Some(unhealthy),
            ) = (
                five_way_sources.as_ref(),
                pool_services.as_ref(),
                quality_selector.as_mut(),
                recovery_last_attempt.as_mut(),
                unhealthy_streak.as_mut(),
            ) {
                // Concurrent bounded READ-ONLY probes; never run multiple
                // wallet readers or reward outbox writers concurrently.
                let mut checks = tokio::task::JoinSet::new();
                for (index, source) in sources.iter().enumerate() {
                    let source = source.clone();
                    checks.spawn(async move { (index, observe_public_pnn_quality(&source).await) });
                }
                let mut observations = vec![None; sources.len()];
                let _ = tokio::time::timeout(Duration::from_secs(8), async {
                    while let Some(done) = checks.join_next().await {
                        if let Ok((index, observation)) = done {
                            observations[index] = observation;
                        }
                    }
                })
                .await;
                checks.abort_all();
                // The public resolver may steer the primary to the same URL
                // as a pinned alternate after reconnect. Never count two
                // identical endpoints as two independent sources.
                let mut urls = std::collections::HashSet::new();
                for (index, source) in sources.iter().enumerate() {
                    let duplicate = source
                        .current_or_pinned_url()
                        .is_none_or(|url| !urls.insert(url));
                    if duplicate {
                        observations[index] = None;
                    }
                }
                let choice = selector.choose(started.elapsed(), &observations);
                // Publish the SAME selected source + generation that the
                // VSPC indexer sees. Its durable commit holds a read fence.
                if let Some(generation) = pnn_generation.as_ref() {
                    match generation.select(choice.active_index).await {
                        Ok(epoch) => {
                            crate::infrastructure::metrics::set_pnn_source_generation(epoch)
                        }
                        Err(reason) => {
                            tracing::error!(
                                "[PNN GENERATION] Invalid selected source, refusing wallet scan: {}",
                                reason
                            );
                            continue;
                        }
                    }
                }
                crate::infrastructure::metrics::set_pnn_pool_eligible_count(choice.eligible_count);
                crate::infrastructure::metrics::set_pnn_pool_eligible_slots_mask(
                    choice.eligible_slots_mask,
                );
                crate::infrastructure::metrics::set_pnn_pool_active_index(choice.active_index);
                // In five-source mode EVERY reward source, including slot 0,
                // is independent from the ancillary primary subscription.
                crate::infrastructure::metrics::set_pnn_utxo_standby_selected(
                    choice.active_index.is_some(),
                );
                // The legacy standby metric refers exactly to the pinned
                // source in slot zero; initial admission is NOT live status.
                crate::infrastructure::metrics::set_pnn_standby_qualified(
                    choice.eligible_slots_mask & 1 != 0,
                );
                if let Some((previous, next)) = choice.transition {
                    crate::infrastructure::metrics::inc_pnn_failover_transitions();
                    info!(
                        previous,
                        next,
                        "[PNN QUALITY] Transactional UTXO source switched to stronger eligible public PNN."
                    );
                }
                // OP071: all FIVE independent, pinned reward sources can
                // fail, including slot 0. The ancillary subscription primary
                // is a SEPARATE connection, not sources[0]. Only reconnect an
                // UNHEALTHY source after a 90s cooldown. Quality selection
                // has already excluded it from the one transactional writer,
                // so reconnect cannot interrupt the current healthy writer.
                for (index, source) in sources.iter().enumerate() {
                    let eligible = choice.eligible_slots_mask & (1u8 << index) != 0;
                    if eligible {
                        unhealthy[index] = 0;
                        if let Some(reconnects) = failed_reconnects.as_mut() {
                            reconnects[index] = 0;
                        }
                        continue;
                    }
                    unhealthy[index] = unhealthy[index].saturating_add(1);
                    let now = tokio::time::Instant::now();
                    let cooldown_ok = last_recovery[index]
                        .is_none_or(|last| now.duration_since(last) >= Duration::from_secs(90));
                    if should_recover_unqualified_pnn(
                        eligible,
                        source.client.is_connected(),
                        unhealthy[index],
                        cooldown_ok,
                    ) {
                        last_recovery[index] = Some(now);
                        let client = source.client.clone();
                        let cancel = token.clone();
                        crate::infrastructure::metrics::inc_pnn_forced_recoveries();
                        if let Some(reconnects) = failed_reconnects.as_mut() {
                            reconnects[index] = reconnects[index].saturating_add(1);
                        }
                        let old_recovery = async move {
                            tokio::select! {
                                _ = cancel.cancelled() => {},
                                _ = tokio::time::timeout(
                                    Duration::from_secs(10),
                                    client.connect(Some(
                                        kaspa_wrpc_client::client::ConnectOptions::blocking_fallback()
                                    )),
                                ) => {}
                            }
                        };
                        if let Some(jobs) = recovery_jobs.as_mut() {
                            if let Some(existing) = jobs[index].take()
                                && !existing.is_finished()
                            {
                                existing.abort();
                            }
                            jobs[index] = Some(tokio::spawn(old_recovery));
                        } else {
                            tokio::spawn(old_recovery);
                        }
                    }
                }
                // Automatic rotation is opt-in at deploy and at most one
                // discovery job may run. Require three failed guarded
                // reconnects and 20 unhealthy scans, then at most one
                // discovery attempt per ten minutes. Source 0 is eligible.
                if dynamic_rotation
                    && discovery_job.is_none()
                    && let (Some(registry), Some(reconnects)) =
                        (pnn_generation.as_ref(), failed_reconnects.as_ref())
                {
                    let now = tokio::time::Instant::now();
                    let cooldown = last_discovery
                        .is_none_or(|last| now.duration_since(last) >= Duration::from_secs(600));
                    if cooldown
                        && let Some((slot, source)) =
                            sources.iter().enumerate().find(|(slot, _)| {
                                should_search_for_spare(
                                    choice.eligible_slots_mask & (1u8 << slot) != 0,
                                    choice.active_index,
                                    *slot,
                                    reconnects[*slot],
                                    unhealthy[*slot],
                                    cooldown,
                                    false,
                                )
                            })
                    {
                        let (epoch, _, snapshot) = registry.snapshot().await;
                        if Arc::ptr_eq(&snapshot[slot], source) {
                            // Previously retired public endpoints are held
                            // for one hour to avoid bouncing into a bad URL.
                            retired_endpoints.retain(|(_, at)| {
                                now.duration_since(*at) < Duration::from_secs(3600)
                            });
                            let mut excluded_urls = HashSet::new();
                            for old in &snapshot {
                                if let Some(url) = old.current_or_pinned_url() {
                                    excluded_urls.insert(url);
                                }
                            }
                            for (url, _) in &retired_endpoints {
                                excluded_urls.insert(url.clone());
                            }
                            let request = RotationRequest {
                                target: slot,
                                generation: epoch,
                                expected_old: source.clone(),
                                excluded_urls,
                            };
                            last_discovery = Some(now);
                            crate::infrastructure::metrics::inc_pnn_spare_discovery();
                            info!(
                                slot = slot + 1,
                                "[PNN ROTATION] Bounded private discovery started for unhealthy inactive source."
                            );
                            let cancel = token.clone();
                            discovery_job = Some(tokio::spawn(async move {
                                tokio::select! {
                                    _ = cancel.cancelled() => None,
                                    result = discover_public_spare(request) => result,
                                }
                            }));
                        }
                    }
                }
                choice
                    .active_index
                    .and_then(|index| services.get(index).cloned())
            } else if let Some(standby_node) = standby.as_ref() {
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
                    crate::infrastructure::metrics::inc_pnn_failover_transitions();
                    info!(from = ?old, to = ?new,
                        "[UTXO FAILOVER] Selected a single eligible public PNN UTXO source.");
                }
                crate::infrastructure::metrics::set_pnn_utxo_standby_selected(
                    decision.selection == Selection::Ready(UtxoNode::KasStandby),
                );
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
                        if !op064_may_enqueue_coinbase(event.is_coinbase, event.block_time_ms) {
                            tracing::debug!(
                                "[OP064 HISTORICAL] Verified pre-cutover block is not eligible for Telegram outbox. No historical reward replay."
                            );
                            continue;
                        }
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
                                crate::infrastructure::metrics::observe_block_to_outbox(event.block_time_ms);

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
mod op064_delivery_cutover_tests {
    use super::*;
    #[test]
    fn old_coinbase_is_never_enqueued_regardless_of_proof_or_retry_path() {
        assert!(!op064_may_enqueue_coinbase(
            true,
            OP064_FIRST_ELIGIBLE_BLOCK_MS - 1
        ));
        assert!(!op064_may_enqueue_coinbase(true, 0));
        assert!(op064_may_enqueue_coinbase(
            true,
            OP064_FIRST_ELIGIBLE_BLOCK_MS
        ));
        assert!(op064_may_enqueue_coinbase(
            true,
            OP064_FIRST_ELIGIBLE_BLOCK_MS + 1
        ));
        assert!(op064_may_enqueue_coinbase(false, 0));
    }
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

#[cfg(test)]
mod op071_five_source_recovery_policy_tests {
    use super::should_recover_unqualified_pnn;

    #[test]
    fn first_independent_pnn_slot_is_recoverable_without_a_special_case() {
        // Same policy for every index. Slot zero is NOT the ancillary primary.
        for slot in 0..5 {
            assert!(
                should_recover_unqualified_pnn(false, false, 1, true),
                "Slot {slot} must be recoverable when disconnected"
            );
        }
    }
    #[test]
    fn never_disconnect_currently_qualified_or_cooling_down_source() {
        assert!(!should_recover_unqualified_pnn(true, true, 99, true));
        assert!(!should_recover_unqualified_pnn(true, false, 99, true));
        assert!(!should_recover_unqualified_pnn(false, false, 99, false));
        assert!(!should_recover_unqualified_pnn(false, true, 9, true));
    }
    #[test]
    fn connected_but_stale_daa_or_unsynced_source_is_not_marked_healthy() {
        assert!(should_recover_unqualified_pnn(false, true, 10, true));
        assert!(should_recover_unqualified_pnn(false, true, 1000, true));
    }
}
