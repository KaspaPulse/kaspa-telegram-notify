pub mod admin;
pub mod admin_confirm;
pub mod lifecycle;
pub mod mining;
pub mod network;
pub mod raw_message;
pub mod wallet;

use crate::domain::models::{PendingInputAction, PendingInputSession, SensitiveAction};
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

/// Ordinary-user actions that are safe while maintenance mode is active.
/// Admin authorization is evaluated separately and keeps its existing behavior.
fn maintenance_allows_command(command: &Command) -> bool {
    matches!(
        command,
        Command::Start
            | Command::Help
            | Command::Remove(_)
            | Command::List
            | Command::Wallets
            | Command::Blocks
            | Command::Donate
            | Command::ForgetWallets
            | Command::ForgetAll
            | Command::Forget
            | Command::HideMenu
    )
}

fn maintenance_allows_callback(data: &str) -> bool {
    if matches!(
        data,
        "cmd_ignore"
            | "cancel_action"
            | "confirm_forget_wallets"
            | "confirm_forget_all"
            | "do_forget_wallets"
            | "do_forget_all"
            | "cmd_wallets"
            | "cmd_remove_wallets"
            | "menu_mining"
            | "menu_more"
            | "cmd_start"
            | "cmd_help"
            | "cmd_list"
            | "cmd_blocks"
            | "refresh_blocks"
            | "cmd_donate"
    ) {
        return true;
    }

    if [
        "rm_wallet_",
        "wallet_panel_",
        "wallet_balance_",
        "wallet_blocks_",
        "wallet_miner_",
        "wallet_remove_confirm_",
        "wallet_remove_do_",
        "wp:",
        "wblk:",
        "wrc:",
        "wrd:",
    ]
    .iter()
    .any(|prefix| data.starts_with(prefix))
    {
        return true;
    }

    data.strip_prefix("admin_do:")
        .and_then(|remainder| remainder.split(':').next())
        .and_then(SensitiveAction::parse)
        .is_some_and(|action| {
            matches!(
                action,
                SensitiveAction::ClearWallets | SensitiveAction::ForgetAll
            )
        })
}

