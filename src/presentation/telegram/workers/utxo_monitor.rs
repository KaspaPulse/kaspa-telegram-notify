use crate::infrastructure::database::postgres_adapter::PostgresRepository;
use crate::infrastructure::node::kaspa_adapter::KaspaRpcAdapter;
use chrono::{TimeZone, Utc};
use std::sync::Arc;
use std::time::Duration;
use teloxide::prelude::*;
use teloxide::types::ChatId;
use tokio::sync::Semaphore;
use tracing::{error, info};

use crate::network::analyze_dag::AnalyzeDagUseCase;
use crate::wallet::wallet_use_cases::UtxoMonitorService;

pub fn start_utxo_monitor(bot: Bot, node: Arc<KaspaRpcAdapter>, db: Arc<PostgresRepository>) {
    let analyzer = Arc::new(AnalyzeDagUseCase::new(node.clone()));
    let utxo_service = Arc::new(UtxoMonitorService::new(node.clone(), db.clone(), analyzer));
    let semaphore = Arc::new(Semaphore::new(10));

    tokio::spawn(async move {
        info!("🚀 [WORKER] Enterprise UTXO Monitor Engine Started...");

        loop {
            tokio::time::sleep(Duration::from_secs(10)).await;

            if let Ok((is_online, _)) = node.get_node_health().await {
                if !is_online {
                    continue;
                }
            }

            let wallets = match db.get_all_tracked_wallets().await {
                Ok(w) => w,
                Err(e) => {
                    error!("[DATABASE ERROR] Failed to fetch wallets: {}", e);
                    continue;
                }
            };

            if wallets.is_empty() {
                continue;
            }

            let mut join_set = tokio::task::JoinSet::new();

            for wallet in wallets {
                let sem = semaphore.clone();
                let service = utxo_service.clone();
                let bot_clone = bot.clone();

                join_set.spawn(async move {
                    let _permit = match sem.acquire_owned().await {
                        Ok(p) => p,
                        Err(_) => return,
                    };

                    match service.check_wallet_utxos(&wallet.address).await {
                        Ok(events) => {
                            for event in events {
                                let log_time = if event.block_time_ms > 0 {
                                    Utc.timestamp_millis_opt(event.block_time_ms as i64).single()
                                        .map(|dt| dt.format("%H:%M:%S.%3f").to_string()).unwrap_or_else(|| "Unknown".to_string())
                                } else { "Real-time".to_string() };

                                info!("💎 [LIVE BLOCK] | Amount: +{:.4} KAS | Wallet: {} | Time: {} | Status: Delivered",
                                      event.amount_kas, crate::utils::format_short_wallet(&event.wallet_address), log_time);

                                let final_msg = crate::presentation::telegram::formatting::events_formatter::format_live_event(&event);
                                crate::utils::log_multiline(&format!("📤 [BOT OUT] Chat: {}", wallet.chat_id), &final_msg, true);
let _ = bot_clone.send_message(ChatId(wallet.chat_id), &final_msg)
                                    .parse_mode(teloxide::types::ParseMode::Html)
                                    .link_preview_options(teloxide::types::LinkPreviewOptions {
                                        url: None,
                                        is_disabled: true,
                                        show_above_text: false,
                                        prefer_small_media: false,
                                        prefer_large_media: false
                                    })
                                    .await;
                            }
                        }
                        Err(e) => error!("Failed to check UTXOs for {}: {}", wallet.address, e),
                    }
                });
            }

            while join_set.join_next().await.is_some() {}
        }
    });
}
