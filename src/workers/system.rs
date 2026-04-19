use kaspa_rpc_core::api::rpc::RpcApi;
use teloxide::prelude::*;
use teloxide::types::ChatId;
use tokio::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

use crate::context::AppContext;

pub fn spawn_price_monitor(ctx: AppContext, token: CancellationToken) {
    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = token.cancelled() => { break; }
                _ = tokio::time::sleep(Duration::from_secs(60)) => {
                    let client = reqwest::Client::new();
                    if let Ok(r) = client.get("https://api.coingecko.com/api/v3/simple/price?ids=kaspa&vs_currencies=usd&include_market_cap=true")
                        .header("User-Agent", "KaspaPulse/1.0 (Enterprise Node)").send().await {
                        if let Ok(j) = r.json::<serde_json::Value>().await {
                            let price = j["kaspa"]["usd"].as_f64().unwrap_or(0.0);
                            let mcap = j["kaspa"]["usd_market_cap"].as_f64().unwrap_or(0.0);
                            let mut write_guard = ctx.price_cache.write().await;
                            *write_guard = (price, mcap);
                        }
                    }
                }
            }
        }
    });
}

pub fn spawn_node_monitor(ctx: AppContext, bot: Bot, token: CancellationToken) {
    tokio::spawn(async move {
        let _ = ctx.rpc.connect(None).await;
        loop {
            tokio::select! {
                _ = token.cancelled() => { break; }
                _ = tokio::time::sleep(Duration::from_secs(30)) => {
                    if ctx.rpc.get_server_info().await.is_err() {
                        error!("[NODE ALERT] RPC Connection Lost! Reconnecting...");
                        if let Err(e) = bot.send_message(ChatId(ctx.admin_id), "🚨 <b>ALERT:</b> Node connection lost! Reconnecting...")
                            .parse_mode(teloxide::types::ParseMode::Html).await { tracing::error!("[TELEGRAM API ERROR] Failed to execute: {}", e); }
                        let _ = ctx.rpc.connect(None).await;
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
                    ctx.utxo_state.clear(); // Safely clear the native UTXO cache state
                    info!("[MEMORY CLEANER] Purged native UTXO cache state to free RAM.");
                }
            }
        }
    });
}


