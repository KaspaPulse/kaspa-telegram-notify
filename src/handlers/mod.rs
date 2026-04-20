#![allow(clippy::too_many_arguments, clippy::redundant_pattern_matching)]

pub mod admin;
pub mod events;
pub mod public;

use chrono::Utc;
use teloxide::{prelude::*, types::ChatId};
use tokio::time::Instant;

use crate::commands::Command;
use crate::context::AppContext;

// Re-export event handlers for main.rs
pub use events::{handle_block_user, handle_raw_message_v2};

// ==========================================
// SECTION 0: SAFETY & SECURITY PROTOCOLS
// ==========================================
pub fn is_local_node() -> bool {
    // 1. Fetch all possible variable names including WS_URL
    let urls = format!(
        "{} {} {} {} {}",
        std::env::var("KASPA_RPC_URL").unwrap_or_default(),
        std::env::var("KASPA_NODE_URL").unwrap_or_default(),
        std::env::var("RPC_URL").unwrap_or_default(),
        std::env::var("NODE_URL").unwrap_or_default(),
        std::env::var("WS_URL").unwrap_or_default()
    )
    .to_lowercase();

    // 2. Empty means FALSE (Not Local)
    if urls.trim().is_empty() {
        return false;
    }

    // 3. Only return true if an explicit local IP is found
    urls.contains("127.0.0.1")
        || urls.contains("localhost")
        || urls.contains("::1")
        || urls.contains("0.0.0.0")
}

// ==========================================
// SECTION 1: ROUTING CONTROLLERS
// ==========================================
pub async fn handle_command(
    bot: Bot,
    msg: Message,
    cmd: Command,
    ctx: AppContext,
) -> anyhow::Result<()> {
    let chat_id = msg.chat.id;
    let user_id = msg.from.as_ref().map(|u| u.id.0 as i64).unwrap_or(0);
    execute_command(bot, chat_id, user_id, cmd, ctx, None).await
}

pub async fn handle_callback(
    bot: Bot,
    q: teloxide::types::CallbackQuery,
    ctx: AppContext,
) -> anyhow::Result<()> {
    let user_id = q.from.id.0 as i64;
    if crate::utils::is_spam(user_id) {
        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("⚠️ Processing... Please wait!")
            .show_alert(false)
            .await;
        return Ok(());
    }

    if let Some(data) = q.data.clone() {
        if data == "admin_sync_blocks" {
            if user_id == ctx.admin_id {
                let chat_id = q
                    .regular_message()
                    .map(|m| m.chat.id)
                    .unwrap_or(ChatId(user_id));
                if is_local_node() {
                    let bot_clone = bot.clone();
                    let ctx_clone = ctx.clone();
                    bot.send_message(chat_id, "🚀 <b>Admin:</b> Global Reverse Sync started...")
                        .parse_mode(teloxide::types::ParseMode::Html)
                        .await?;
                    tokio::spawn(async move {
                        if let Err(e) =
                            crate::workers::sync_all_wallets_from_pruning_point(ctx_clone).await
                        {
                            tracing::error!("❌ [ADMIN SYNC ERROR]: {}", e);
                        } else {
                            let _ = bot_clone
                                .send_message(ChatId(user_id), "✅ <b>Global Sync Complete!</b>")
                                .parse_mode(teloxide::types::ParseMode::Html)
                                .await;
                        }
                    });
                } else {
                    if let Err(e) = bot.send_message(chat_id, "⚠️ <b>Sync Blocked (Safety Protocol)</b>\nDisabled on Public Nodes to prevent IP bans. You must connect the bot to a local node (127.0.0.1) to use this feature.").parse_mode(teloxide::types::ParseMode::Html).await { tracing::error!("[TELEGRAM API ERROR] Failed to execute: {}", e); }
                }
            }
            if let Err(e) = bot.answer_callback_query(q.id).await { tracing::error!("[TELEGRAM API ERROR] Failed to execute: {}", e); }
            return Ok(());
        }

        if let Some(msg) = q.regular_message() {
            let (cmd, is_refresh) = match data.as_str() {
                "cmd_balance" => (Some(Command::Balance), false),
                "refresh_balance" => (Some(Command::Balance), true),
                "cmd_miner" => (Some(Command::Miner), false),
                "refresh_miner" => (Some(Command::Miner), true),
                "cmd_blocks" => (Some(Command::Blocks), false),
                "refresh_blocks" => (Some(Command::Blocks), true),
                "cmd_list" => (Some(Command::List), false),
                "refresh_list" => (Some(Command::List), true),
                "cmd_price" => (Some(Command::Price), false),
                "refresh_price" => (Some(Command::Price), true),
                "cmd_market" => (Some(Command::Market), false),
                "refresh_market" => (Some(Command::Market), true),
                "cmd_network" => (Some(Command::Network), false),
                "refresh_network" => (Some(Command::Network), true),
                "cmd_fees" => (Some(Command::Fees), false),
                "refresh_fees" => (Some(Command::Fees), true),
                "cmd_supply" => (Some(Command::Supply), false),
                "refresh_supply" => (Some(Command::Supply), true),
                "cmd_dag" => (Some(Command::Dag), false),
                "refresh_dag" => (Some(Command::Dag), true),
                "cmd_stats" => (Some(Command::Stats), false),
                "refresh_stats" => (Some(Command::Stats), true),
                "cmd_sys" => (Some(Command::Sys), false),
                "refresh_sys" => (Some(Command::Sys), true),
                "cmd_donate" => (Some(Command::Donate), false),
                "cmd_logs" => (Some(Command::Logs), false),
                "cmd_pause" => (Some(Command::Pause), false),
                "cmd_resume" => (Some(Command::Resume), false),
                "cmd_restart" => (Some(Command::Restart), false),
                _ => (None, false),
            };

            if let Some(c) = cmd {
                let edit_msg_id = if is_refresh { Some(msg.id) } else { None };
                let _ =
                    execute_command(bot.clone(), msg.chat.id, user_id, c, ctx, edit_msg_id).await;
            }
        }
    }
    if let Err(e) = bot.answer_callback_query(q.id).await { tracing::error!("[TELEGRAM API ERROR] Failed to execute: {}", e); }
    Ok(())
}

