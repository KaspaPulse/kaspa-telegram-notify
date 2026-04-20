use kaspa_rpc_core::api::rpc::RpcApi;
use std::collections::{HashSet, VecDeque};
use tracing::info;

use crate::context::AppContext;
use crate::utils::format_short_wallet;

/// Background task to sync all wallets from their last checkpoint to the current tips.
pub async fn sync_all_wallets_from_pruning_point(ctx: AppContext) -> anyhow::Result<()> {
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
pub async fn sync_single_wallet(ctx: AppContext, wallet: String) -> anyhow::Result<()> {
    info!("🔍 [REVERSE SCAN] Initiated for wallet: {}", wallet);

    let dag_info = ctx.rpc.get_block_dag_info().await?;
    let pruning_point = dag_info.pruning_point_hash;
    let last_checkpoint = crate::state::get_sync_checkpoint(&ctx.pool, &wallet).await;

    info!(
        "📊 [SYNC STATS] Checkpoint Score: {} | Pruning Point: {}",
        last_checkpoint, pruning_point
    );

    let mut queue = VecDeque::from(dag_info.tip_hashes.clone());
    let mut visited = HashSet::new();
    let mut discovered_rewards = HashSet::new();
    let mut scanned_count = 0;
    let mut recovered_count = 0;

    while let Some(current_hash) = queue.pop_front() {
        if !visited.insert(current_hash) || current_hash == pruning_point {
            continue;
        }

        if let Ok(block) = ctx.rpc.get_block(current_hash, true).await {
            scanned_count += 1;

            // [PHASE 6 FIX] Prevent Unbounded Queue OOM by enforcing a maximum scan limit
            if scanned_count > 100_000 {
                tracing::warn!("⚠️ [SYNC LIMIT] Reached maximum scan depth (100,000 blocks) for {}. Halting sync to prevent memory exhaustion.", wallet);
                break;
            }

            if scanned_count % 500 == 0 {
                info!(
                    "⏳ [PROGRESS] Scanned {} blocks... current DAA: {}",
                    scanned_count, block.header.daa_score
                );
            }

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
                                            if blue_block.transactions[0]
                                                .payload
                                                .windows(script.len())
                                                .any(|w| w == script)
                                            {
                                                discovered_rewards.insert(*blue_hash);
                                                let tx_id = blue_block.transactions[0]
                                                    .verbose_data
                                                    .as_ref()
                                                    .map(|v| v.transaction_id.to_string())
                                                    .unwrap_or_default();
                                                let outpoint = format!("{}:{}", tx_id, index);

                                                let exists = false; // Optimized: Skipping SELECT, relying on PostgreSQL ON CONFLICT DO NOTHING

                                                if !exists {
                                                    crate::state::record_recovery_block(
                                                        &ctx.pool,
                                                        &outpoint,
                                                        &wallet,
                                                        output.value as f64 / 1e8,
                                                        blue_block.header.daa_score,
                                                    )
                                                    .await;

                                                    recovered_count += 1;

                                                    info!("✅ [RECOVERY SUCCESS] | Amount: {:.2} KAS | DAA: {} | Hash: {} | Wallet: {}",
                                                          output.value as f64 / 1e8,
                                                          blue_block.header.daa_score,
                                                          blue_hash,
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
            }

            for level in block.header.parents_by_level {
                for parent in level {
                    if !visited.contains(&parent) {
                        queue.push_back(parent);
                    }
                }
            }
        }
    }

    crate::state::update_sync_checkpoint(&ctx.pool, &wallet, dag_info.virtual_daa_score).await;
    info!(
        "🏁 [SYNC FINISHED] Wallet: {} | Total Scanned: {} | Total Recovered: {}",
        format_short_wallet(&wallet),
        scanned_count,
        recovered_count
    );
    Ok(())
}
