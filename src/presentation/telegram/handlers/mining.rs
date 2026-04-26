use crate::domain::models::AppContext;
use crate::network::stats_use_cases::GetMinerStatsUseCase;
use std::sync::Arc;
use teloxide::prelude::*;

pub async fn handle_blocks(
    bot: teloxide::prelude::Bot,
    msg: teloxide::prelude::Message,
    cid: i64,
    wallet_query: std::sync::Arc<crate::wallet::wallet_use_cases::WalletQueriesUseCase>,
    _app_context: std::sync::Arc<crate::domain::models::AppContext>,
) -> anyhow::Result<()> {
    match wallet_query.get_blocks_stats(cid).await {
        Ok((b1h, b24h, total_lifetime, daily_data)) => {
            let mut daily_breakdown = String::new();
            if !daily_data.is_empty() {
                daily_breakdown.push_str("\n📅 <b>Last 7 Days:</b>\n");
                for (day, count) in daily_data.iter().take(7) {
                    daily_breakdown
                        .push_str(&format!("├ <code>{}</code>: {} blocks\n", day, count));
                }
            }

            let text = format!("🧱 <b>Mined Blocks Forensics</b>\n━━━━━━━━━━━━━━━━━━\n⏱️ <b>Last 1 Hour:</b> <code>{}</code>\n⏳ <b>Last 24 Hours:</b> <code>{}</code>\n🏆 <b>Lifetime Blocks:</b> <code>{}</code>\n📈 <b>Mining Status:</b> {}{}", b1h, b24h, total_lifetime, if b1h > 0 { "Active 🟢" } else { "Idle 🟡" }, daily_breakdown);
            let markup = crate::utils::refresh_markup("refresh_blocks");
            let _ = crate::utils::send_or_edit_log(&bot, msg.chat.id, msg.from.as_ref().filter(|u| u.is_bot).map(|_| msg.id), text, Some(markup)).await;
        }
        Err(e) => {
            crate::send_logged!(bot, msg, format!("❌ Error: {}", e));
        }
    }
    Ok(())
}

pub async fn handle_miner(
    bot: Bot,
    msg: Message,
    cid: i64,
    app_context: Arc<AppContext>,
    miner_stats: Arc<GetMinerStatsUseCase>,
) -> anyhow::Result<()> {
    let tracked: Vec<String> =
        sqlx::query_scalar("SELECT wallet FROM user_wallets WHERE chat_id = $1")
            .bind(cid)
            .fetch_all(&app_context.pool)
            .await
            .unwrap_or_default();
    if tracked.is_empty() {
        crate::send_logged!(
            bot,
            msg,
            "⚠️ <b>No wallets tracked.</b> Use /add to track one."
        );
    } else {
        let mut text = format!("⛏️ <b>Solo-Miner Hashrate (Enterprise Engine)</b>\n⏱️ <code>{}</code>\n━━━━━━━━━━━━━━━━━━\n", chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC"));
        for w in &tracked {
            match miner_stats.execute(w).await {
                Ok(stats) => {
                    text.push_str(&format!("💼 <b>{}</b>\n🌐 <b>Global Hashrate:</b> <code>{}</code>\n📊 <b>Actual Hashrate:</b>\n├ 1H: <code>{}</code> | 24H: <code>{}</code>\n⚡ <b>Unspent Hashrate:</b>\n├ 1H: <code>{}</code> | 24H: <code>{}</code>\n\n",
                        stats.wallet_address, stats.global_network_hashrate, stats.actual_hashrate_1h, stats.actual_hashrate_24h, stats.unspent_hashrate_1h, stats.unspent_hashrate_24h));
                }
                Err(_) => {
                    text.push_str(&format!(
                        "💼 <b>{}</b>\n⚠️ Error fetching stats.\n\n",
                        crate::utils::format_short_wallet(w)
                    ));
                }
            }
        }
        let markup = crate::utils::refresh_markup("refresh_miner");
        let _ = crate::utils::send_or_edit_log(&bot, msg.chat.id, msg.from.as_ref().filter(|u| u.is_bot).map(|_| msg.id), text, Some(markup)).await;
    }
    Ok(())
}

