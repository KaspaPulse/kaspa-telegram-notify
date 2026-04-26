use kaspa_rpc_core::api::rpc::RpcApi;
use std::collections::{HashSet, VecDeque};
use tracing::{info, warn};

use crate::domain::models::AppContext;
use crate::utils::format_short_wallet;

/// Background task to sync all wallets from their last checkpoint to the current tips.
pub async fn sync_all_wallets_from_pruning_point(
    ctx: AppContext,
) -> Result<(), crate::domain::errors::BotError> {
    let wallets: Vec<String> = sqlx::query_scalar("SELECT DISTINCT wallet FROM user_wallets")
        .fetch_all(&ctx.pool)
        .await?;

    if wallets.is_empty() {
        info!("ℹ️ [SYNC] No wallets found in database to scan.");
        return Ok(());
    }

    info!(
        "🚀 [GLOBAL SYNC] Starting historical scan for {} wallets...",
        wallets.len()
    );
    for wallet in wallets {
        let _ = sync_single_wallet(ctx.clone(), wallet).await;
    }
    info!("✅ [GLOBAL SYNC] All wallets processed.");
    Ok(())
}

/// Core logic for reverse-scanning a single wallet with advanced rich logging.
pub async fn sync_single_wallet(
    ctx: AppContext,
    wallet: String,
) -> Result<(), crate::domain::errors::BotError> {
    info!("🔍 [REVERSE SCAN] Initiated for wallet: {}", wallet);

    let dag_info = ctx.rpc.get_block_dag_info().await?;
    let pruning_point = dag_info.pruning_point_hash;

    // ✅ Checkpoint Retrieval
    let last_checkpoint =
        crate::infrastructure::database::state_store::get_sync_checkpoint(&ctx.pool, &wallet).await;

    info!(
        "📊 [SYNC STATS] Checkpoint Score: {} | Pruning Point Hash: {}",
        last_checkpoint, pruning_point
    );

    let mut queue = VecDeque::from(dag_info.tip_hashes.clone());
    let mut visited = HashSet::new();
    // [ENTERPRISE FIX] Limit heavy byte scanning to 4 concurrent CPU threads
    static CPU_SEMAPHORE: std::sync::OnceLock<std::sync::Arc<tokio::sync::Semaphore>> = std::sync::OnceLock::new();
    let cpu_semaphore = CPU_SEMAPHORE.get_or_init(|| std::sync::Arc::new(tokio::sync::Semaphore::new(4))).clone();
    let mut discovered_rewards = HashSet::new();
    let mut scanned_count = 0;
    let mut recovered_count = 0;

    while let Some(current_hash) = queue.pop_front() {
        if !visited.insert(current_hash) || current_hash == pruning_point {
            continue;
        }

        if let Ok(block) = ctx.rpc.get_block(current_hash, true).await {
            scanned_count += 1;

            // ❌ REMOVED: tokio::task::yield_now().await; -> It causes queue thrashing instead of yielding.

            // [SAFETY] Guard against infinite or too deep scans
            if scanned_count > 100_000 {
                warn!(
                    "⚠️ [SYNC LIMIT] Reached maximum scan depth (100,000 blocks) for {}. Stopping.",
                    wallet
                );
                break;
            }

            if scanned_count % 500 == 0 {
                info!(
                    "⏳ [PROGRESS] Scanned {} blocks... current DAA: {}",
                    scanned_count, block.header.daa_score
                );
            }

            // Stop scanning this branch if we reached the checkpoint
            if block.header.daa_score <= last_checkpoint {
                continue;
            }

            if let Some(tx0) = block.transactions.first() {
                for (index, output) in tx0.outputs.iter().enumerate() {
                    if let Some(verbose) = &output.verbose_data {
                        if verbose.script_public_key_address.to_string() == wallet {
                            if let Some(block_verbose) = &block.verbose_data {
                                for blue_hash in &block_verbose.merge_set_blues_hashes {
                                    if !discovered_rewards.contains(blue_hash) {
                                        if let Ok(blue_block) =
                                            ctx.rpc.get_block(*blue_hash, true).await
                                        {
                                            let script = output.script_public_key.script().to_vec();

                                            // ✅ Enterprise Performance Patch: Offload heavy CPU-bound byte scanning to a blocking thread pool
                                            let payload_clone =
                                                blue_block.transactions[0].payload.clone();
                                            let script_clone = script.clone();

                                            let _permit = cpu_semaphore.clone().acquire_owned().await.unwrap();
                                            let is_match = match tokio::task::spawn_blocking(
                                                move || {
                                                    payload_clone
                                                        .windows(script_clone.len())
                                                        .any(|w| w == script_clone)
                                                },
                                            )
                                            .await
                                            {
                                                Ok(result) => result,
                                                Err(join_err) => {
                                                    // Log the exact panic reason (e.g. Out of Memory, Bounds Check)
                                                    tracing::error!("🚨 [CRITICAL ALERT] CPU thread panicked during deep block scan: {}", join_err);

                                                    // Bubble up the error to the Domain layer instead of swallowing it
                                                    return Err(
                                                        crate::domain::errors::BotError::Internal(
                                                            format!(
                                                                "Sync thread panicked: {}",
                                                                join_err
                                                            ),
                                                        ),
                                                    );
                                                }
                                            };

                                            if is_match {
                                                discovered_rewards.insert(*blue_hash);

                                                let tx_id = blue_block.transactions[0]
                                                    .verbose_data
                                                    .as_ref()
                                                    .map(|v| v.transaction_id.to_string())
                                                    .unwrap_or_default();

                                                // ✅ Constructing 'outpoint' to match DB Schema (PKey)
                                                let outpoint = format!("{}:{}", tx_id, index);

                                                // ✅ Call record_recovery_block with corrected signature
                                                crate::infrastructure::database::state_store::record_recovery_block(
                                                    &ctx.pool,
                                                    &wallet,
                                                    &outpoint,
                                                    output.value as i64,
                                                    blue_block.header.daa_score,
                                                )
                                                .await;

                                                recovered_count += 1;

                                                info!("✅ [RECOVERY SUCCESS] | Amount: {:.2} KAS | DAA: {} | Wallet: {}",
                                                      (output.value as f64 / 1e8),
                                                      blue_block.header.daa_score,
                                                      format_short_wallet(&wallet));
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }

            // Traverse DAG backwards
            for level in block.header.parents_by_level {
                for parent in level {
                    if !visited.contains(&parent) {
                        queue.push_back(parent);
                    }
                }
            }
        }
    }

    // ✅ Update Checkpoint to the virtual tips upon successful scan
    crate::infrastructure::database::state_store::update_sync_checkpoint(
        &ctx.pool,
        &wallet,
        dag_info.virtual_daa_score,
    )
    .await;

    info!(
        "🏁 [SYNC FINISHED] Wallet: {} | Total Scanned: {} | Total Recovered: {}",
        format_short_wallet(&wallet),
        scanned_count,
        recovered_count
    );
    Ok(())
}