fn callback_disables_keyboard(data: &str) -> bool {
    data.starts_with("admin_do:")
        || data.starts_with("wrd:")
        || data.starts_with("btn_toggle_")
        || matches!(
            data,
            "do_pause"
                | "do_resume"
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

        if app_context.maintenance_mode.load(Ordering::Relaxed)
            && !is_admin
            && !maintenance_allows_command(&cmd)
        {
            let _ = bot
                .send_message(chat_id, raw_message::MAINTENANCE_MESSAGE)
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
                let help_text = String::from(
                    "📚 <b>Kaspa Pulse Help</b>\n\
                     ━━━━━━━━━━━━━━━━━━\n\
                     <b>Community Mining Alerts</b>\n\n\
                     Kaspa Pulse tracks Kaspa wallets, estimates solo-mining activity, and sends confirmed mining-reward alerts.\n\n\
                     🚀 <b>Quick Start</b>\n\
                     • Use /start to open the main menu.\n\
                     • Open <b>Wallets</b> to manage tracked wallets.\n\
                     • Open <b>Mining</b> for hashrate and mined-block views.\n\
                     • Open <b>Network</b> for node health, BlockDAG, fees, and supply.\n\
                     • Open <b>Market</b> for KAS market data.\n\
                     • Open <b>More</b> for help, donation, and privacy actions.\n\n\
                     ✅ <b>Reward Confirmation Policy</b>\n\
                     • Rewards are detected from wallet UTXOs, then held until they reach the configured confirmation threshold.\n\
                     • Default threshold: <b>10 DAA confirmations</b>.\n\
                     • Confirmed rewards continue to DAG analysis before alert delivery.\n\n\
                     👛 <b>Wallet Buttons</b>\n\
                     • <b>My Wallets</b> — Show tracked wallets.\n\
                     • <b>Add Wallet</b> — Add a wallet.\n\
                     • <b>Remove Wallet</b> — Remove one wallet.\n\
                     • <b>Clear Wallets</b> — Remove all tracked wallets after confirmation.\n\
                     • <b>Back</b> — Return to the previous menu.\n\n\
                     ⌨️ <b>Visible Commands</b>\n\
                     • /start — Open the main menu.\n\
                     • /help — Show this guide.\n\
                     • /balance — Check live balance and UTXOs.\n\
                     • /wallets — Open wallet management.\n\
                     • /network — Show Kaspa node and network health.",
                );

                let mut help_text_2 = String::from(
                    "🧭 <b>Mining, Network &amp; Safety</b>\n\
                     ━━━━━━━━━━━━━━━━━━\n\
                     • Confirmed reward analysis can include accepting block, real mined block, worker, nonce, and DAA details.\n\
                     • DAG analysis does not stop when a candidate block is unavailable; unavailable candidates are skipped safely while the search continues.\n\
                     • <b>Network Health</b> shows node status, explicit Kaspa node version, network, sync, peers, hashrate, and BPS.\n\n\
                     🔐 <b>Privacy &amp; Safety</b>\n\
                     • Clear Wallets and Delete My Data require confirmation.\n\
                     • Admin-only actions remain protected by the configured private admin identity.\n\
                     • Hidden legacy commands remain supported for compatibility; hiding a command is never used as authorization.",
                );
                if is_admin {
                    help_text_2.push_str(
                        "\n\n🛡️ <b>Owner Buttons</b>\n\
                         • <b>Overview</b> — Bot, Kaspa node/version, database, monitoring, delivery, uptime, users, wallets, and last alert.\n\
                         • <b>Operations</b> — State-aware monitoring, maintenance, service information, and maintenance tools.\n\
                         • <b>Alerts</b> — Alert status and the single valid enable/disable action.\n\
                         • <b>Diagnostics</b> — Errors, Events, Delivery, Database, and Logs.\n\
                         • <b>Settings</b> — Monitoring and maintenance settings.\n\n\
                         🛠️ <b>Owner Commands</b>\n\
                         • /admin — Open the Administration panel.\n\
                         • Legacy admin commands remain supported for compatibility but are intentionally hidden from the Telegram command menu.\n\
                         • Advanced legacy commands include /events, /errors, /delivery, /logs, /db_diag, /health, /stats, /sys, /pause, /resume, /mute_alerts, /unmute_alerts, /alerts_status, and /cleanup_events."
                    );
                }
                crate::send_logged!(bot, msg, help_text);
                crate::send_logged!(bot, msg, help_text_2);
            }
            Command::Start => {
                let markup =
                    crate::presentation::telegram::menus::TelegramMenus::home_menu_markup(is_admin);

                let welcome = "🤖 <b>Kaspa Pulse</b>\nCommunity Mining Alerts\n━━━━━━━━━━━━━━━━━━\nTrack Kaspa wallets, monitor solo-mining rewards, and receive live alerts.\n\n⚡ <b>Quick Start:</b>\nPaste any <code>kaspa:...</code> address in this chat to activate tracking.\n\n👇 <i>Select a category below or type /help.</i>";

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
                wallet::handle_add(bot, msg, cid, actor_user_id, wallet, ucs.wallet_mgt).await?;
            }
            Command::Remove(wallet) => {
                wallet::handle_remove(bot, msg, cid, wallet, ucs.wallet_mgt).await?
            }
            Command::List => wallet::handle_list(bot, msg, cid, ucs.wallet_query).await?,
            Command::Wallets => {
                send_wallet_panel(&bot, &msg, &ucs, cid).await?;
            }
            Command::Balance => {
                wallet::handle_balance(bot, msg, cid, ucs.wallet_query, app_context).await?
            }

            Command::Blocks => {
                mining::handle_blocks(bot, msg, cid, ucs.wallet_query, app_context).await?
            }
            Command::Miner => {
                mining::handle_miner(bot, msg, cid, ucs.wallet_query, ucs.miner_stats).await?
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

            Command::Admin => {
                crate::utils::send_logged_message(
                    &bot,
                    msg.chat.id,
                    Some(msg.id),
                    "🛡️ <b>Administration</b>\n━━━━━━━━━━━━━━━━━━\nChoose an administration area."
                        .to_string(),
                    Some(crate::presentation::telegram::menus::TelegramMenus::admin_menu_markup()),
                )
                .await?;
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
            Command::RestartInfo => {
                if !is_admin {
                    crate::send_logged!(bot, msg, "⛔ Unauthorized.");
                    return Ok(());
                }
                admin::handle_restart_info(bot, msg).await?;
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

    if app_context.maintenance_mode.load(Ordering::Relaxed)
        && !callback_is_admin
        && !maintenance_allows_callback(&data)
    {
        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("Maintenance Mode: this action is temporarily unavailable.")
            .await;
        restore_safe_callback_menu(&bot, &q, false, raw_message::MAINTENANCE_MESSAGE).await;
        return Ok(());
    }

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

    if data == "cmd_start" {
        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("Main menu")
            .await;
        edit_callback_state(
            &bot,
            &q,
            "🤖 <b>Kaspa Pulse</b>\nCommunity Mining Alerts\n━━━━━━━━━━━━━━━━━━\nTrack Kaspa wallets, monitor solo-mining rewards, and receive live alerts.\n\n👇 <i>Select a category below or type /help.</i>",
            crate::presentation::telegram::menus::TelegramMenus::home_menu_markup(
                callback_is_admin,
            ),
        )
        .await;
        return Ok(());
    }

    if data == "menu_mining" {
        let _ = bot.answer_callback_query(q.id.clone()).text("Mining").await;
        edit_callback_state(
            &bot,
            &q,
            "⛏️ <b>Mining</b>\n━━━━━━━━━━━━━━━━━━\nChoose a mining view.",
            crate::presentation::telegram::menus::TelegramMenus::mining_menu_markup(),
        )
        .await;
        return Ok(());
    }

    if data == "menu_network" {
        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("Network")
            .await;
        edit_callback_state(
            &bot,
            &q,
            "🌐 <b>Network</b>\n━━━━━━━━━━━━━━━━━━\nChoose a network view.",
            crate::presentation::telegram::menus::TelegramMenus::network_menu_markup(),
        )
        .await;
        return Ok(());
    }

    if data == "menu_more" {
        let _ = bot.answer_callback_query(q.id.clone()).text("More").await;
        edit_callback_state(
            &bot,
            &q,
            "☰ <b>More</b>\n━━━━━━━━━━━━━━━━━━\nHelp, support, and privacy actions.",
            crate::presentation::telegram::menus::TelegramMenus::more_menu_markup(),
        )
        .await;
        return Ok(());
    }

    if data == "cmd_admin"
        || matches!(
            data.as_str(),
            "admin_overview"
                | "admin_operations"
                | "admin_alerts"
                | "admin_diagnostics"
                | "admin_settings"
                | "admin_maintenance_tools"
        )
    {
        if !callback_is_admin {
            let _ = bot
                .answer_callback_query(q.id.clone())
                .text("Unauthorized.")
                .await;
            edit_callback_state(
                &bot,
                &q,
                "⛔ <b>Unauthorized action.</b>",
                crate::presentation::telegram::menus::TelegramMenus::main_menu_markup(),
            )
            .await;
            return Ok(());
        }

        let _ = bot.answer_callback_query(q.id.clone()).await;
        match data.as_str() {
            "cmd_admin" => {
                edit_callback_state(
                    &bot,
                    &q,
                    "🛡️ <b>Administration</b>\n━━━━━━━━━━━━━━━━━━\nChoose an administration area.",
                    crate::presentation::telegram::menus::TelegramMenus::admin_menu_markup(),
                )
                .await;
            }
            "admin_overview" => {
                let text = admin::overview_panel_text(&app_context).await;
                edit_callback_state(
                    &bot,
                    &q,
                    text,
                    crate::presentation::telegram::menus::TelegramMenus::admin_menu_markup(),
                )
                .await;
            }
            "admin_operations" => {
                let monitoring = app_context.live_sync_enabled.load(Ordering::Relaxed);
                let maintenance = app_context.maintenance_mode.load(Ordering::Relaxed);
                edit_callback_state(
                    &bot,
                    &q,
                    admin::operations_panel_text(&app_context),
                    crate::presentation::telegram::menus::TelegramMenus::admin_operations_markup(
                        monitoring,
                        maintenance,
                    ),
                )
                .await;
            }
            "admin_alerts" => {
                let enabled = crate::wallet::alert_delivery_gate::is_alert_delivery_enabled(
                    &app_context.pool,
                )
                .await
                .ok();
                let text = crate::wallet::alert_delivery_gate::alert_delivery_status_text(
                    &app_context.pool,
                )
                .await;
                edit_callback_state(
                    &bot,
                    &q,
                    text,
                    crate::presentation::telegram::menus::TelegramMenus::admin_alerts_markup(
                        enabled,
                    ),
                )
                .await;
            }
            "admin_diagnostics" => {
                edit_callback_state(
                    &bot,
                    &q,
                    admin::diagnostics_panel_text(),
                    crate::presentation::telegram::menus::TelegramMenus::admin_diagnostics_markup(),
                )
                .await;
            }
            "admin_settings" => {
                if let Some(message) = q.message.as_ref() {
                    admin::handle_interactive_settings(
                        bot.clone(),
                        message.chat().id,
                        Some(message.id()),
                        app_context.clone(),
                    )
                    .await?;
                }
            }
            "admin_maintenance_tools" => {
                edit_callback_state(
                    &bot,
                    &q,
                    admin::maintenance_tools_text(),
                    crate::presentation::telegram::menus::TelegramMenus::admin_maintenance_tools_markup(),
                )
                .await;
            }
            _ => unreachable!("admin navigation is matched above"),
        }
        return Ok(());
    }

    if data == "btn_toggle_ENABLE_MEMORY_CLEANER" {
        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("Housekeeping is always enabled.")
            .await;
        if callback_is_admin {
            let monitoring = app_context.live_sync_enabled.load(Ordering::Relaxed);
            let maintenance = app_context.maintenance_mode.load(Ordering::Relaxed);
            edit_callback_state(
                &bot,
                &q,
                admin::settings_panel_text(&app_context),
                crate::presentation::telegram::menus::TelegramMenus::admin_settings_markup(
                    monitoring,
                    maintenance,
                ),
            )
            .await;
        }
        return Ok(());
    }

    if callback_is_admin && matches!(data.as_str(), "cmd_pause" | "cmd_resume") {
        let monitoring = app_context.live_sync_enabled.load(Ordering::Relaxed);
        let stale = (data == "cmd_pause" && !monitoring) || (data == "cmd_resume" && monitoring);
        if stale {
            let _ = bot
                .answer_callback_query(q.id.clone())
                .text(if monitoring {
                    "Monitoring is already active."
                } else {
                    "Monitoring is already paused."
                })
                .await;
            edit_callback_state(
                &bot,
                &q,
                admin::operations_panel_text(&app_context),
                crate::presentation::telegram::menus::TelegramMenus::admin_operations_markup(
                    monitoring,
                    app_context.maintenance_mode.load(Ordering::Relaxed),
                ),
            )
            .await;
            return Ok(());
        }
    }

    if callback_is_admin
        && matches!(data.as_str(), "cmd_mute_alerts" | "cmd_unmute_alerts")
        && let Ok(enabled) =
            crate::wallet::alert_delivery_gate::is_alert_delivery_enabled(&app_context.pool).await
    {
        let stale =
            (data == "cmd_mute_alerts" && !enabled) || (data == "cmd_unmute_alerts" && enabled);
        if stale {
            let _ = bot
                .answer_callback_query(q.id.clone())
                .text(if enabled {
                    "Alerts are already enabled."
                } else {
                    "Alerts are already disabled."
                })
                .await;
            edit_callback_state(
                &bot,
                &q,
                crate::wallet::alert_delivery_gate::alert_delivery_status_text(&app_context.pool)
                    .await,
                crate::presentation::telegram::menus::TelegramMenus::admin_alerts_markup(Some(
                    enabled,
                )),
            )
            .await;
            return Ok(());
        }
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
        "do_pause" | "do_resume" | "do_cleanup_events"
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
                        if !app_context.live_sync_enabled.load(Ordering::Relaxed) {
                            Ok(())
                        } else {
                            admin::set_monitoring_enabled(&app_context, false).await
                        }
                    }
                    "do_resume" => {
                        if app_context.live_sync_enabled.load(Ordering::Relaxed) {
                            Ok(())
                        } else {
                            admin::set_monitoring_enabled(&app_context, true).await
                        }
                    }
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
                if matches!(data.as_str(), "do_pause" | "do_resume") {
                    let monitoring = app_context.live_sync_enabled.load(Ordering::Relaxed);
                    let maintenance = app_context.maintenance_mode.load(Ordering::Relaxed);
                    edit_callback_state(
                        &bot,
                        &q,
                        admin::operations_panel_text(&app_context),
                        crate::presentation::telegram::menus::TelegramMenus::admin_operations_markup(
                            monitoring,
                            maintenance,
                        ),
                    )
                    .await;
                } else {
                    restore_safe_callback_menu(
                        &bot,
                        &q,
                        true,
                        "✅ <b>Action completed.</b>\nThe admin menu is available again.",
                    )
                    .await;
                }
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
        let current_enabled =
            crate::wallet::alert_delivery_gate::is_alert_delivery_enabled(&app_context.pool)
                .await?;
        if current_enabled != enabled
            && let Err(error) = crate::wallet::alert_delivery_gate::set_alert_delivery_enabled(
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
            crate::presentation::telegram::menus::TelegramMenus::admin_alerts_markup(Some(enabled)),
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

            let deletion = match db
                .remove_all_user_data(chat_id, identity.actor_user_id)
                .await
            {
                Ok(summary) => summary,
                Err(error) => {
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
            };

            crate::presentation::telegram::handlers::admin_confirm::clear_all_runtime_state_for_identity(
                &app_context,
                identity,
            );
            for wallet in &deletion.orphan_wallet_addresses {
                app_context.state.remove(wallet);
                app_context.utxo_state.remove(wallet);
            }

            tracing::info!(
                wallets_deleted = deletion.wallets_deleted,
                event_rows_deleted = deletion.event_rows_deleted,
                chat_history_rows_deleted = deletion.chat_history_rows_deleted,
                queue_rows_deleted = deletion.queue_rows_deleted,
                admin_audit_rows_anonymized = deletion.admin_audit_rows_anonymized,
                orphan_wallets_cleaned = deletion.orphan_wallets_cleaned,
                "[PRIVACY] User data deletion completed and verified."
            );

            restore_safe_callback_menu(
                &bot,
                &q,
                false,
                "🗑️ <b>Your user-linked tracking data has been deleted and verified.</b>",
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
            app_context.pending_input_sessions.insert(
                identity.actor_chat_key(),
                PendingInputSession {
                    action: PendingInputAction::AddWallet,
                    message_id: msg.id().0,
                },
            );

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

    if [
        "rm_wallet_",
        "wallet_panel_",
        "wallet_balance_",
        "wallet_blocks_",
        "wallet_miner_",
        "wallet_remove_confirm_",
        "wallet_remove_do_",
    ]
    .iter()
    .any(|prefix| data.starts_with(prefix))
    {
        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("This wallet button is outdated. Open Wallets again.")
            .await;
        restore_wallet_callback_menu(
            &bot,
            &q,
            "⏳ <b>This wallet button is outdated.</b>\nOpen Wallets again to get a fresh action panel.",
        )
        .await;
        return Ok(());
    }

    if let Some(token) = data.strip_prefix("wp:") {
        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("Wallet panel")
            .await;
        if let Some(msg) = q.message {
            wallet::handle_wallet_panel(
                bot,
                msg.chat().id,
                msg.id(),
                msg.chat().id.0,
                token,
                ucs.wallet_query.clone(),
            )
            .await?;
        }
        return Ok(());
    }

    if let Some(token) = data.strip_prefix("wbal:") {
        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("Balance")
            .await;
        if let Some(msg) = q.message {
            wallet::handle_wallet_balance_detail(
                bot,
                msg.chat().id,
                msg.id(),
                msg.chat().id.0,
                token,
                ucs.wallet_query.clone(),
                app_context.clone(),
            )
            .await?;
        }
        return Ok(());
    }

    if let Some(rest) = data.strip_prefix("wblk:") {
        let _ = bot.answer_callback_query(q.id.clone()).text("Blocks").await;
        if let Some(ref msg) = q.message {
            let mut parts = rest.split(':');
            let token = parts.next().unwrap_or_default();
            let history_page = parts
                .next()
                .and_then(|value| value.parse::<usize>().ok())
                .unwrap_or(0);
            if parts.next().is_some() {
                restore_wallet_callback_menu(&bot, &q, "⚠️ Invalid wallet action.").await;
                return Ok(());
            }
            mining::handle_wallet_blocks_detail(
                bot,
                msg.chat().id,
                msg.id(),
                msg.chat().id.0,
                token,
                history_page,
                ucs.wallet_query.clone(),
            )
            .await?;
        }
        return Ok(());
    }

    if let Some(token) = data.strip_prefix("wmin:") {
        let _ = bot.answer_callback_query(q.id.clone()).text("Miner").await;
        if let Some(msg) = q.message {
            mining::handle_wallet_miner_detail(
                bot,
                msg.chat().id,
                msg.id(),
                msg.chat().id.0,
                token,
                ucs.wallet_query.clone(),
                ucs.miner_stats.clone(),
            )
            .await?;
        }
        return Ok(());
    }

    if let Some(token) = data.strip_prefix("wrc:") {
        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("Confirm remove")
            .await;
        if let Some(msg) = q.message {
            wallet::handle_wallet_remove_confirm(
                bot,
                msg.chat().id,
                msg.id(),
                msg.chat().id.0,
                token,
                ucs.wallet_query.clone(),
            )
            .await?;
        }
        return Ok(());
    }

    if let Some(token) = data.strip_prefix("wrd:") {
        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("Removing wallet")
            .await;
        if let Some(message) = q.message.as_ref()
            && let Err(error) = wallet::handle_wallet_remove_do(
                bot.clone(),
                message.chat().id,
                message.id(),
                message.chat().id.0,
                token,
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
        return Ok(());
    }

    if data.starts_with("btn_toggle_") {
        let flag = data.replace("btn_toggle_", "");
        if flag == "ENABLE_MEMORY_CLEANER" {
            let _ = bot
                .answer_callback_query(q.id.clone())
                .text("Housekeeping is always enabled.")
                .await;
            if callback_is_admin && let Some(message) = q.message.as_ref() {
                admin::handle_interactive_settings(
                    bot.clone(),
                    message.chat().id,
                    Some(message.id()),
                    app_context.clone(),
                )
                .await?;
            }
            return Ok(());
        }

        let db = PostgresRepository::new(app_context.pool.clone());

        let update_result = match flag.as_str() {
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
        "cmd_restart_info" => Some(Command::RestartInfo),
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

fn wallet_panel_text(wallets: &[String]) -> String {
    if wallets.is_empty() {
        "👛 <b>Wallets</b>\n━━━━━━━━━━━━━━━━━━\nNo tracked wallets yet.\n\nPress Add Wallet and send your <code>kaspa:...</code> address.".to_string()
    } else {
        let list = wallets
            .iter()
            .enumerate()
            .map(|(index, wallet)| format!("{}. <code>{}</code>", index + 1, wallet))
            .collect::<Vec<_>>()
            .join("\n");
        format!(
            "👛 <b>Wallets</b>\n━━━━━━━━━━━━━━━━━━\n{}\n\nChoose an action below.",
            list
        )
    }
}

async fn send_wallet_panel(
    bot: &Bot,
    msg: &Message,
    ucs: &BotUseCases,
    cid: i64,
) -> anyhow::Result<()> {
    let wallets = ucs.wallet_query.get_list(cid).await.map_err(|error| {
        wallet::log_wallet_data_error("send_wallet_panel", &error);
        anyhow::Error::from(error)
    })?;
    crate::utils::send_logged_message(
        bot,
        msg.chat.id,
        Some(msg.id),
        wallet_panel_text(&wallets),
        Some(crate::presentation::telegram::menus::TelegramMenus::wallet_menu_markup()),
    )
    .await
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

    for wallet in &wallets {
        rows.push(vec![InlineKeyboardButton::callback(
            format!("➖ {}", crate::utils::format_short_wallet(wallet)),
            format!("wrc:{}", wallet::wallet_callback_token(cid, wallet)),
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
    use super::{
        CallbackRecoveryMenu, callback_disables_keyboard, callback_recovery_menu,
        maintenance_allows_callback, maintenance_allows_command,
    };
    use crate::presentation::telegram::commands::Command;

    #[test]
    fn state_changing_callbacks_disable_the_keyboard() {
        assert!(callback_disables_keyboard("admin_do:pause:redacted"));
        assert!(callback_disables_keyboard("do_forget_wallets"));
        assert!(callback_disables_keyboard(
            "wrd:0123456789abcdef0123456789abcdef"
        ));
        assert!(callback_disables_keyboard("btn_toggle_ENABLE_LIVE_SYNC"));
    }

    #[test]
    fn navigation_callbacks_keep_their_keyboard_until_rendered() {
        assert!(!callback_disables_keyboard("cmd_wallets"));
        assert!(!callback_disables_keyboard(
            "wp:0123456789abcdef0123456789abcdef"
        ));
    }

    #[test]
    fn maintenance_command_contract_allows_help_local_reads_and_deletion() {
        for command in [
            Command::Start,
            Command::Help,
            Command::Remove("kaspa:test".to_string()),
            Command::List,
            Command::Blocks,
            Command::Donate,
            Command::ForgetWallets,
            Command::ForgetAll,
            Command::Forget,
            Command::HideMenu,
        ] {
            assert!(maintenance_allows_command(&command), "{command:?}");
        }

        for command in [
            Command::Add("kaspa:test".to_string()),
            Command::Balance,
            Command::Miner,
            Command::Network,
            Command::Dag,
            Command::Price,
            Command::Market,
            Command::Supply,
            Command::Fees,
        ] {
            assert!(!maintenance_allows_command(&command), "{command:?}");
        }
    }

    #[test]
    fn maintenance_callback_contract_blocks_add_external_and_edit_actions() {
        for data in [
            "cmd_add_wallet",
            "cmd_balance",
            "wbal:0123456789abcdef0123456789abcdef",
            "cmd_miner",
            "wmin:0123456789abcdef0123456789abcdef",
            "cmd_network",
            "cmd_dag",
            "cmd_market",
            "cmd_supply",
            "cmd_fees",
            "btn_toggle_ENABLE_LIVE_SYNC",
        ] {
            assert!(!maintenance_allows_callback(data), "{data}");
        }
    }

    #[test]
    fn maintenance_callback_contract_allows_reads_deletion_privacy_and_cancel() {
        for data in [
            "cmd_ignore",
            "cancel_action",
            "cmd_start",
            "cmd_help",
            "cmd_wallets",
            "cmd_list",
            "cmd_blocks",
            "refresh_blocks",
            "cmd_donate",
            "cmd_remove_wallets",
            "confirm_forget_wallets",
            "confirm_forget_all",
            "do_forget_wallets",
            "do_forget_all",
            "wp:0123456789abcdef0123456789abcdef",
            "wblk:0123456789abcdef0123456789abcdef:0",
            "wrc:0123456789abcdef0123456789abcdef",
            "wrd:0123456789abcdef0123456789abcdef",
            "admin_do:clear_wallets:00000000000000000000000000000000",
            "admin_do:forget_all:00000000000000000000000000000000",
        ] {
            assert!(maintenance_allows_callback(data), "{data}");
        }
    }

    #[tokio::test]
    async fn my_chat_member_handler_accepts_synthetic_update_without_external_io() {
        let update: teloxide::types::Update = serde_json::from_str(
            r#"{
                "update_id": 1,
                "my_chat_member": {
                    "chat": {"id": 1001, "first_name": "Synthetic", "type": "private"},
                    "from": {"id": 2001, "is_bot": false, "first_name": "Tester"},
                    "date": 1644677726,
                    "old_chat_member": {
                        "user": {"id": 3001, "is_bot": true, "first_name": "AuditBot"},
                        "status": "member"
                    },
                    "new_chat_member": {
                        "user": {"id": 3001, "is_bot": true, "first_name": "AuditBot"},
                        "status": "kicked",
                        "until_date": 0
                    }
                }
            }"#,
        )
        .expect("synthetic my_chat_member update must parse");
        let teloxide::types::UpdateKind::MyChatMember(member) = update.kind else {
            panic!("expected my_chat_member update");
        };

        super::handle_block_user(teloxide::Bot::new("1234567890:TEST_TOKEN"), member)
            .await
            .expect("no-op membership handler must accept the event");
    }

    #[tokio::test]
    async fn my_chat_member_filter_dispatches_to_application_handler() {
        use teloxide::dispatching::UpdateFilterExt;

        let update: teloxide::types::Update = serde_json::from_str(
            r#"{
                "update_id": 2,
                "my_chat_member": {
                    "chat": {"id": 1002, "first_name": "Synthetic", "type": "private"},
                    "from": {"id": 2002, "is_bot": false, "first_name": "Tester"},
                    "date": 1644677726,
                    "old_chat_member": {
                        "user": {"id": 3002, "is_bot": true, "first_name": "AuditBot"},
                        "status": "member"
                    },
                    "new_chat_member": {
                        "user": {"id": 3002, "is_bot": true, "first_name": "AuditBot"},
                        "status": "kicked",
                        "until_date": 0
                    }
                }
            }"#,
        )
        .expect("synthetic my_chat_member update must parse");

        let handler = teloxide::dptree::entry().branch(
            teloxide::types::Update::filter_my_chat_member().endpoint(super::handle_block_user),
        );
        let result = handler
            .dispatch(teloxide::dptree::deps![
                update,
                teloxide::Bot::new("1234567890:TEST_TOKEN")
            ])
            .await;

        assert!(
            result.is_break(),
            "my_chat_member must reach the registered handler"
        );
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
