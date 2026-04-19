use kaspa_addresses::Address;
use kaspa_rpc_core::api::rpc::RpcApi;
use std::collections::HashSet;
use teloxide::{prelude::*, types::ChatId};

use super::is_local_node;
use crate::context::AppContext;
use crate::utils::{f_num, format_short_wallet, refresh_markup, send_or_edit_log};

pub async fn handle_start(bot: Bot, chat_id: ChatId, user_id: i64, ctx: &AppContext) {
    let markup = if user_id == ctx.admin_id {
        crate::kaspa_features::admin_menu_markup()
    } else {
        crate::kaspa_features::main_menu_markup()
    };

    let welcome_text = "\
🤖 <b>Kaspa Pulse Enterprise</b>
━━━━━━━━━━━━━━━━━━
Welcome to the ultimate Kaspa Solo Mining & Node monitoring engine. I operate with zero-latency, providing direct unindexed BlockDAG forensics.

⚡ <b>Quick Start:</b>
Simply paste any <code>kaspa:...</code> address in this chat to instantly activate real-time tracking and historical block recovery.

🧠 <b>AI-Powered Intelligence:</b>
Equipped with an advanced AI. Ask complex Kaspa questions, or send <b>Voice Notes</b> for instant transcription and analysis!

👇 <i>Select an option below or type /help for commands.</i>";

    let _ = bot
        .send_message(chat_id, welcome_text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .reply_markup(markup)
        .await;
}

pub async fn handle_help(bot: Bot, chat_id: ChatId) {
    let help_text = "\
📚 <b>Kaspa Pulse Enterprise - Help Guide</b>
━━━━━━━━━━━━━━━━━━
Welcome to the most advanced Kaspa Solo Mining & Node monitoring bot.

🛠️ <b>Public Commands:</b>
• /start - Main menu & initialization
• /add <code>kaspa:...</code> - Start tracking a wallet
• /remove <code>kaspa:...</code> - Stop tracking a wallet
• /list - View your tracked wallets
• /balance - Live node balances & UTXOs
• /blocks - Mined blocks & lifetime value
• /miner - Real-time solo hashrate estimate
• /network - Node health, peers, & sync status
• /dag - BlockDAG metrics & difficulty
• /price - Live KAS price & market cap
• /supply - Circulating supply metrics
• /fees - Current mempool fee estimate
• /donate - Support the developer

👑 <b>Admin Commands:</b>
• /stats, /sys, /logs - System diagnostics
• /pause, /resume, /restart - Engine control
• /sync - Manual historical reverse-scan
• /learn, /autolearn - AI Knowledge DB management

✨ <b>Smart Features:</b>
🎙️ <b>AI Voice & Chat:</b> Ask questions or send voice notes! The AI contextually knows your balance and network stats.
⚡ <b>Auto-Track:</b> Just paste any <code>kaspa:...</code> address in the chat to start monitoring instantly.
🔍 <b>Forensics:</b> Extracts Worker IDs and nonces directly from the unindexed BlockDAG.";

    let _ = bot
        .send_message(chat_id, help_text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await;
}

pub async fn handle_donate(bot: Bot, chat_id: ChatId) {
    if let Err(e) = bot.send_message(chat_id, "❤️ <b>Support Development</b>\n\n<b>KAS Address:</b>\n<code>kaspa:qz0yqq8z3twwgg7lq2mjzg6w4edqys45w2wslz7tym2tc6s84580vvx9zr44g</code>").parse_mode(teloxide::types::ParseMode::Html).await { tracing::error!("[TELEGRAM API ERROR] Failed to execute: {}", e); }
}

pub async fn handle_add(bot: Bot, chat_id: ChatId, wallet: String, ctx: &AppContext) {
    let wallet = wallet.trim().to_string();
    if wallet.is_empty() || !wallet.starts_with("kaspa:") {
        let _ = bot
            .send_message(
                chat_id,
                "⚠️ <b>Invalid Format.</b>\nUse: <code>/add kaspa:q...</code>",
            )
            .parse_mode(teloxide::types::ParseMode::Html)
            .await;
        return;
    }
    crate::state::add_wallet_to_db(&ctx.pool, &wallet, chat_id.0).await;
    ctx.state
        .entry(wallet.clone())
        .or_insert_with(HashSet::new)
        .insert(chat_id.0);

    let is_local = is_local_node();
    let sync_status_msg = if is_local {
        "\n\n🔄 <i>Historical sync started from pruning point...</i>"
    } else {
        "\n\n⚠️ <i>Live tracking active. (Historical sync disabled on public nodes)</i>"
    };
    let _ = bot
        .send_message(
            chat_id,
            format!(
                "✅ <b>Wallet Added!</b>\n<code>{}</code> is now monitored.{}",
                wallet, sync_status_msg
            ),
        )
        .parse_mode(teloxide::types::ParseMode::Html)
        .await;

    if is_local {
        let ctx_c = ctx.clone();
        let wallet_c = wallet.clone();
        tokio::spawn(async move {
            let _ = crate::workers::sync_single_wallet(ctx_c, wallet_c).await;
        });
    }
}

pub async fn handle_remove(bot: Bot, chat_id: ChatId, wallet: String, ctx: &AppContext) {
    let wallet = wallet.trim().to_string();
    crate::state::remove_wallet_from_db(&ctx.pool, &wallet, chat_id.0).await;
    if let Some(mut users) = ctx.state.get_mut(&wallet) {
        users.remove(&chat_id.0);
    }
    let _ = bot
        .send_message(chat_id, "🗑️ <b>Wallet Removed.</b>")
        .parse_mode(teloxide::types::ParseMode::Html)
        .await;
}

pub async fn handle_list(bot: Bot, chat_id: ChatId, ctx: &AppContext) {
    let mut tracked = String::new();
    for e in ctx.state.iter().filter(|e| e.value().contains(&chat_id.0)) {
        tracked.push_str(&format!("• <code>{}</code>\n", e.key()));
    }
    let text = if tracked.is_empty() {
        "📂 <b>No wallets tracked.</b>".to_string()
    } else {
        format!("📂 <b>Tracked Wallets:</b>\n{}", tracked)
    };
    let _ = bot
        .send_message(chat_id, text)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await;
}

pub async fn handle_balance(
    bot: Bot,
    chat_id: ChatId,
    ctx: &AppContext,
    current_utc_time: String,
    edit_msg_id: Option<teloxide::types::MessageId>,
) {
    let mut total = 0.0;
    let mut text = format!(
        "💰 <b>Wallet Analysis & Live Balance</b>\n⏱️ <code>{}</code>\n━━━━━━━━━━━━━━━━━━\n",
        current_utc_time
    );
    let tracked_wallets: Vec<String> = ctx
        .state
        .iter()
        .filter(|e| e.value().contains(&chat_id.0))
        .map(|e| e.key().clone())
        .collect();
    for wallet_str in tracked_wallets {
                if let Ok((balance, utxo_count)) = crate::services::kaspa::KaspaNodeService::get_balance(&ctx.rpc, &wallet_str).await {
            total += balance;
            text.push_str(&format!(
                "⏱️ <code>{}</code>\n├ <b>Live Balance:</b> {:.8} KAS\n└ <b>UTXOs:</b> {}\n\n",
                format_short_wallet(&wallet_str),
                balance,
                utxo_count
            ));
        } else {
            tracing::error!("[NODE ERROR] Failed to fetch data for wallet: {}", wallet_str);
        }
    }
    text.push_str(&format!(
        "━━━━━━━━━━━━━━━━━━\n💎 <b>Total Holdings:</b> <code>{} KAS</code>",
        f_num(total)
    ));
    let _ = send_or_edit_log(
        &bot,
        chat_id,
        edit_msg_id,
        text,
        Some(refresh_markup("refresh_balance")),
    )
    .await;
}

pub async fn handle_blocks(
    bot: Bot,
    chat_id: ChatId,
    ctx: &AppContext,
    current_utc_time: String,
    edit_msg_id: Option<teloxide::types::MessageId>,
) {
    let tracked: Vec<String> = ctx
        .state
        .iter()
        .filter(|e| e.value().contains(&chat_id.0))
        .map(|e| e.key().clone())
        .collect();
    if tracked.is_empty() {
        let _ = send_or_edit_log(
            &bot,
            chat_id,
            edit_msg_id,
            "⚠️ <b>No wallets tracked.</b>".to_string(),
            None,
        )
        .await;
        return;
    }
    let mut text = format!(
        "🧱 <b>Mined Blocks Analysis</b>\n⏱️ <code>{}</code>\n━━━━━━━━━━━━━━━━━━\n",
        current_utc_time
    );
    let (mut global_blocks, mut global_rewards) = (0, 0.0);
    for w in tracked {
        if let Ok((total_blocks, total_kas)) = crate::state::get_lifetime_stats(&ctx.pool, &w).await
        {
            global_blocks += total_blocks;
            global_rewards += total_kas;
            text.push_str(&format!(
                "💼 <b>{}</b>\n├ <b>Lifetime Blocks:</b> {}\n├ <b>Lifetime Value:</b> {:.8} KAS\n",
                format_short_wallet(&w),
                total_blocks,
                total_kas
            ));
            let daily_records: Result<Vec<(String, i64, f64)>, sqlx::Error> = sqlx::query_as("SELECT TO_CHAR(timestamp, 'YYYY-MM-DD'), COUNT(*), SUM(amount) FROM mined_blocks WHERE wallet = $1 GROUP BY TO_CHAR(timestamp, 'YYYY-MM-DD') ORDER BY TO_CHAR(timestamp, 'YYYY-MM-DD') DESC LIMIT 5").bind(&w).fetch_all(&ctx.pool).await;
            if let Ok(records) = daily_records {
                if !records.is_empty() {
                    text.push_str("├ <b>Daily Breakdown:</b>\n");
                    for (date, count, amount) in records {
                        text.push_str(&format!(
                            "│  ▪ <code>{}</code>: {} Blks ({:.2} KAS)\n",
                            date, count, amount
                        ));
                    }
                }
            }
            text.push('\n');
        }
    }
    text.push_str(&format!(
        "━━━━━━━━━━━━━━━━━━\n🏆 <b>Total Blocks:</b> {}\n💎 <b>Total Value:</b> {:.8} KAS",
        global_blocks, global_rewards
    ));
    let _ = send_or_edit_log(
        &bot,
        chat_id,
        edit_msg_id,
        text,
        Some(refresh_markup("refresh_blocks")),
    )
    .await;
}

pub async fn handle_miner(
    bot: Bot,
    chat_id: ChatId,
    ctx: &AppContext,
    current_utc_time: String,
    edit_msg_id: Option<teloxide::types::MessageId>,
) {
    let tracked: Vec<String> = ctx
        .state
        .iter()
        .filter(|e| e.value().contains(&chat_id.0))
        .map(|e| e.key().clone())
        .collect();
    if tracked.is_empty() {
        return;
    }
    let mut text = format!(
        "⛏️ <b>Solo-Miner Hashrate</b>\n⏱️ <code>{}</code>\n━━━━━━━━━━━━━━━━━━\n",
        current_utc_time
    );
    if let Ok(dag_info) = ctx.rpc.get_block_dag_info().await {
        if let Ok(net_hashrate) = ctx.rpc.estimate_network_hashes_per_second(1000, None).await {
            let net_hashrate = net_hashrate as f64;
            for w in tracked {
                let db_1h: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM mined_blocks WHERE wallet = $1 AND timestamp >= CURRENT_TIMESTAMP - INTERVAL '1 hour'").bind(&w).fetch_one(&ctx.pool).await.unwrap_or((0,));
                let db_24h: (i64,) = sqlx::query_as("SELECT COUNT(*) FROM mined_blocks WHERE wallet = $1 AND timestamp >= CURRENT_TIMESTAMP - INTERVAL '24 hours'").bind(&w).fetch_one(&ctx.pool).await.unwrap_or((0,));
                let mut live_1h = 0;
                let mut live_24h = 0;
                if let Ok(addr) = Address::try_from(w.as_str()) {
                    if let Ok(utxos) = ctx.rpc.get_utxos_by_addresses(vec![addr]).await {
                        for u in utxos.into_iter().filter(|u| u.utxo_entry.is_coinbase) {
                            let age = dag_info
                                .virtual_daa_score
                                .saturating_sub(u.utxo_entry.block_daa_score);
                            if age <= 3600 {
                                live_1h += 1;
                            }
                            if age <= 86400 {
                                live_24h += 1;
                            }
                        }
                    }
                }
                text.push_str(&format!("💼 <b>{}</b>\n📊 <b>Actual Hashrate (Database):</b>\n├ 1 Hour: {} ({} Blks)\n├ 24 Hours: {} ({} Blks)\n⚡ <b>Unspent Hashrate (Live Node):</b>\n├ 1 Hour: {} ({} Blks)\n└ 24 Hours: {} ({} Blks)\n\n", format_short_wallet(&w), crate::kaspa_features::format_hashrate(net_hashrate * (db_1h.0 as f64 / 3600.0)), db_1h.0, crate::kaspa_features::format_hashrate(net_hashrate * (db_24h.0 as f64 / 86400.0)), db_24h.0, crate::kaspa_features::format_hashrate(net_hashrate * (live_1h as f64 / 3600.0)), live_1h, crate::kaspa_features::format_hashrate(net_hashrate * (live_24h as f64 / 86400.0)), live_24h));
            }
            text.push_str(&format!(
                "━━━━━━━━━━━━━━━━━━\n🌐 <b>Global Network Hashrate:</b> {}",
                crate::kaspa_features::format_hashrate(net_hashrate)
            ));
        }
    }
    let _ = send_or_edit_log(
        &bot,
        chat_id,
        edit_msg_id,
        text,
        Some(refresh_markup("refresh_miner")),
    )
    .await;
}

pub async fn handle_network(
    bot: Bot,
    chat_id: ChatId,
    ctx: &AppContext,
    current_utc_time: String,
    edit_msg_id: Option<teloxide::types::MessageId>,
) {
    let mut text = String::from("🛠️ <b>Node Health & Network</b>\n");
    if let Ok(info) = ctx.rpc.get_server_info().await {
        text.push_str(&format!(
            "├ <b>Version:</b> {} | <b>Net:</b> {}\n├ <b>UTXO Index:</b> {}\n",
            info.server_version,
            info.network_id,
            if info.has_utxo_index {
                "Enabled ✅"
            } else {
                "Disabled ❌"
            }
        ));
    }
    if let Ok(peers) = ctx.rpc.get_connected_peer_info().await {
        text.push_str(&format!(
            "├ <b>Connected Peers:</b> {}\n",
            peers.peer_info.len()
        ));
    }
    if let Ok(sync) = ctx.rpc.get_sync_status().await {
        text.push_str(&format!(
            "└ <b>Sync Status:</b> {}\n\n",
            if sync {
                "100% Synced ✅"
            } else {
                "Syncing ⚠️"
            }
        ));
    }
    text.push_str("📊 <b>GHOSTDAG Consensus</b>\n");
    if let Ok(dag) = ctx.rpc.get_block_dag_info().await {
        text.push_str(&format!(
            "├ <b>Total Blocks:</b> {}\n├ <b>DAA Score:</b> {}\n├ <b>Difficulty:</b> {}\n",
            f_num(dag.block_count as f64),
            dag.virtual_daa_score,
            crate::kaspa_features::format_difficulty(dag.difficulty)
        ));
    }
    if let Ok(hashrate) = ctx.rpc.estimate_network_hashes_per_second(1000, None).await {
        text.push_str(&format!(
            "├ <b>Hashrate:</b> {}\n",
            crate::kaspa_features::format_hashrate(hashrate as f64)
        ));
    }
    if let Ok(supply) = ctx.rpc.get_coin_supply().await {
        let circ = supply.circulating_sompi as f64 / 1e8;
        let max = supply.max_sompi as f64 / 1e8;
        text.push_str(&format!(
            "├ <b>Circulating:</b> {} KAS\n└ <b>Minted:</b> {:.2}%\n\n",
            f_num(circ),
            (circ / max) * 100.0
        ));
    }
    text.push_str(&format!("\n⏱️ <code>{}</code>", current_utc_time));
    let _ = send_or_edit_log(
        &bot,
        chat_id,
        edit_msg_id,
        text,
        Some(refresh_markup("refresh_network")),
    )
    .await;
}

pub async fn handle_dag(
    bot: Bot,
    chat_id: ChatId,
    ctx: &AppContext,
    current_utc_time: String,
    edit_msg_id: Option<teloxide::types::MessageId>,
) {
    if let Ok(info) = ctx.rpc.get_block_dag_info().await {
        let text = format!("📊 <b>BlockDAG Details:</b>\n🧱 <b>Blocks:</b> <code>{}</code>\n📜 <b>Headers:</b> <code>{}</code>\n\n⏱️ <code>{}</code>", f_num(info.block_count as f64), f_num(info.header_count as f64), current_utc_time);
        let _ = send_or_edit_log(
            &bot,
            chat_id,
            edit_msg_id,
            text,
            Some(refresh_markup("refresh_dag")),
        )
        .await;
    }
}

pub async fn handle_price(
    bot: Bot,
    chat_id: ChatId,
    ctx: &AppContext,
    current_utc_time: String,
    edit_msg_id: Option<teloxide::types::MessageId>,
) {
    let price = ctx.price_cache.read().await.0;
    let text = if price > 0.0 {
        format!(
            "💵 <b>Price:</b> <code>${:.4} USD</code> (CoinGecko)\n\n⏱️ <code>{}</code>",
            price, current_utc_time
        )
    } else {
        format!(
            "⚠️ <b>Price API Syncing...</b>\n\n⏱️ <code>{}</code>",
            current_utc_time
        )
    };
    let _ = send_or_edit_log(
        &bot,
        chat_id,
        edit_msg_id,
        text,
        Some(refresh_markup("refresh_price")),
    )
    .await;
}

pub async fn handle_market(
    bot: Bot,
    chat_id: ChatId,
    ctx: &AppContext,
    current_utc_time: String,
    edit_msg_id: Option<teloxide::types::MessageId>,
) {
    let mcap = ctx.price_cache.read().await.1;
    let text = if mcap > 0.0 {
        format!(
            "📈 <b>Market Cap:</b> <code>${} USD</code> (CoinGecko)\n\n⏱️ <code>{}</code>",
            f_num(mcap),
            current_utc_time
        )
    } else {
        format!(
            "⚠️ <b>Market Cap API Syncing...</b>\n\n⏱️ <code>{}</code>",
            current_utc_time
        )
    };
    let _ = send_or_edit_log(
        &bot,
        chat_id,
        edit_msg_id,
        text,
        Some(refresh_markup("refresh_market")),
    )
    .await;
}

pub async fn handle_supply(
    bot: Bot,
    chat_id: ChatId,
    ctx: &AppContext,
    current_utc_time: String,
    edit_msg_id: Option<teloxide::types::MessageId>,
) {
    if let Ok(supply) = ctx.rpc.get_coin_supply().await {
        let circ = supply.circulating_sompi as f64 / 1e8;
        let max = supply.max_sompi as f64 / 1e8;
        let text = format!("🪙 <b>Coin Supply:</b>\n├ <b>Circulating:</b> <code>{} KAS</code>\n├ <b>Max Supply:</b> <code>{} KAS</code>\n└ <b>Minted:</b> <code>{:.2}%</code>\n\n⏱️ <code>{}</code>", f_num(circ), f_num(max), (circ / max) * 100.0, current_utc_time);
        let _ = send_or_edit_log(
            &bot,
            chat_id,
            edit_msg_id,
            text,
            Some(refresh_markup("refresh_supply")),
        )
        .await;
    }
}

pub async fn handle_fees(
    bot: Bot,
    chat_id: ChatId,
    current_utc_time: String,
    edit_msg_id: Option<teloxide::types::MessageId>,
) {
    if let Ok(r) = reqwest::get("https://api.kaspa.org/info/fee-estimate").await {
        if let Ok(j) = r.json::<serde_json::Value>().await {
            let text = format!(
                "⛽ <b>Fee Estimate:</b> <code>{:.2} sompi/gram</code>\n\n⏱️ <code>{}</code>",
                j["normalBuckets"][0]["feerate"].as_f64().unwrap_or(0.0),
                current_utc_time
            );
            let _ = send_or_edit_log(
                &bot,
                chat_id,
                edit_msg_id,
                text,
                Some(refresh_markup("refresh_fees")),
            )
            .await;
        }
    }
}

