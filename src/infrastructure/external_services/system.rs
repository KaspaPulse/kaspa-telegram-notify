use kaspa_rpc_core::api::rpc::RpcApi;
use std::sync::atomic::Ordering;
use teloxide::prelude::*;
use teloxide::types::ChatId;
use tokio::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::domain::models::AppContext;

pub fn spawn_price_monitor(ctx: AppContext, token: CancellationToken) {
    tokio::spawn(async move {
        let client = reqwest::Client::new();

        // Fetch instantly on boot
        let mut p = 0.0;
        let mut m = 0.0;
        if let Ok(r) = client.get("https://api.kaspa.org/info/price").send().await {
            if let Ok(j) = r.json::<serde_json::Value>().await {
                p = j["price"].as_f64().unwrap_or(0.0);
            }
        }
        if let Ok(r) = client
            .get("https://api.kaspa.org/info/marketcap")
            .send()
            .await
        {
            if let Ok(j) = r.json::<serde_json::Value>().await {
                m = j["marketcap"].as_f64().unwrap_or(0.0);
            }
        }
        if p > 0.0 {
            let mut write_guard = ctx.price_cache.write().await;
            *write_guard = (p, m);
        }

        loop {
            tokio::select! {
                _ = token.cancelled() => { break; }
                _ = tokio::time::sleep(tokio::time::Duration::from_secs(60)) => {
                    let mut p = 0.0;
                    let mut m = 0.0;
                    if let Ok(r) = client.get("https://api.kaspa.org/info/price").send().await {
                        if let Ok(j) = r.json::<serde_json::Value>().await { p = j["price"].as_f64().unwrap_or(0.0); }
                    }
                    if let Ok(r) = client.get("https://api.kaspa.org/info/marketcap").send().await {
                        if let Ok(j) = r.json::<serde_json::Value>().await { m = j["marketcap"].as_f64().unwrap_or(0.0); }
                    }
                    if p > 0.0 {
                        let mut write_guard = ctx.price_cache.write().await;
                        *write_guard = (p, m);
                    }
                }
            }
        }
    });
}

pub fn spawn_node_monitor(ctx: AppContext, bot: Bot, token: CancellationToken) {
    tokio::spawn(async move {
        let mut failed_attempts = 0;
        let mut is_disconnected = false;
        let _ = ctx.rpc.connect(None).await;

        tokio::time::sleep(Duration::from_secs(10)).await;

        loop {
            tokio::select! {
                _ = token.cancelled() => { break; }
                _ = tokio::time::sleep(Duration::from_secs(60)) => {
                    if ctx.rpc.get_server_info().await.is_err() {
                        failed_attempts += 1;
                        tracing::error!("[NODE ALERT] RPC Connection Lost! Attempt {}...", failed_attempts);

                        if failed_attempts == 1 {
                            is_disconnected = true;
                            // Safe sleep mode
                            ctx.live_sync_enabled.store(false, Ordering::Relaxed);
                            let _ = bot.send_message(ChatId(ctx.admin_id), "⚠️ <b>WARNING:</b> Primary Node connection dropped!\n⏸️ UTXO Monitoring paused safely.\n🔄 Attempting background recovery...")
                                .parse_mode(teloxide::types::ParseMode::Html).await;
                        }

                        if failed_attempts % 10 == 0 {
                            let _ = bot.send_message(ChatId(ctx.admin_id), format!("🚨 <b>CRITICAL:</b> Node still unreachable after {} attempts. Continuing to retry quietly...", failed_attempts))
                                .parse_mode(teloxide::types::ParseMode::Html).await;
                        }

                        let _ = ctx.rpc.connect(None).await;
                    } else {
                        if is_disconnected {
                            tracing::info!("[NODE RECOVERED] RPC Tunnel stabilized.");
                            ctx.live_sync_enabled.store(true, Ordering::Relaxed);
                            let _ = bot.send_message(ChatId(ctx.admin_id), "✅ <b>RECOVERED:</b> Node connection stabilized.\n▶️ UTXO Monitoring resumed smoothly.")
                                .parse_mode(teloxide::types::ParseMode::Html).await;

                            failed_attempts = 0;
                            is_disconnected = false;
                        }
                    }
                }
            }
        }
    });
}

pub fn spawn_memory_cleaner(ctx: AppContext, token: CancellationToken) {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = token.cancelled() => { break; }
                _ = tokio::time::sleep(Duration::from_secs(3600)) => {
                    ctx.utxo_state.retain(|wallet, _| ctx.state.contains_key(wallet));
                    ctx.rate_limiter.retain_recent();
                    let db_res = sqlx::query("DELETE FROM chat_history WHERE timestamp < CURRENT_TIMESTAMP - INTERVAL '30 days'").execute(&ctx.pool).await;
                    if let Err(e) = db_res { tracing::error!("[DATABASE ERROR] Failed to purge old chats: {}", e); }
                    tracing::info!("[MEMORY CLEANER] Purged UTXO cache, inactive rate limits, and 30-day chat history.");
                }
            }
        }
    });
}