async fn execute_command(
    bot: Bot,
    chat_id: ChatId,
    user_id: i64,
    cmd: Command,
    ctx: AppContext,
    edit_msg_id: Option<teloxide::types::MessageId>,
) -> anyhow::Result<()> {
    let timer = Instant::now();
    // ⏱️ التحديث: إضافة .%3f لطباعة أجزاء الثانية
    let current_utc_time = Utc::now().format("%Y-%m-%d %H:%M:%S.%3f UTC").to_string();

    match cmd {
        Command::Start => public::handle_start(bot, chat_id, user_id, &ctx).await,
        Command::Help => public::handle_help(bot, chat_id).await,
        Command::Donate => public::handle_donate(bot, chat_id).await,
        Command::Add(w) => public::handle_add(bot, chat_id, w, &ctx).await,
        Command::Remove(w) => public::handle_remove(bot, chat_id, w, &ctx).await,
        Command::List => public::handle_list(bot, chat_id, &ctx).await,
        Command::Balance => {
            public::handle_balance(bot, chat_id, &ctx, current_utc_time, edit_msg_id).await
        }
        Command::Blocks => {
            public::handle_blocks(bot, chat_id, &ctx, current_utc_time, edit_msg_id).await
        }
        Command::Miner => {
            public::handle_miner(bot, chat_id, &ctx, current_utc_time, edit_msg_id).await
        }
        Command::Network => {
            public::handle_network(bot, chat_id, &ctx, current_utc_time, edit_msg_id).await
        }
        Command::Dag => public::handle_dag(bot, chat_id, &ctx, current_utc_time, edit_msg_id).await,
        Command::Price => {
            public::handle_price(bot, chat_id, &ctx, current_utc_time, edit_msg_id).await
        }
        Command::Market => {
            public::handle_market(bot, chat_id, &ctx, current_utc_time, edit_msg_id).await
        }
        Command::Supply => {
            public::handle_supply(bot, chat_id, &ctx, current_utc_time, edit_msg_id).await
        }
        Command::Fees => public::handle_fees(bot, chat_id, current_utc_time, edit_msg_id).await,
        Command::Stats => {
            admin::handle_stats(bot, chat_id, user_id, &ctx, edit_msg_id, current_utc_time).await
        }
        Command::Sys => {
            admin::handle_sys(bot, chat_id, user_id, &ctx, edit_msg_id, current_utc_time).await
        }
        Command::Pause => admin::handle_pause(bot, chat_id, user_id, &ctx).await,
        Command::Resume => admin::handle_resume(bot, chat_id, user_id, &ctx).await,
        Command::Restart => admin::handle_restart(bot, chat_id, user_id, &ctx).await,
        Command::Broadcast(m) => admin::handle_broadcast(bot, chat_id, user_id, m, &ctx).await,
        Command::Logs => admin::handle_logs(bot, chat_id, user_id, &ctx).await,
        Command::Learn(f) => admin::handle_learn(bot, chat_id, user_id, f, &ctx).await,
        Command::AutoLearn => admin::handle_autolearn(bot, chat_id, user_id, &ctx).await,
        Command::Settings => admin::handle_settings(bot, chat_id, user_id, &ctx).await,
        Command::Toggle(f) => admin::handle_toggle(bot, chat_id, user_id, f, &ctx).await,
        Command::Sync => {
            if user_id == ctx.admin_id {
                if is_local_node() {
                    let bc = bot.clone();
                    let cc = ctx.clone();
                    bot.send_message(chat_id, "🚀 <b>Admin:</b> Manual Sync started...")
                        .parse_mode(teloxide::types::ParseMode::Html)
                        .await?;
                    tokio::spawn(async move {
                        let _ = crate::workers::sync_all_wallets_from_pruning_point(cc).await;
                        let _ = bc
                            .send_message(chat_id, "✅ <b>Sync Complete!</b>")
                            .parse_mode(teloxide::types::ParseMode::Html)
                            .await;
                    });
                } else {
                    if let Err(e) = bot.send_message(chat_id, "⚠️ <b>Sync Blocked (Safety Protocol)</b>\nDisabled on Public Nodes to prevent IP bans. You must connect the bot to a local node (127.0.0.1) to use this feature.").parse_mode(teloxide::types::ParseMode::Html).await { tracing::error!("[TELEGRAM API ERROR] Failed to execute: {}", e); }
                }
            }
        }
    };

    tracing::info!(
        "[TIME] Request processed in {}ms | ChatID: {}",
        timer.elapsed().as_millis(),
        chat_id.0
    );
    Ok(())
}

