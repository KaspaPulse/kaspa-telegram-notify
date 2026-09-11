pub mod admin;
pub mod admin_confirm;
pub mod mining;
pub mod network;
pub mod raw_message;
pub mod wallet;

use crate::domain::models::{PendingInputAction, SensitiveAction};
use crate::infrastructure::database::postgres_adapter::PostgresRepository;
use crate::network::stats_use_cases::{
    GetMarketStatsUseCase, GetMinerStatsUseCase, NetworkStatsUseCase,
};
use crate::presentation::telegram::commands::Command;
use crate::wallet::wallet_use_cases::{WalletManagementUseCase, WalletQueriesUseCase};
use std::sync::Arc;
use std::sync::atomic::Ordering;
use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, ParseMode};

#[derive(Clone)]
#[allow(dead_code)]
pub struct BotUseCases {
    pub wallet_mgt: Arc<WalletManagementUseCase>,
    pub wallet_query: Arc<WalletQueriesUseCase>,
    pub network_stats: Arc<NetworkStatsUseCase>,
    pub market_stats: Arc<GetMarketStatsUseCase>,
    pub miner_stats: Arc<GetMinerStatsUseCase>,
    pub dag_uc: Arc<crate::network::analyze_dag::AnalyzeDagUseCase>,
}

fn callback_disables_keyboard(data: &str) -> bool {
    data.starts_with("admin_do:")
        || data.starts_with("rm_wallet_")
        || data.starts_with("wallet_remove_do_")
        || data.starts_with("btn_toggle_")
        || matches!(
            data,
            "do_pause"
                | "do_resume"
                | "do_restart"
                | "do_cleanup_events"
                | "do_mute_alerts"
                | "do_unmute_alerts"
                | "do_forget_wallets"
                | "do_forget_all"
        )
}

async fn disable_callback_keyboard(
    bot: &Bot,
    q: &teloxide::types::CallbackQuery,
) -> anyhow::Result<()> {
    if let Some(message) = q.message.as_ref() {
        bot.edit_message_reply_markup(message.chat().id, message.id())
            .await?;
    }

    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum CallbackRecoveryMenu {
    Admin,
    Main,
    Wallet,
}

fn callback_recovery_menu(data: &str, callback_is_admin: bool) -> CallbackRecoveryMenu {
    let action = match data {
        "do_forget_wallets" => Some(SensitiveAction::ClearWallets),
        "do_forget_all" => Some(SensitiveAction::ForgetAll),
        _ => data
            .strip_prefix("admin_do:")
            .and_then(|remainder| remainder.split(':').next())
            .and_then(SensitiveAction::parse),
    };

    match action {
        Some(SensitiveAction::ClearWallets) => CallbackRecoveryMenu::Wallet,
        Some(SensitiveAction::ForgetAll) => CallbackRecoveryMenu::Main,
        Some(action) if action.requires_admin() => {
            if callback_is_admin {
                CallbackRecoveryMenu::Admin
            } else {
                CallbackRecoveryMenu::Main
            }
        }
        Some(_) => CallbackRecoveryMenu::Main,
        None if callback_is_admin => CallbackRecoveryMenu::Admin,
        None => CallbackRecoveryMenu::Main,
    }
}

fn callback_recovery_markup(menu: CallbackRecoveryMenu) -> InlineKeyboardMarkup {
    match menu {
        CallbackRecoveryMenu::Admin => {
            crate::presentation::telegram::menus::TelegramMenus::admin_menu_markup()
        }
        CallbackRecoveryMenu::Main => {
            crate::presentation::telegram::menus::TelegramMenus::main_menu_markup()
        }
        CallbackRecoveryMenu::Wallet => {
            crate::presentation::telegram::menus::TelegramMenus::wallet_menu_markup()
        }
    }
}

fn safe_callback_menu(callback_is_admin: bool) -> InlineKeyboardMarkup {
    callback_recovery_markup(if callback_is_admin {
        CallbackRecoveryMenu::Admin
    } else {
        CallbackRecoveryMenu::Main
    })
}

async fn edit_callback_state(
    bot: &Bot,
    q: &teloxide::types::CallbackQuery,
    text: impl Into<String>,
    markup: InlineKeyboardMarkup,
) {
    let Some(message) = q.message.as_ref() else {
        return;
    };

    let chat_id = message.chat().id;
    let message_id = message.id();
    let text = text.into();
    let fallback_markup = markup.clone();

    if let Err(edit_error) = bot
        .edit_message_text(chat_id, message_id, text.clone())
        .parse_mode(ParseMode::Html)
        .reply_markup(markup)
        .await
    {
        tracing::warn!(
            "[CALLBACK UI] Failed to restore message {} in chat {}: {}",
            message_id.0,
            chat_id.0,
            edit_error
        );

        if let Err(send_error) = bot
            .send_message(chat_id, text)
            .parse_mode(ParseMode::Html)
            .reply_markup(fallback_markup)
            .await
        {
            tracing::error!(
                "[CALLBACK UI] Failed to send a replacement action panel in chat {} after edit failure: {}",
                chat_id.0,
                send_error
            );
        }
    }
}

async fn restore_safe_callback_menu(
    bot: &Bot,
    q: &teloxide::types::CallbackQuery,
    callback_is_admin: bool,
    text: impl Into<String>,
) {
    edit_callback_state(bot, q, text, safe_callback_menu(callback_is_admin)).await;
}

async fn restore_wallet_callback_menu(
    bot: &Bot,
    q: &teloxide::types::CallbackQuery,
    text: impl Into<String>,
) {
    edit_callback_state(
        bot,
        q,
        text,
        callback_recovery_markup(CallbackRecoveryMenu::Wallet),
    )
    .await;
}

async fn restore_contextual_callback_menu(
    bot: &Bot,
    q: &teloxide::types::CallbackQuery,
    data: &str,
    callback_is_admin: bool,
    text: impl Into<String>,
) {
    edit_callback_state(
        bot,
        q,
        text,
        callback_recovery_markup(callback_recovery_menu(data, callback_is_admin)),
    )
    .await;
}

#[allow(clippy::too_many_arguments)]
pub fn handle_command(
    bot: Bot,
    msg: Message,
    cmd: Command,
    ucs: BotUseCases,
    app_context: Arc<crate::domain::models::AppContext>,
) -> std::pin::Pin<Box<dyn std::future::Future<Output = anyhow::Result<()>> + Send>> {
    Box::pin(async move {
        let identity = match crate::presentation::telegram::request_identity::from_message(&msg) {
            Ok(identity) => identity,
            Err(reason) => {
                let _ = bot.send_message(msg.chat.id, reason).await;
                return Ok(());
            }
        };

        let chat_id = msg.chat.id;
        let cid = identity.chat_id;
        let actor_user_id = identity.actor_user_id;
        let is_admin =
            identity.is_private_admin(app_context.admin_user_id, app_context.admin_chat_id);

        crate::utils::log_multiline(
            &format!(
                "BOT IN | Actor: {} | Chat: {} | User: {}",
                actor_user_id,
                cid,
                msg.from
                    .as_ref()
                    .and_then(|u| u.username.clone())
                    .unwrap_or_else(|| "Unknown".to_string())
            ),
            msg.text().unwrap_or("Callback/System"),
            false,
        );

        if cmd.is_admin_only() && !is_admin {
            crate::send_logged!(
                bot,
                msg,
                "⛔ <b>Admin commands are available only to the configured admin user in the private admin chat.</b>"
            );
            return Ok(());
        }

        if app_context.maintenance_mode.load(Ordering::Relaxed) && !is_admin {
            let _ = bot
                .send_message(
                    chat_id,
                    "🚧 <b>Maintenance Mode</b>\nThe bot is currently under maintenance.",
                )
                .parse_mode(ParseMode::Html)
                .await;
            return Ok(());
        }

        if !is_admin && crate::utils::is_command_rate_limited(actor_user_id) {
            crate::send_logged!(bot, msg, crate::utils::rate_limit_message());
            return Ok(());
        }
        match cmd {
            Command::Forget | Command::ForgetAll => {
                admin_confirm::send_command_confirmation(
                    &bot,
                    &app_context,
                    identity,
                    SensitiveAction::ForgetAll,
                )
                .await?;
            }

            Command::ForgetWallets => {
                admin_confirm::send_command_confirmation(
                    &bot,
                    &app_context,
                    identity,
                    SensitiveAction::ClearWallets,
                )
                .await?;
            }

            Command::HideMenu => {
                let _ = bot
                    .send_message(msg.chat.id, "✅ تم إخفاء القائمة الثابتة من الجوال بنجاح.")
                    .reply_markup(teloxide::types::KeyboardRemove::new())
                    .await;
            }

            Command::Help => {
                let help_text = r#"📚 <b>Kaspa Pulse Help</b>
━━━━━━━━━━━━━━━━━━
<b>Community Mining Alerts</b>

Kaspa Pulse monitors Kaspa wallets, detects native node mining rewards, identifies the real mined block, and sends Telegram alerts after reward confirmation.

🚀 <b>Quick Start</b>
• Press <b>Wallets</b> from /start to manage your wallets.
• Press <b>Add Wallet</b> or paste any <code>kaspa:...</code> address in chat.
• Use <b>Remove Wallet</b> to choose a wallet from buttons.
• <b>Clear Wallets</b> and <b>Delete My Data</b> require confirmation before deletion.
• Mining rewards are not sent immediately; the bot waits for reward confirmations before DAG analysis and alert delivery.

✅ <b>Reward Confirmation Policy</b>
• Rewards are first detected from wallet UTXOs.
• The bot waits until the reward reaches the configured confirmation threshold.
• Default confirmation threshold: <b>10 DAA confirmations</b>.
• After confirmation, the bot analyzes the DAG to find:
  - Accepting Block
  - Real Mined Block
  - Worker name
  - Nonce / block details
• If some candidate DAG blocks are unavailable, the bot skips them safely and continues searching.

👛 <b>Wallet Commands</b>
• /add <code>kaspa:...</code> - Track a wallet.
• /remove <code>kaspa:...</code> - Stop tracking a wallet.
• /list - Show Wallet 1, Wallet 2, and all tracked addresses.
• /balance - Show total balance plus per-wallet balance, value, UTXOs, and status.

👛 <b>Wallet Buttons</b>
• <b>Wallets</b> - Open wallet management panel.
• <b>Add Wallet</b> - Add a new wallet.
• <b>Remove Wallet</b> - Remove one wallet by button.
• <b>Clear Wallets</b> - Remove all tracked wallets after confirmation.
• <b>Delete My Data</b> - Delete all user data after confirmation.
• <b>Back</b> - Return to main menu.

⛏️ <b>Mining Commands</b>
• /blocks - Show total mined blocks plus per-wallet block stats.
• /miner - Estimate solo-mining hashrate for your tracked wallets.

⛏️ <b>Mining Alert Details</b>
Each confirmed mining alert may include:
• Reward time
• Wallet
• Reward amount
• Live balance
• TXID
• Real mined block
• Accepting block
• Worker name
• DAA score"#;

                let help_text_2 = r#"🌐 <b>Network &amp; Market</b>
• /network - Show node status, peers, sync status, Live BPS, and Expected BPS.
• /dag - Show BlockDAG overview, pruning point, readable pruning time, and BPS.
• /price - Show KAS price, market cap, hashrate, peers, pruning point, and BPS.
• /supply - Show circulating supply, max supply, and minted percentage.
• /fees - Show current network fee estimate.

🌐 <b>Network Buttons</b>
• <b>Network</b> - Node and sync status.
• <b>DAG</b> - BlockDAG overview.
• <b>Price</b> - KAS price and market info.
• <b>Supply</b> - Supply and minted percentage.
• <b>Fees</b> - Current network fee estimate.

❤️ <b>Support</b>
• /donate - Show the donation address.

🛡️ <b>Owner Commands</b>
• /health - Production health report.
• /settings - Settings panel.
• /stats - System counters and bot statistics.
• /sys - System diagnostics such as RAM, swap, and server time.
• /logs - Recent service logs.
• /db_diag - Database diagnostics.
• /events - Latest 10 compact bot events.
• /errors - Recent error events.
• /delivery - Alert delivery summary.
• /subscribers - Subscriber summary.
• /wallet_events - Wallet event activity.
• /cleanup_events - Clean old bot events.
• /pause - Pause live monitoring.
• /resume - Resume live monitoring.
• /restart - Restart the service.

🛡️ <b>Owner Buttons</b>
• <b>Health</b> - Production health report.
• <b>Settings</b> - Open settings panel.
• <b>Stats</b> - System statistics.
• <b>System</b> - Server diagnostics.
• <b>Logs</b> - Recent logs.
• <b>DB Diagnostics</b> - Database checks.
• <b>Events</b> - Latest compact event log.
• <b>Errors</b> - Recent errors.
• <b>Delivery</b> - Alert delivery summary.
• <b>Subscribers</b> - Subscriber information.
• <b>Wallet Events</b> - Wallet-related activity.
• <b>Cleanup Events</b> - Purge old event logs.
• <b>Pause</b> - Pause monitoring.
• <b>Resume</b> - Resume monitoring.
• <b>Restart</b> - Restart service.

⚙️ <b>System Behavior</b>
• Telegram commands are synced automatically at startup.
• Old deleted Telegram commands are cleared before syncing new commands.
• Important events are recorded in <code>bot_event_log</code>.
• Startup, webhook start, shutdown, delivery, duplicate alerts, DB errors, RPC errors, and panic recovery are logged.
• Panic markers are recovered after restart and shown in /errors.
• Memory cleaner removes old runtime state, old dedup records, and old seen UTXOs.

🧪 <b>Production Safety</b>
• DAG analysis does not stop when a candidate block is unavailable.
• Missing candidate DAG blocks are skipped safely.
• Critical RPC/DB failures are logged.
• Duplicate alerts are prevented using alert deduplication.
• Regression tests protect the alert flow from breaking changes.

ℹ️ <i>Tip: Most actions are easier from the /start buttons.</i>
For mining alerts, wait for the configured confirmations before expecting Telegram delivery."#;

                crate::send_logged!(bot, msg, help_text);
                crate::send_logged!(bot, msg, help_text_2);
            }
            Command::Start => {
                let markup = if is_admin {
                    crate::presentation::telegram::menus::TelegramMenus::admin_menu_markup()
                } else {
                    crate::presentation::telegram::menus::TelegramMenus::main_menu_markup()
                };

                let welcome = "🤖 <b>Kaspa Pulse</b>\nCommunity Mining Alerts\n━━━━━━━━━━━━━━━━━━\nTrack Kaspa wallets, monitor solo-mining rewards, and receive live alerts.\n\n⚡ <b>Quick Start:</b>\nPaste any <code>kaspa:...</code> address in this chat to activate tracking.\n\n👇 <i>Select an option below or type /help for commands.</i>";

                let _ = crate::utils::send_logged_message(
                    &bot,
                    msg.chat.id,
                    Some(msg.id),
                    welcome.to_string(),
                    Some(markup),
                )
                .await;
            }

            Command::Donate => {
                crate::send_logged!(
                    bot,
                    msg,
                    "❤️ <b>Support Development</b>\n\n<b>KAS Address:</b>\n<code>kaspa:qz0yqq8z3twwgg7lq2mjzg6w4edqys45w2wslz7tym2tc6s84580vvx9zr44g</code>"
                );
            }

            Command::Add(wallet) => {
                wallet::handle_add(bot, msg, cid, actor_user_id, wallet, ucs.wallet_mgt).await?
            }
            Command::Remove(wallet) => {
                wallet::handle_remove(bot, msg, cid, wallet, ucs.wallet_mgt).await?
            }
            Command::List => wallet::handle_list(bot, msg, cid, ucs.wallet_query).await?,
            Command::Balance => {
                wallet::handle_balance(bot, msg, cid, ucs.wallet_query, app_context).await?
            }

            Command::Blocks => {
                mining::handle_blocks(bot, msg, cid, ucs.wallet_query, app_context).await?
            }
            Command::Miner => {
                mining::handle_miner(bot, msg, cid, app_context, ucs.miner_stats).await?
            }

            Command::Network => {
                network::handle_network_overview(bot, msg, app_context, ucs.network_stats).await?
            }
            Command::Dag => network::handle_dag(bot, msg, app_context, ucs.dag_uc.clone()).await?,
            Command::Fees => network::handle_fees(bot, msg).await?,
            Command::Supply => network::handle_supply(bot, msg, app_context).await?,
            Command::Price | Command::Market => {
                network::handle_market_data(
                    bot.clone(),
                    msg.clone(),
                    app_context.clone(),
                    ucs.market_stats.clone(),
                )
                .await?
            }

            Command::Health => {
                if !is_admin {
                    crate::send_logged!(bot, msg, "⛔ Unauthorized.");
                    return Ok(());
                }
                admin::handle_health(bot, msg, app_context).await?;
            }
            Command::Pause => {
                if !is_admin {
                    crate::send_logged!(bot, msg, "⛔ Unauthorized.");
                    return Ok(());
                }

                crate::presentation::telegram::handlers::admin_confirm::send_command_confirmation(
                    &bot,
                    &app_context,
                    identity,
                    SensitiveAction::Pause,
                )
                .await?;
            }
            Command::Resume => {
                if !is_admin {
                    crate::send_logged!(bot, msg, "⛔ Unauthorized.");
                    return Ok(());
                }

                crate::presentation::telegram::handlers::admin_confirm::send_command_confirmation(
                    &bot,
                    &app_context,
                    identity,
                    SensitiveAction::Resume,
                )
                .await?;
            }
            Command::MuteAlerts => {
                if !is_admin {
                    crate::send_logged!(bot, msg, "⛔ Unauthorized.");
                    return Ok(());
                }

                crate::presentation::telegram::handlers::admin_confirm::send_command_confirmation(
                    &bot,
                    &app_context,
                    identity,
                    SensitiveAction::MuteAlerts,
                )
                .await?;
            }
            Command::UnmuteAlerts => {
                if !is_admin {
                    crate::send_logged!(bot, msg, "⛔ Unauthorized.");
                    return Ok(());
                }

                crate::presentation::telegram::handlers::admin_confirm::send_command_confirmation(
                    &bot,
                    &app_context,
                    identity,
                    SensitiveAction::UnmuteAlerts,
                )
                .await?;
            }
            Command::AlertsStatus => {
                if !is_admin {
                    crate::send_logged!(bot, msg, "⛔ Unauthorized.");
                    return Ok(());
                }

                admin::handle_alerts_status(bot, msg, app_context).await?;
            }
            Command::Restart => {
                if !is_admin {
                    crate::send_logged!(bot, msg, "⛔ Unauthorized.");
                    return Ok(());
                }

                crate::presentation::telegram::handlers::admin_confirm::send_command_confirmation(
                    &bot,
                    &app_context,
                    identity,
                    SensitiveAction::Restart,
                )
                .await?;
            }
            Command::Stats => {
                if !is_admin {
                    crate::send_logged!(bot, msg, "⛔ Unauthorized.");
                    return Ok(());
                }
                admin::handle_stats(bot, msg, app_context).await?;
            }
            Command::Toggle(flag) => {
                if !is_admin {
                    crate::send_logged!(bot, msg, "⛔ Unauthorized.");
                    return Ok(());
                }

                if let Some(action) =
                    crate::presentation::telegram::handlers::admin_confirm::sensitive_action_from_toggle_flag(&flag)
                {
                    crate::presentation::telegram::handlers::admin_confirm::send_command_confirmation(
                        &bot,
                        &app_context,
                        identity,
                        action,
                    )
                    .await?;
                } else {
                    admin::handle_toggle(bot, msg, flag, app_context).await?;
                }
            }
            Command::Sys => {
                if !is_admin {
                    crate::send_logged!(bot, msg, "⛔ Unauthorized.");
                    return Ok(());
                }
                admin::handle_sys(
                    bot,
                    msg,
                    app_context.live_sync_enabled.load(Ordering::Relaxed),
                )
                .await?;
            }
            Command::Errors => {
                if !is_admin {
                    crate::send_logged!(bot, msg, "⛔ Unauthorized.");
                    return Ok(());
                }
                admin::handle_errors(bot, msg, app_context).await?
            }
            Command::Delivery => {
                if !is_admin {
                    crate::send_logged!(bot, msg, "⛔ Unauthorized.");
                    return Ok(());
                }
                admin::handle_delivery(bot, msg, app_context).await?
            }
            Command::Subscribers(wallet) => {
                if !is_admin {
                    crate::send_logged!(bot, msg, "⛔ Unauthorized.");
                    return Ok(());
                }
                admin::handle_subscribers(bot, msg, wallet, app_context).await?
            }
            Command::WalletEvents(wallet) => {
                if !is_admin {
                    crate::send_logged!(bot, msg, "⛔ Unauthorized.");
                    return Ok(());
                }
                admin::handle_wallet_events(bot, msg, wallet, app_context).await?
            }
            Command::CleanupEvents => {
                if !is_admin {
                    crate::send_logged!(bot, msg, "⛔ Unauthorized.");
                    return Ok(());
                }

                crate::presentation::telegram::handlers::admin_confirm::send_command_confirmation(
                    &bot,
                    &app_context,
                    identity,
                    SensitiveAction::CleanupEvents,
                )
                .await?;
            }

            Command::Events => {
                if !is_admin {
                    crate::send_logged!(bot, msg, "⛔ Unauthorized.");
                    return Ok(());
                }
                admin::handle_events(bot, msg, app_context).await?
            }

            Command::Logs => {
                if !is_admin {
                    crate::send_logged!(bot, msg, "⛔ Unauthorized.");
                    return Ok(());
                }
                admin::handle_logs(bot, msg).await?;
            }
            Command::Broadcast(msg_text) => {
                if !is_admin {
                    crate::send_logged!(bot, msg, "⛔ Unauthorized.");
                    return Ok(());
                }
                admin::handle_broadcast(bot, msg, msg_text).await?;
            }
            Command::Settings => {
                if !is_admin {
                    crate::send_logged!(bot, msg, "⛔ Unauthorized.");
                    return Ok(());
                }

                let _ = admin::handle_interactive_settings(
                    bot.clone(),
                    msg.chat.id,
                    None,
                    app_context.clone(),
                )
                .await;
            }
            Command::DbDiag => {
                if !is_admin {
                    crate::send_logged!(bot, msg, "⛔ Unauthorized.");
                    return Ok(());
                }
                admin::handle_db_diag(bot, msg, app_context).await?;
            }
        }

        Ok(())
    })
}

#[allow(clippy::too_many_arguments)]
pub async fn handle_callback(
    bot: Bot,
    q: teloxide::types::CallbackQuery,
    ucs: BotUseCases,
    app_context: Arc<crate::domain::models::AppContext>,
    callback_execution_registry: Arc<
        crate::presentation::telegram::callback_inflight::CallbackExecutionRegistry,
    >,
) -> anyhow::Result<()> {
    let Some(mut data) = q.data.clone() else {
        let _ = bot.answer_callback_query(q.id).await;
        return Ok(());
    };

    let identity = match crate::presentation::telegram::request_identity::from_callback(&q) {
        Ok(identity) => identity,
        Err(reason) => {
            let _ = bot.answer_callback_query(q.id).text(reason).await;
            return Ok(());
        }
    };

    let callback_data_for_log = crate::utils::sanitize_callback_data_for_log(&data);

    crate::utils::log_multiline(
        &format!(
            "BOT CALLBACK IN | Actor: {} | Chat: {} | User: {} | Data:",
            identity.actor_user_id,
            identity.chat_id,
            q.from
                .username
                .clone()
                .unwrap_or_else(|| "Unknown".to_string())
        ),
        &callback_data_for_log,
        false,
    );

    let callback_is_admin =
        identity.is_private_admin(app_context.admin_user_id, app_context.admin_chat_id);

    if !callback_is_admin && crate::utils::is_callback_rate_limited(identity.actor_user_id) {
        let _ = bot
            .answer_callback_query(q.id)
            .text("Too many requests. Please slow down.")
            .await;
        return Ok(());
    }
    if data == "cmd_ignore" {
        let _ = bot.answer_callback_query(q.id).await;
        return Ok(());
    }

    if callback_disables_keyboard(&data) && q.message.is_none() {
        let _ = bot
            .answer_callback_query(q.id)
            .text("This action message is no longer available.")
            .await;
        return Ok(());
    }

    let _callback_execution_guard = if let Some(message) = q.message.as_ref() {
        let key = crate::presentation::telegram::callback_inflight::CallbackExecutionKey::new(
            message.chat().id.0,
            message.id().0,
        );

        match callback_execution_registry.try_acquire(key) {
            Some(guard) => Some(guard),
            None => {
                crate::infrastructure::observability::increment_callbacks_rejected_inflight();
                let _ = bot
                    .answer_callback_query(q.id)
                    .text("This action is already running.")
                    .await;
                return Ok(());
            }
        }
    } else {
        None
    };

    if callback_disables_keyboard(&data)
        && let Err(error) = disable_callback_keyboard(&bot, &q).await
    {
        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("Unable to lock this action panel. Please try again.")
            .await;
        tracing::warn!(
            "[CALLBACK UI] Effectful callback refused because the keyboard could not be disabled: {}",
            error
        );
        return Err(error);
    }

    crate::presentation::telegram::handlers::admin_confirm::cleanup_expired(&app_context);

    let mut confirmed_sensitive_action = false;

    if data.starts_with("admin_do:") {
        match crate::presentation::telegram::handlers::admin_confirm::validate_admin_do_callback(
            &app_context,
            identity,
            &data,
        ) {
            Ok(action) => {
                data = action.execute_callback().to_string();
                confirmed_sensitive_action = true;
                if action.requires_admin() {
                    crate::infrastructure::metrics::inc_admin_actions_confirmed();

                    let _ = crate::infrastructure::admin_audit::record_admin_action(
                        &app_context.pool,
                        identity.actor_user_id,
                        identity.chat_id,
                        action.as_str(),
                        None,
                        Some("confirmed"),
                        "confirmed",
                    )
                    .await;
                }

                let _ = bot
                    .answer_callback_query(q.id.clone())
                    .text("Confirmed.")
                    .await;
            }
            Err(reason) => {
                let _ = bot
                    .answer_callback_query(q.id.clone())
                    .text(reason.clone())
                    .await;

                restore_contextual_callback_menu(
                    &bot,
                    &q,
                    &data,
                    callback_is_admin,
                    format!(
                        "⏳ <b>Confirmation failed.</b>\n{}",
                        crate::utils::html_escape(&reason)
                    ),
                )
                .await;

                return Ok(());
            }
        }
    }

    if !confirmed_sensitive_action {
        if let Some(action) =
            crate::presentation::telegram::handlers::admin_confirm::sensitive_action_from_callback(
                &data,
            )
        {
            if action.requires_admin() && !callback_is_admin {
                let _ = bot
                    .answer_callback_query(q.id.clone())
                    .text("Unauthorized.")
                    .await;
                restore_safe_callback_menu(
                    &bot,
                    &q,
                    callback_is_admin,
                    "⛔ <b>Unauthorized action.</b>",
                )
                .await;
                return Ok(());
            }

            let _ = bot.answer_callback_query(q.id.clone()).await;

            if let Some(message) = q.message.as_ref()
                && let Err(error) =
                    crate::presentation::telegram::handlers::admin_confirm::edit_callback_confirmation(
                        &bot,
                        message,
                        &app_context,
                        identity,
                        action,
                    )
                    .await
                {
                    restore_safe_callback_menu(
                        &bot,
                        &q,
                        callback_is_admin,
                        "❌ <b>Confirmation could not be opened.</b>\nThe menu has been restored.",
                    )
                    .await;
                    return Err(error);
                }

            return Ok(());
        }

        if data == "do_forget_all" || data == "do_forget_wallets" {
            let _ = bot
                .answer_callback_query(q.id.clone())
                .text("Confirmation expired. Please try again.")
                .await;

            restore_contextual_callback_menu(
                &bot,
                &q,
                &data,
                callback_is_admin,
                "⏳ <b>Confirmation expired.</b>\nPlease start the action again.",
            )
            .await;

            return Ok(());
        }
    }

    if matches!(
        data.as_str(),
        "do_pause" | "do_resume" | "do_restart" | "do_cleanup_events"
    ) {
        if !confirmed_sensitive_action {
            let _ = bot
                .answer_callback_query(q.id.clone())
                .text("Confirmation expired. Please try again.")
                .await;
            restore_safe_callback_menu(
                &bot,
                &q,
                callback_is_admin,
                "⏳ <b>Confirmation expired.</b>\nPlease start the action again.",
            )
            .await;
            return Ok(());
        }

        if !callback_is_admin {
            let _ = bot
                .answer_callback_query(q.id.clone())
                .text("Unauthorized.")
                .await;
            restore_safe_callback_menu(
                &bot,
                &q,
                callback_is_admin,
                "⛔ <b>Unauthorized action.</b>",
            )
            .await;
            return Ok(());
        }

        let action_result =
            if let Some(teloxide::types::MaybeInaccessibleMessage::Regular(message)) =
                q.message.as_ref()
            {
                let mut message = (**message).clone();
                message.from = Some(q.from.clone());

                match data.as_str() {
                    "do_pause" => {
                        admin::handle_pause(bot.clone(), message, app_context.clone()).await
                    }
                    "do_resume" => {
                        admin::handle_resume(bot.clone(), message, app_context.clone()).await
                    }
                    "do_restart" => admin::handle_restart(bot.clone(), message).await,
                    "do_cleanup_events" => {
                        admin::handle_cleanup_events(bot.clone(), message, app_context.clone())
                            .await
                    }
                    _ => unreachable!("confirmed sensitive action was prevalidated"),
                }
            } else {
                let _ = bot
                    .answer_callback_query(q.id.clone())
                    .text("This confirmation message is no longer accessible.")
                    .await;
                restore_safe_callback_menu(
                    &bot,
                    &q,
                    callback_is_admin,
                    "⚠️ <b>The confirmation message is no longer accessible.</b>",
                )
                .await;
                return Ok(());
            };

        match action_result {
            Ok(()) => {
                restore_safe_callback_menu(
                    &bot,
                    &q,
                    true,
                    "✅ <b>Action completed.</b>\nThe admin menu is available again.",
                )
                .await;
            }
            Err(error) => {
                restore_safe_callback_menu(
                    &bot,
                    &q,
                    true,
                    "❌ <b>Action failed.</b>\nThe admin menu has been restored.",
                )
                .await;
                return Err(error);
            }
        }

        return Ok(());
    }

    if data == "cancel_action" {
        crate::presentation::telegram::handlers::admin_confirm::cancel_for_identity(
            &app_context,
            identity,
        );

        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("Cancelled.")
            .await;
        if let Some(msg) = q.message {
            let markup = if callback_is_admin {
                crate::presentation::telegram::menus::TelegramMenus::admin_menu_markup()
            } else {
                crate::presentation::telegram::menus::TelegramMenus::main_menu_markup()
            };

            let _ = bot
                .edit_message_text(msg.chat().id, msg.id(), "✅ Action cancelled.")
                .parse_mode(ParseMode::Html)
                .reply_markup(markup)
                .await;
        }
        return Ok(());
    }

    if data == "confirm_forget_wallets" {
        let _ = bot.answer_callback_query(q.id.clone()).await;
        if let Some(msg) = q.message {
            let text = "⚠️ <b>Confirm Clear Wallets</b>\nThis will remove all tracked wallets from your account.\n\nAre you sure?";
            let _ = bot
                .edit_message_text(msg.chat().id, msg.id(), text)
                .parse_mode(ParseMode::Html)
                .reply_markup(
                    crate::presentation::telegram::menus::TelegramMenus::confirm_wallet_clear_markup(),
                )
                .await;
        }
        return Ok(());
    }

    if data == "confirm_forget_all" {
        let _ = bot.answer_callback_query(q.id.clone()).await;
        if let Some(msg) = q.message {
            let text = "🚨 <b>Confirm Delete My Data</b>\nThis will remove all tracked wallets and user data linked to this chat.\n\nAre you sure?";
            let _ = bot
                .edit_message_text(msg.chat().id, msg.id(), text)
                .parse_mode(ParseMode::Html)
                .reply_markup(
                    crate::presentation::telegram::menus::TelegramMenus::confirm_full_delete_markup(
                    ),
                )
                .await;
        }
        return Ok(());
    }

    if data == "do_mute_alerts" || data == "do_unmute_alerts" {
        if !confirmed_sensitive_action {
            let _ = bot
                .answer_callback_query(q.id.clone())
                .text("Confirmation expired. Please try again.")
                .await;
            restore_safe_callback_menu(
                &bot,
                &q,
                callback_is_admin,
                "⏳ <b>Confirmation expired.</b>\nPlease start the action again.",
            )
            .await;
            return Ok(());
        }

        if !callback_is_admin {
            let _ = bot
                .answer_callback_query(q.id.clone())
                .text("Unauthorized.")
                .await;
            restore_safe_callback_menu(
                &bot,
                &q,
                callback_is_admin,
                "⛔ <b>Unauthorized action.</b>",
            )
            .await;
            return Ok(());
        }

        let enabled = data == "do_unmute_alerts";
        if let Err(error) = crate::wallet::alert_delivery_gate::set_alert_delivery_enabled(
            &app_context.pool,
            enabled,
        )
        .await
        {
            restore_safe_callback_menu(
                &bot,
                &q,
                true,
                "❌ <b>Alert delivery setting was not changed.</b>\nThe admin menu has been restored.",
            )
            .await;
            return Err(error.into());
        }

        let status_text =
            crate::wallet::alert_delivery_gate::alert_delivery_status_text(&app_context.pool).await;

        let _ = bot
            .answer_callback_query(q.id.clone())
            .text(if enabled {
                "Alerts resumed."
            } else {
                "Alerts muted."
            })
            .await;

        edit_callback_state(
            &bot,
            &q,
            status_text,
            crate::presentation::telegram::menus::TelegramMenus::admin_menu_markup(),
        )
        .await;

        return Ok(());
    }

    if data == "do_forget_wallets" {
        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("Clearing wallets...")
            .await;

        if let Some(message) = q.message.as_ref() {
            let db = PostgresRepository::new(app_context.pool.clone());
            let chat_id = message.chat().id.0;

            if let Err(error) = db.remove_all_user_wallets(chat_id).await {
                tracing::error!("[DATABASE ERROR] Failed to clear wallets: {}", error);
                restore_wallet_callback_menu(
                    &bot,
                    &q,
                    "❌ <b>Wallets were not deleted.</b>\nPlease try again.",
                )
                .await;
                return Err(error.into());
            }

            restore_wallet_callback_menu(&bot, &q, "🗑️ <b>All tracked wallets deleted.</b>").await;
        }

        return Ok(());
    }

    if data == "do_forget_all" {
        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("Deleting data...")
            .await;

        if let Some(message) = q.message.as_ref() {
            let db = PostgresRepository::new(app_context.pool.clone());
            let chat_id = message.chat().id.0;

            if let Err(error) = db.remove_all_user_data(chat_id).await {
                tracing::error!("[DATABASE ERROR] Failed to delete user data: {}", error);
                restore_contextual_callback_menu(
                    &bot,
                    &q,
                    &data,
                    callback_is_admin,
                    "❌ <b>Your data was not deleted.</b>\nPlease try again.",
                )
                .await;
                return Err(error.into());
            }

            restore_safe_callback_menu(
                &bot,
                &q,
                false,
                "🗑️ <b>All your tracking data has been deleted.</b>",
            )
            .await;
        }

        return Ok(());
    }

    if data == "cmd_add_wallet" {
        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("Send your wallet address.")
            .await;

        if let Some(msg) = q.message {
            app_context
                .pending_input_sessions
                .insert(identity.actor_chat_key(), PendingInputAction::AddWallet);

            let text = "➕ <b>Add Wallet</b>\nPlease send your Kaspa wallet address now.\n\nExample:\n<code>kaspa:qq...</code>";

            let _ = bot
                .edit_message_text(msg.chat().id, msg.id(), text)
                .parse_mode(ParseMode::Html)
                .reply_markup(InlineKeyboardMarkup::new(vec![vec![
                    InlineKeyboardButton::callback("❌ Cancel", "cancel_action"),
                ]]))
                .await;
        }

        return Ok(());
    }

    if data == "cmd_wallets" {
        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("Wallets")
            .await;

        if let Some(msg) = q.message {
            render_wallet_panel(&bot, msg.chat().id, msg.id(), &ucs, msg.chat().id.0).await?;
        }

        return Ok(());
    }

    if data == "cmd_remove_wallets" {
        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("Choose a wallet.")
            .await;

        if let Some(msg) = q.message {
            render_remove_wallet_panel(&bot, msg.chat().id, msg.id(), &ucs, msg.chat().id.0)
                .await?;
        }

        return Ok(());
    }

    if let Some(index_text) = data.strip_prefix("rm_wallet_") {
        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("Removing wallet...")
            .await;

        if let Some(message) = q.message.as_ref() {
            let chat_id = message.chat().id.0;
            let index: usize = index_text.parse().unwrap_or(usize::MAX);
            let wallets = match ucs.wallet_query.get_list(chat_id).await {
                Ok(wallets) => wallets,
                Err(error) => {
                    restore_wallet_callback_menu(
                        &bot,
                        &q,
                        "❌ <b>Wallet list could not be loaded.</b>\nPlease try again.",
                    )
                    .await;
                    return Err(error.into());
                }
            };

            if let Some(wallet_address) = wallets.get(index) {
                if let Err(error) = ucs.wallet_mgt.remove_wallet(wallet_address, chat_id).await {
                    tracing::error!("[DATABASE ERROR] Failed to remove wallet: {}", error);
                    restore_wallet_callback_menu(
                        &bot,
                        &q,
                        "❌ <b>Wallet was not removed.</b>\nPlease try again.",
                    )
                    .await;
                    return Err(error.into());
                }

                if let Err(error) =
                    render_wallet_panel(&bot, message.chat().id, message.id(), &ucs, chat_id).await
                {
                    restore_wallet_callback_menu(
                        &bot,
                        &q,
                        "✅ <b>Wallet removed.</b>\nThe wallet menu has been restored.",
                    )
                    .await;
                    return Err(error);
                }
            } else {
                restore_wallet_callback_menu(&bot, &q, "⚠️ Wallet not found.").await;
            }
        }

        return Ok(());
    }

    if let Some(index_text) = data.strip_prefix("wallet_panel_") {
        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("Wallet panel")
            .await;

        if let Some(msg) = q.message {
            let index: usize = index_text.parse().unwrap_or(usize::MAX);
            wallet::handle_wallet_panel(
                bot,
                msg.chat().id,
                msg.id(),
                msg.chat().id.0,
                index,
                ucs.wallet_query.clone(),
            )
            .await?;
        }

        return Ok(());
    }

    if let Some(index_text) = data.strip_prefix("wallet_balance_") {
        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("Balance")
            .await;

        if let Some(msg) = q.message {
            let index: usize = index_text.parse().unwrap_or(usize::MAX);
            wallet::handle_wallet_balance_detail(
                bot,
                msg.chat().id,
                msg.id(),
                msg.chat().id.0,
                index,
                ucs.wallet_query.clone(),
                app_context.clone(),
            )
            .await?;
        }

        return Ok(());
    }

    if let Some(index_text) = data.strip_prefix("wallet_blocks_") {
        let _ = bot.answer_callback_query(q.id.clone()).text("Blocks").await;

        if let Some(msg) = q.message {
            let mut parts = index_text.split('_');
            let index = parts
                .next()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(usize::MAX);
            let history_page = parts
                .next()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0);

            mining::handle_wallet_blocks_detail(
                bot,
                msg.chat().id,
                msg.id(),
                msg.chat().id.0,
                index,
                history_page,
                ucs.wallet_query.clone(),
            )
            .await?;
        }

        return Ok(());
    }
    if let Some(index_text) = data.strip_prefix("wallet_miner_") {
        let _ = bot.answer_callback_query(q.id.clone()).text("Miner").await;

        if let Some(msg) = q.message {
            let index: usize = index_text.parse().unwrap_or(usize::MAX);
            mining::handle_wallet_miner_detail(
                bot,
                msg.chat().id,
                msg.id(),
                msg.chat().id.0,
                index,
                app_context.clone(),
                ucs.miner_stats.clone(),
            )
            .await?;
        }

        return Ok(());
    }

    if let Some(index_text) = data.strip_prefix("wallet_remove_confirm_") {
        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("Confirm remove")
            .await;

        if let Some(msg) = q.message {
            let index: usize = index_text.parse().unwrap_or(usize::MAX);
            wallet::handle_wallet_remove_confirm(
                bot,
                msg.chat().id,
                msg.id(),
                msg.chat().id.0,
                index,
                ucs.wallet_query.clone(),
            )
            .await?;
        }

        return Ok(());
    }

    if let Some(index_text) = data.strip_prefix("wallet_remove_do_") {
        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("Removing wallet")
            .await;

        if let Some(message) = q.message.as_ref() {
            let index: usize = index_text.parse().unwrap_or(usize::MAX);
            if let Err(error) = wallet::handle_wallet_remove_do(
                bot.clone(),
                message.chat().id,
                message.id(),
                message.chat().id.0,
                index,
                ucs.wallet_query.clone(),
                ucs.wallet_mgt.clone(),
            )
            .await
            {
                restore_wallet_callback_menu(
                    &bot,
                    &q,
                    "❌ <b>Wallet was not removed.</b>\nPlease try again.",
                )
                .await;
                return Err(error);
            }
        }

        return Ok(());
    }

    if data.starts_with("btn_toggle_") {
        let flag = data.replace("btn_toggle_", "");
        let db = PostgresRepository::new(app_context.pool.clone());

        let update_result = match flag.as_str() {
            "ENABLE_MEMORY_CLEANER" => {
                let current = app_context.memory_cleaner_enabled.load(Ordering::Relaxed);
                let new_state = !current;
                match db
                    .update_setting(&flag, if new_state { "true" } else { "false" })
                    .await
                {
                    Ok(()) => {
                        app_context
                            .memory_cleaner_enabled
                            .store(new_state, Ordering::Relaxed);
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            }
            "ENABLE_LIVE_SYNC" => {
                let current = app_context.live_sync_enabled.load(Ordering::Relaxed);
                let new_state = !current;
                match db
                    .update_setting(&flag, if new_state { "true" } else { "false" })
                    .await
                {
                    Ok(()) => {
                        app_context
                            .live_sync_enabled
                            .store(new_state, Ordering::Relaxed);
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            }
            "MAINTENANCE_MODE" => {
                let current = app_context.maintenance_mode.load(Ordering::Relaxed);
                let new_state = !current;
                match db
                    .update_setting(&flag, if new_state { "true" } else { "false" })
                    .await
                {
                    Ok(()) => {
                        app_context
                            .maintenance_mode
                            .store(new_state, Ordering::Relaxed);
                        Ok(())
                    }
                    Err(error) => Err(error),
                }
            }
            _ => {
                restore_safe_callback_menu(
                    &bot,
                    &q,
                    callback_is_admin,
                    "⚠️ <b>Unknown setting.</b>",
                )
                .await;
                return Ok(());
            }
        };

        if let Err(error) = update_result {
            restore_safe_callback_menu(
                &bot,
                &q,
                true,
                "❌ <b>Setting was not changed.</b>\nThe admin menu has been restored.",
            )
            .await;
            return Err(error.into());
        }

        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("Setting updated.")
            .await;

        if let Some(message) = q.message.as_ref()
            && let Err(error) = admin::handle_interactive_settings(
                bot.clone(),
                message.chat().id,
                Some(message.id()),
                app_context.clone(),
            )
            .await
        {
            restore_safe_callback_menu(
                &bot,
                &q,
                true,
                "✅ <b>Setting updated.</b>\nThe admin menu has been restored.",
            )
            .await;
            return Err(error);
        }

        return Ok(());
    }

    let mapped_command = match data.as_str() {
        "cmd_start" => Some(Command::Start),
        "cmd_help" => Some(Command::Help),

        "cmd_balance" | "refresh_balance" => Some(Command::Balance),
        "cmd_list" => Some(Command::List),
        "cmd_blocks" | "refresh_blocks" => Some(Command::Blocks),
        "cmd_miner" | "refresh_miner" => Some(Command::Miner),

        "cmd_network" | "refresh_network" => Some(Command::Network),
        "cmd_dag" | "refresh_dag" => Some(Command::Dag),
        "cmd_price" | "refresh_price" => Some(Command::Price),
        "cmd_market" | "refresh_market" => Some(Command::Market),
        "cmd_supply" | "refresh_supply" => Some(Command::Supply),
        "cmd_fees" | "refresh_fees" => Some(Command::Fees),

        "cmd_donate" => Some(Command::Donate),
        "cmd_health" => Some(Command::Health),
        "cmd_stats" | "refresh_stats" => Some(Command::Stats),
        "cmd_sys" => Some(Command::Sys),
        "cmd_logs" => Some(Command::Logs),
        "cmd_events" => Some(Command::Events),
        "cmd_errors" => Some(Command::Errors),
        "cmd_cleanup_events" => Some(Command::CleanupEvents),
        "cmd_delivery" => Some(Command::Delivery),
        "cmd_mute_alerts" => Some(Command::MuteAlerts),
        "cmd_unmute_alerts" => Some(Command::UnmuteAlerts),
        "cmd_alerts_status" => Some(Command::AlertsStatus),
        "cmd_pause" => Some(Command::Pause),
        "cmd_resume" => Some(Command::Resume),
        "cmd_restart" => Some(Command::Restart),
        "cmd_settings" => Some(Command::Settings),
        "cmd_db_diag" => Some(Command::DbDiag),

        _ => None,
    };

    if let Some(command) = mapped_command {
        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("Processing...")
            .await;

        if let Some(teloxide::types::MaybeInaccessibleMessage::Regular(message)) = q.message {
            let mut message = *message;
            message.from = Some(q.from.clone());
            handle_command(bot, message, command, ucs, app_context).await?;
        } else {
            let _ = bot
                .answer_callback_query(q.id)
                .text("This message is no longer accessible.")
                .await;
        }

        return Ok(());
    }

    let _ = bot
        .answer_callback_query(q.id)
        .text("This button is no longer available.")
        .await;

    Ok(())
}

async fn render_wallet_panel(
    bot: &Bot,
    chat_id: teloxide::types::ChatId,
    message_id: teloxide::types::MessageId,
    ucs: &BotUseCases,
    cid: i64,
) -> anyhow::Result<()> {
    let wallets = match ucs.wallet_query.get_list(cid).await {
        Ok(wallets) => wallets,
        Err(error) => {
            wallet::log_wallet_data_error("render_wallet_panel", &error);
            let _ = bot
                .edit_message_text(
                    chat_id,
                    message_id,
                    wallet::wallet_data_unavailable_message(),
                )
                .parse_mode(ParseMode::Html)
                .reply_markup(
                    crate::presentation::telegram::menus::TelegramMenus::wallet_menu_markup(),
                )
                .await;
            return Err(error.into());
        }
    };

    let text = if wallets.is_empty() {
        "👛 <b>Wallets</b>\n━━━━━━━━━━━━━━━━━━\nNo tracked wallets yet.\n\nPress Add Wallet and send your <code>kaspa:...</code> address.".to_string()
    } else {
        let list = wallets
            .iter()
            .enumerate()
            .map(|(i, wallet)| format!("{}. <code>{}</code>", i + 1, wallet))
            .collect::<Vec<_>>()
            .join("\n");

        format!(
            "👛 <b>Wallets</b>\n━━━━━━━━━━━━━━━━━━\n{}\n\nChoose an action below.",
            list
        )
    };

    let _ = bot
        .edit_message_text(chat_id, message_id, text)
        .parse_mode(ParseMode::Html)
        .reply_markup(crate::presentation::telegram::menus::TelegramMenus::wallet_menu_markup())
        .await?;

    Ok(())
}

async fn render_remove_wallet_panel(
    bot: &Bot,
    chat_id: teloxide::types::ChatId,
    message_id: teloxide::types::MessageId,
    ucs: &BotUseCases,
    cid: i64,
) -> anyhow::Result<()> {
    let wallets = match ucs.wallet_query.get_list(cid).await {
        Ok(wallets) => wallets,
        Err(error) => {
            wallet::log_wallet_data_error("render_remove_wallet_panel", &error);
            let _ = bot
                .edit_message_text(
                    chat_id,
                    message_id,
                    wallet::wallet_data_unavailable_message(),
                )
                .parse_mode(ParseMode::Html)
                .reply_markup(
                    crate::presentation::telegram::menus::TelegramMenus::wallet_menu_markup(),
                )
                .await;
            return Err(error.into());
        }
    };

    if wallets.is_empty() {
        let _ = bot
            .edit_message_text(
                chat_id,
                message_id,
                "📭 <b>No tracked wallets.</b>\nThere is nothing to remove.",
            )
            .parse_mode(ParseMode::Html)
            .reply_markup(crate::presentation::telegram::menus::TelegramMenus::wallet_menu_markup())
            .await?;

        return Ok(());
    }

    let mut rows = Vec::new();

    for (index, wallet) in wallets.iter().enumerate() {
        rows.push(vec![InlineKeyboardButton::callback(
            format!("➖ {}", crate::utils::format_short_wallet(wallet)),
            format!("rm_wallet_{}", index),
        )]);
    }

    rows.push(vec![InlineKeyboardButton::callback(
        "🔙 Back to Wallets",
        "cmd_wallets",
    )]);

    let text = "➖ <b>Remove Wallet</b>\nSelect the wallet you want to remove.";

    let _ = bot
        .edit_message_text(chat_id, message_id, text)
        .parse_mode(ParseMode::Html)
        .reply_markup(InlineKeyboardMarkup::new(rows))
        .await?;

    Ok(())
}

pub async fn handle_raw_message(
    bot: Bot,
    msg: Message,
    app_context: Arc<crate::domain::models::AppContext>,
) -> anyhow::Result<()> {
    raw_message::handle_raw_message(bot, msg, app_context).await
}

pub async fn handle_block_user(
    _bot: Bot,
    _msg: teloxide::types::ChatMemberUpdated,
) -> anyhow::Result<()> {
    Ok(())
}

#[cfg(test)]
mod callback_execution_tests {
    use super::{CallbackRecoveryMenu, callback_disables_keyboard, callback_recovery_menu};

    #[test]
    fn state_changing_callbacks_disable_the_keyboard() {
        assert!(callback_disables_keyboard("admin_do:restart:redacted"));
        assert!(callback_disables_keyboard("do_forget_wallets"));
        assert!(callback_disables_keyboard("wallet_remove_do_2"));
        assert!(callback_disables_keyboard("btn_toggle_ENABLE_LIVE_SYNC"));
    }

    #[test]
    fn navigation_callbacks_keep_their_keyboard_until_rendered() {
        assert!(!callback_disables_keyboard("cmd_wallets"));
        assert!(!callback_disables_keyboard("wallet_panel_0"));
    }

    #[test]
    fn confirmation_recovery_restores_the_action_context() {
        assert_eq!(
            callback_recovery_menu("admin_do:clear_wallets:invalid-or-expired-token", false,),
            CallbackRecoveryMenu::Wallet
        );
        assert_eq!(
            callback_recovery_menu("do_forget_wallets", false),
            CallbackRecoveryMenu::Wallet
        );
        assert_eq!(
            callback_recovery_menu("admin_do:forget_all:invalid-or-expired-token", false),
            CallbackRecoveryMenu::Main
        );
        assert_eq!(
            callback_recovery_menu("do_forget_all", false),
            CallbackRecoveryMenu::Main
        );
        assert_eq!(
            callback_recovery_menu("admin_do:pause:invalid-or-expired-token", true),
            CallbackRecoveryMenu::Admin
        );
        assert_eq!(
            callback_recovery_menu("admin_do:pause:invalid-or-expired-token", false),
            CallbackRecoveryMenu::Main
        );
    }
}
