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

/// A private user's explicit choice owns the entire bot language, including
/// the Telegram slash picker. No phone-locale or global Arabic command scope
/// is allowed to override a new user's English default.
fn native_commands_for_user(
    admin: bool,
    lang: crate::presentation::telegram::menus::Language,
) -> Vec<teloxide::types::BotCommand> {
    use crate::presentation::telegram::commands::*;
    use crate::presentation::telegram::menus::Language;
    match (admin, lang) {
        (true, Language::Ar) => public_bot_commands_ar(),
        (true, Language::En) => public_bot_commands(),
        (false, Language::Ar) => public_bot_commands_ar(),
        (false, Language::En) => public_bot_commands(),
    }
}

fn may_sync_private_native_commands(identity: crate::domain::models::RequestIdentity) -> bool {
    // A group callback must never update shared chat commands for everyone.
    identity.is_private && i64::try_from(identity.actor_user_id).ok() == Some(identity.chat_id)
}

async fn sync_private_native_commands(
    bot: &Bot,
    identity: crate::domain::models::RequestIdentity,
    admin: bool,
    lang: crate::presentation::telegram::menus::Language,
) {
    if !may_sync_private_native_commands(identity) {
        return;
    }
    let scope = teloxide::types::BotCommandScope::Chat {
        chat_id: teloxide::types::Recipient::Id(ChatId(identity.chat_id)),
    };
    let operation = bot
        .set_my_commands(native_commands_for_user(admin, lang))
        .scope(scope);
    match tokio::time::timeout(std::time::Duration::from_secs(4), operation).await {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => {
            tracing::warn!(
                error = %crate::utils::sanitize_for_log(&error.to_string()),
                "Private Telegram command-language sync failed; persisted preference unchanged"
            );
        }
        Err(_) => {
            tracing::warn!(
                "Private Telegram command-language sync timed out; persisted preference unchanged"
            );
        }
    }
}

/// Save only Telegram-asserted profile fields for an already-known bot user.
/// No new subscriber rows, Telegram lookups, or guessed usernames.
async fn refresh_known_telegram_identity(pool: &sqlx::PgPool, user: &teloxide::types::User) {
    let result=sqlx::query(
        "UPDATE public.bot_users SET telegram_username=$1,telegram_first_name=$2,telegram_last_name=$3,last_seen_at=now() WHERE telegram_user_id=$4")
        .bind(user.username.as_deref())
        .bind(&user.first_name)
        .bind(user.last_name.as_deref())
        .bind(user.id.0 as i64)
        .execute(pool).await;
    if result.is_err() {
        tracing::warn!("[PROFILE] Existing Telegram user metadata update unavailable.");
    }
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

fn callback_recovery_markup(
    menu: CallbackRecoveryMenu,
    lang: crate::presentation::telegram::menus::Language,
) -> InlineKeyboardMarkup {
    use crate::presentation::telegram::menus::TelegramMenus;
    match menu {
        CallbackRecoveryMenu::Admin => TelegramMenus::home_markup(lang, true),
        CallbackRecoveryMenu::Main => TelegramMenus::home_markup(lang, false),
        CallbackRecoveryMenu::Wallet => TelegramMenus::wallet_menu_markup_lang(lang),
    }
}

fn safe_callback_menu(
    callback_is_admin: bool,
    lang: crate::presentation::telegram::menus::Language,
) -> InlineKeyboardMarkup {
    callback_recovery_markup(
        if callback_is_admin {
            CallbackRecoveryMenu::Admin
        } else {
            CallbackRecoveryMenu::Main
        },
        lang,
    )
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
    ctx: &Arc<crate::domain::models::AppContext>,
    bot: &Bot,
    q: &teloxide::types::CallbackQuery,
    callback_is_admin: bool,
    text: impl Into<String>,
) {
    let lang =
        crate::presentation::telegram::menus::user_language(&ctx.pool, q.from.id.0 as i64).await;
    edit_callback_state(bot, q, text, safe_callback_menu(callback_is_admin, lang)).await;
}

async fn restore_wallet_callback_menu(
    ctx: &Arc<crate::domain::models::AppContext>,
    bot: &Bot,
    q: &teloxide::types::CallbackQuery,
    text: impl Into<String>,
) {
    let lang =
        crate::presentation::telegram::menus::user_language(&ctx.pool, q.from.id.0 as i64).await;
    edit_callback_state(
        bot,
        q,
        text,
        callback_recovery_markup(CallbackRecoveryMenu::Wallet, lang),
    )
    .await;
}

async fn restore_contextual_callback_menu(
    ctx: &Arc<crate::domain::models::AppContext>,
    bot: &Bot,
    q: &teloxide::types::CallbackQuery,
    data: &str,
    callback_is_admin: bool,
    text: impl Into<String>,
) {
    let lang =
        crate::presentation::telegram::menus::user_language(&ctx.pool, q.from.id.0 as i64).await;
    edit_callback_state(
        bot,
        q,
        text,
        callback_recovery_markup(callback_recovery_menu(data, callback_is_admin), lang),
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
        if let Some(user) = msg.from.as_ref() {
            refresh_known_telegram_identity(&app_context.pool, user).await;
        }
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
                let lang = crate::presentation::telegram::menus::user_language(
                    &app_context.pool,
                    actor_user_id as i64,
                )
                .await;
                let _ = bot
                    .send_message(
                        msg.chat.id,
                        lang.label(
                            "✅ The legacy reply keyboard is hidden.",
                            "✅ تم إخفاء لوحة الأزرار القديمة.",
                        ),
                    )
                    .reply_markup(teloxide::types::KeyboardRemove::new())
                    .await;
            }

            Command::Help => {
                use crate::presentation::telegram::menus::{Language, user_language};
                let lang = user_language(&app_context.pool, actor_user_id as i64).await;
                let help = match lang {
                    Language::Ar => {
                        "❔ <b>مساعدة كاسبا</b>\n\n👛 المحافظ: أرسل عنوان <code>kaspa:...</code> لإضافته، ثم اختر المحفظة لعرض الرصيد والكتل.\n🌐 الشبكة: معلومات العقدة العامة PNN وBlockDAG.\n💹 السوق: سعر KAS والقيمة السوقية.\n🌍 اللغة: المزيد ← اللغة.\n🔔 تصل تنبيهات التعدين بعد التأكيد.\n\n/start — الرئيسية\n/help — المساعدة"
                    }
                    Language::En => {
                        "❔ <b>Kaspa Help</b>\n\n👛 Wallets: send <code>kaspa:...</code> to add a wallet; select it for balance and blocks.\n🌐 Network: public PNN and BlockDAG health.\n💹 Market: KAS price and market cap.\n🌍 Language: More → Language.\n🔔 Mining alerts arrive after confirmation.\n\n/start — Home\n/help — Help"
                    }
                };
                let markup = crate::presentation::telegram::menus::TelegramMenus::home_markup(
                    lang, is_admin,
                );
                let _ = crate::utils::send_logged_message(
                    &bot,
                    chat_id,
                    Some(msg.id),
                    help.to_string(),
                    Some(markup),
                )
                .await;
            }
            Command::Start => {
                let lang = crate::presentation::telegram::menus::user_language(
                    &app_context.pool,
                    actor_user_id as i64,
                )
                .await;
                sync_private_native_commands(&bot, identity, is_admin, lang).await;
                let markup = crate::presentation::telegram::menus::TelegramMenus::home_markup(
                    lang, is_admin,
                );
                let welcome = lang.label(
                    "⛏️ <b>Kaspa Block Alert Bot</b>\nTrack mining rewards, wallets, and live alerts.\n\nSend <code>kaspa:...</code> to add a wallet.\nChoose an action below.",
                    "⛏️ <b>بوت تنبيهات كاسبا</b>\nالمحافظ ومكافآت التعدين والتنبيهات المباشرة.\n\nأرسل عنوان <code>kaspa:...</code> لإضافة محفظة.\nاختر من القائمة المختصرة أدناه."
                );
                let _ = crate::utils::send_logged_message(
                    &bot,
                    msg.chat.id,
                    Some(msg.id),
                    welcome.to_string(),
                    Some(markup),
                )
                .await;
            }

            Command::Admin => {
                if !is_admin {
                    let lang = crate::presentation::telegram::menus::user_language(
                        &app_context.pool,
                        actor_user_id as i64,
                    )
                    .await;
                    let _ = bot
                        .send_message(
                            chat_id,
                            lang.label(
                                "⛔ Only the authorized administrator can open this panel.",
                                "⛔ هذه اللوحة متاحة للمسؤول المعتمد فقط.",
                            ),
                        )
                        .await;
                    return Ok(());
                }
                let lang = crate::presentation::telegram::menus::user_language(
                    &app_context.pool,
                    actor_user_id as i64,
                )
                .await;
                let title = lang.label(
                    "🛡️ <b>Admin</b>\nBot management and subscribers.",
                    "🛡️ <b>الإدارة</b>\nالمشتركون وإدارة التنبيهات والبوت.",
                );
                let _ = crate::utils::send_logged_message(
                    &bot,
                    chat_id,
                    Some(msg.id),
                    title.to_string(),
                    Some(crate::presentation::telegram::menus::TelegramMenus::owner_markup(lang)),
                )
                .await;
            }
            Command::Donate => {
                crate::send_logged!(
                    bot,
                    msg,
                    "❤️ <b>Support Development</b>\n\n<b>KAS Address:</b>\n<code>kaspa:qz0yqq8z3twwgg7lq2mjzg6w4edqys45w2wslz7tym2tc6s84580vvx9zr44g</code>\n🔎 <a href=\"https://kaspa.stream/addresses/kaspa%3Aqz0yqq8z3twwgg7lq2mjzg6w4edqys45w2wslz7tym2tc6s84580vvx9zr44g\">View on Kaspa.stream</a>"
                );
            }

            Command::Add(wallet) => {
                wallet::handle_add(bot, msg, cid, actor_user_id, wallet, ucs.wallet_mgt).await?;
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
                mining::handle_miner(bot, msg, cid, ucs.wallet_query, ucs.miner_stats).await?
            }

            Command::Network => {
                network::handle_network_overview(bot, msg, app_context, ucs.network_stats, None)
                    .await?
            }
            Command::Dag => {
                network::handle_dag(bot, msg, app_context, ucs.dag_uc.clone(), None).await?
            }
            Command::Fees => network::handle_fees(bot, msg, app_context, None).await?,
            Command::Supply => network::handle_supply(bot, msg, app_context, None).await?,
            Command::Price | Command::Market => {
                network::handle_market_data(
                    bot.clone(),
                    msg.clone(),
                    app_context.clone(),
                    ucs.market_stats.clone(),
                    None,
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

    refresh_known_telegram_identity(&app_context.pool, &q.from).await;
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
        restore_safe_callback_menu(
            &app_context,
            &bot,
            &q,
            false,
            raw_message::MAINTENANCE_MESSAGE,
        )
        .await;
        return Ok(());
    }

    if !callback_is_admin && crate::utils::is_callback_rate_limited(identity.actor_user_id) {
        let _ = bot
            .answer_callback_query(q.id)
            .text("Too many requests. Please slow down.")
            .await;
        return Ok(());
    }
    // Read-only network callbacks edit the SAME Telegram message.
    // Do not route them through synthetic /commands that create duplicates.
    if matches!(
        data.as_str(),
        "cmd_network"
            | "refresh_network"
            | "cmd_dag"
            | "refresh_dag"
            | "cmd_supply"
            | "refresh_supply"
            | "cmd_fees"
            | "refresh_fees"
            | "cmd_market"
            | "refresh_market"
            | "cmd_price"
            | "refresh_price"
    ) {
        let _ = bot.answer_callback_query(q.id.clone()).await;
        if let Some(teloxide::types::MaybeInaccessibleMessage::Regular(message)) = q.message {
            let mid = message.id;
            let mut m = *message;
            m.from = Some(q.from.clone());
            match data.as_str() {
                "cmd_network" | "refresh_network" => {
                    network::handle_network_overview(
                        bot,
                        m,
                        app_context,
                        ucs.network_stats,
                        Some(mid),
                    )
                    .await?
                }
                "cmd_dag" | "refresh_dag" => {
                    network::handle_dag(bot, m, app_context, ucs.dag_uc, Some(mid)).await?
                }
                "cmd_supply" | "refresh_supply" => {
                    network::handle_supply(bot, m, app_context, Some(mid)).await?
                }
                "cmd_fees" | "refresh_fees" => {
                    network::handle_fees(bot, m, app_context, Some(mid)).await?
                }
                _ => {
                    network::handle_market_data(bot, m, app_context, ucs.market_stats, Some(mid))
                        .await?
                }
            }
        }
        return Ok(());
    }

    if let Some(raw_page) = data.strip_prefix("ui:admin_subs:") {
        if !callback_is_admin {
            let _ = bot.answer_callback_query(q.id).text("Admin only").await;
            return Ok(());
        }
        let page = raw_page
            .parse::<i64>()
            .ok()
            .filter(|p| (0..=1000).contains(p));
        let Some(page) = page else {
            let _ = bot.answer_callback_query(q.id).text("Invalid page").await;
            return Ok(());
        };
        let lang = crate::presentation::telegram::menus::user_language(
            &app_context.pool,
            identity.actor_user_id as i64,
        )
        .await;
        let _ = bot.answer_callback_query(q.id.clone()).await;
        if let Some(msg) = q.message.as_ref() {
            admin::render_subscribers_panel(
                &bot,
                msg.chat().id,
                msg.id(),
                page,
                lang,
                &app_context.pool,
            )
            .await?;
        }
        return Ok(());
    }
    if let Some(raw_id) = data.strip_prefix("ui:admin_user:") {
        if !callback_is_admin {
            let _ = bot.answer_callback_query(q.id).text("Admin only").await;
            return Ok(());
        }
        let Some(user_id) = raw_id.parse::<i64>().ok().filter(|id| *id > 0) else {
            let _ = bot
                .answer_callback_query(q.id)
                .text("Invalid subscriber")
                .await;
            return Ok(());
        };
        let lang = crate::presentation::telegram::menus::user_language(
            &app_context.pool,
            identity.actor_user_id as i64,
        )
        .await;
        let _ = bot.answer_callback_query(q.id.clone()).await;
        if let Some(msg) = q.message.as_ref() {
            admin::render_subscriber_detail(
                &bot,
                msg.chat().id,
                msg.id(),
                user_id,
                lang,
                &app_context,
            )
            .await?;
        }
        return Ok(());
    }

    if data.starts_with("ui:") {
        use crate::presentation::telegram::menus::{Language, TelegramMenus, user_language};
        let mut lang = user_language(&app_context.pool, identity.actor_user_id as i64).await;
        if let Some(selected) = data.strip_prefix("ui:lang:") {
            if !matches!(selected, "en" | "ar") {
                let _ = bot
                    .answer_callback_query(q.id)
                    .text("Unsupported language")
                    .await;
                return Ok(());
            }
            if let Err(err) = sqlx::query(
                "INSERT INTO public.telegram_user_language(user_id, language) VALUES($1,$2) ON CONFLICT(user_id) DO UPDATE SET language=EXCLUDED.language"
            )
            .bind(identity.actor_user_id as i64)
            .bind(selected)
            .execute(&app_context.pool).await {
                tracing::error!(error=%err, "Language preference could not be persisted");
                let _ = bot.answer_callback_query(q.id).text("Language update unavailable").await;
                return Ok(());
            }
            lang = Language::from_db(selected);
            sync_private_native_commands(&bot, identity, callback_is_admin, lang).await;
        }
        let (title, markup) = match data.as_str() {
            "ui:home" | "ui:lang:en" | "ui:lang:ar" => (
                lang.label(
                    "⛏️ <b>Kaspa Block Alert Bot</b>\nChoose an action.",
                    "⛏️ <b>بوت تنبيهات كاسبا</b>\nاختر الخدمة.",
                ),
                TelegramMenus::home_markup(lang, callback_is_admin),
            ),
            "ui:mining" => (
                lang.label(
                    "⛏️ <b>Mining</b>\nMining rewards and hashrate.",
                    "⛏️ <b>التعدين</b>\nالمكافآت ومعدل التعدين.",
                ),
                TelegramMenus::mining_markup(lang),
            ),
            "ui:network" | "ui:market" => (
                lang.label(
                    "🌐 <b>Network</b>\nChoose a network indicator.",
                    "🌐 <b>الشبكة</b>\nاختر معلومات الشبكة.",
                ),
                TelegramMenus::market_markup(lang),
            ),
            "ui:more" => (
                lang.label(
                    "⚙️ <b>More</b>\nLanguage, help and support.",
                    "⚙️ <b>المزيد</b>\nاللغة والمساعدة والدعم.",
                ),
                TelegramMenus::more_markup(lang),
            ),
            "ui:language" => (
                lang.label(
                    "🌍 <b>Language</b>\nChoose your language.",
                    "🌍 <b>اللغة</b>\nاختر لغة العرض.",
                ),
                TelegramMenus::language_markup(lang),
            ),
            "ui:admin" | "ui:owner" if callback_is_admin => (
                lang.label(
                    "🛡️ <b>Admin</b>\nSubscribers, alerts and settings.",
                    "🛡️ <b>إدارة البوت</b>\nالتشخيص والعمليات.",
                ),
                TelegramMenus::owner_markup(lang),
            ),
            "ui:admin_ops" | "ui:owner_ops" if callback_is_admin => (
                lang.label(
                    "🛠️ <b>Admin Operations</b>\nSensitive changes require confirmation.",
                    "🛠️ <b>عمليات الإدارة</b>\nالتغييرات الحساسة تتطلب تأكيدًا.",
                ),
                TelegramMenus::owner_ops_markup(lang),
            ),
            _ => (
                lang.label("🏠 <b>Home</b>", "🏠 <b>الرئيسية</b>"),
                TelegramMenus::home_markup(lang, callback_is_admin),
            ),
        };
        let _ = bot.answer_callback_query(q.id.clone()).await;
        edit_callback_state(&bot, &q, title, markup).await;
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
                    &app_context,
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
                    &app_context,
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
                    restore_safe_callback_menu(&app_context,
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
                &app_context,
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
                &app_context,
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
                &app_context,
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
                    &app_context,
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
                    &app_context,
                    &bot,
                    &q,
                    true,
                    "✅ <b>Action completed.</b>\nThe admin menu is available again.",
                )
                .await;
            }
            Err(error) => {
                restore_safe_callback_menu(
                    &app_context,
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
            let lang = crate::presentation::telegram::menus::user_language(
                &app_context.pool,
                identity.actor_user_id as i64,
            )
            .await;
            let markup = crate::presentation::telegram::menus::TelegramMenus::home_markup(
                lang,
                callback_is_admin,
            );
            let _ = bot
                .edit_message_text(
                    msg.chat().id,
                    msg.id(),
                    lang.label("✅ Action cancelled.", "✅ أُلغيت العملية."),
                )
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
                &app_context,
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
                &app_context,
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
            restore_safe_callback_menu(&app_context,
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
                    &app_context,
                    &bot,
                    &q,
                    "❌ <b>Wallets were not deleted.</b>\nPlease try again.",
                )
                .await;
                return Err(error.into());
            }

            restore_wallet_callback_menu(
                &app_context,
                &bot,
                &q,
                "🗑️ <b>All tracked wallets deleted.</b>",
            )
            .await;
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
                        &app_context,
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
                &app_context,
                &bot,
                &q,
                false,
                "🗑️ <b>Your user-linked tracking data has been deleted and verified.</b>",
            )
            .await;
        }

        return Ok(());
    }

    if matches!(data.as_str(), "cmd_help" | "cmd_donate") {
        use crate::presentation::telegram::menus::{TelegramMenus, user_language};
        let lang = user_language(&app_context.pool, identity.actor_user_id as i64).await;
        let text = if data == "cmd_help" {
            lang.label(
                "❔ <b>Help</b>\n👛 Wallets: add and manage addresses.\n🌐 Network: live PNN and BlockDAG.\n💹 Market: KAS price.\n🌍 Language: More → Language.\n\nType /start to open Home.",
                "❔ <b>المساعدة</b>\n👛 المحافظ: إضافة العناوين وإدارتها.\n🌐 الشبكة: PNN وBlockDAG.\n💹 السوق: سعر KAS.\n🌍 اللغة: المزيد ← اللغة.\n\nاكتب /start لفتح الرئيسية."
            ).to_owned()
        } else {
            lang.label(
                "❤️ <b>Support</b>\nKAS donation address:\n<code>kaspa:qz0yqq8z3twwgg7lq2mjzg6w4edqys45w2wslz7tym2tc6s84580vvx9zr44g</code>\n🔎 <a href=\"https://kaspa.stream/addresses/kaspa%3Aqz0yqq8z3twwgg7lq2mjzg6w4edqys45w2wslz7tym2tc6s84580vvx9zr44g\">View on Kaspa.stream</a>",
                "❤️ <b>الدعم</b>\nعنوان التبرع بـKAS:\n<code>kaspa:qz0yqq8z3twwgg7lq2mjzg6w4edqys45w2wslz7tym2tc6s84580vvx9zr44g</code>\n🔎 <a href=\"https://kaspa.stream/addresses/kaspa%3Aqz0yqq8z3twwgg7lq2mjzg6w4edqys45w2wslz7tym2tc6s84580vvx9zr44g\">فتح العنوان في Kaspa.stream</a>"
            ).to_owned()
        };
        let _ = bot.answer_callback_query(q.id.clone()).await;
        let markup = if data == "cmd_donate" {
            TelegramMenus::support_markup(lang)
        } else {
            TelegramMenus::more_markup(lang)
        };
        edit_callback_state(&bot, &q, text, markup).await;
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

            let lang = crate::presentation::telegram::menus::user_language(
                &app_context.pool,
                identity.actor_user_id as i64,
            )
            .await;
            let text=lang.label(
                "➕ <b>Add Wallet</b>\nSend your Kaspa wallet address now.\n\nExample:\n<code>kaspa:qq...</code>",
                "➕ <b>إضافة محفظة</b>\nأرسل عنوان محفظة كاسبا الآن.\n\nمثال:\n<code>kaspa:qq...</code>");

            let _ = bot
                .edit_message_text(msg.chat().id, msg.id(), text)
                .parse_mode(ParseMode::Html)
                .reply_markup(InlineKeyboardMarkup::new(vec![vec![
                    InlineKeyboardButton::callback(
                        lang.label("❌ Cancel", "❌ إلغاء"),
                        "cancel_action",
                    ),
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
            render_wallet_panel(
                &bot,
                msg.chat().id,
                msg.id(),
                &ucs,
                msg.chat().id.0,
                &app_context.pool,
            )
            .await?;
        }

        return Ok(());
    }

    if data == "cmd_remove_wallets" {
        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("Choose a wallet.")
            .await;

        if let Some(msg) = q.message {
            render_remove_wallet_panel(
                &bot,
                msg.chat().id,
                msg.id(),
                &ucs,
                msg.chat().id.0,
                &app_context.pool,
            )
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
        restore_wallet_callback_menu(&app_context,
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
                app_context.clone(),
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
                restore_wallet_callback_menu(&app_context, &bot, &q, "⚠️ Invalid wallet action.")
                    .await;
                return Ok(());
            }
            mining::handle_wallet_blocks_detail(
                bot,
                (msg.chat().id, msg.id()),
                msg.chat().id.0,
                token,
                history_page,
                ucs.wallet_query.clone(),
                crate::presentation::telegram::menus::user_language(
                    &app_context.pool,
                    identity.actor_user_id as i64,
                )
                .await,
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
                &app_context,
                &bot,
                &q,
                "❌ <b>Wallet was not removed.</b>\nPlease try again.",
            )
            .await;
            return Err(error);
        }
        return Ok(());
    }

    if data.starts_with("btn_toggle_") && !callback_is_admin {
        let _ = bot
            .answer_callback_query(q.id.clone())
            .text("Admin only")
            .await;
        restore_safe_callback_menu(&app_context, &bot, &q, false, "⛔ Unauthorized.").await;
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
                    &app_context,
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
                &app_context,
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
                &app_context,
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
        "cmd_admin" => Some(Command::Admin),

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

async fn render_wallet_panel(
    bot: &Bot,
    chat_id: teloxide::types::ChatId,
    message_id: teloxide::types::MessageId,
    ucs: &BotUseCases,
    cid: i64,
    pool: &sqlx::PgPool,
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
                    crate::presentation::telegram::menus::TelegramMenus::wallet_menu_markup_lang(
                        crate::presentation::telegram::menus::user_language(pool, cid).await,
                    ),
                )
                .await;
            return Err(error.into());
        }
    };

    // The active wallet is a per-chat selection, never a global default.
    let active: Option<String> =
        sqlx::query_scalar("SELECT wallet FROM public.telegram_active_wallet WHERE chat_id=$1")
            .bind(cid)
            .fetch_optional(pool)
            .await?;
    let lang = crate::presentation::telegram::menus::user_language(pool, cid).await;

    use crate::presentation::telegram::menus::Language;
    let text = if wallets.is_empty() {
        match lang {
            Language::Ar => "👛 <b>المحافظ</b>\n━━━━━━━━━━━━━━━━━━\nلا توجد محافظ مسجلة.\n\nاضغط إضافة محفظة وأرسل عنوان <code>kaspa:...</code>.".to_string(),
            Language::En => "👛 <b>Wallets</b>\n━━━━━━━━━━━━━━━━━━\nNo tracked wallets yet.\n\nPress Add Wallet and send your <code>kaspa:...</code> address.".to_string(),
        }
    } else {
        let list = wallets
            .iter()
            .enumerate()
            .map(|(i, address)| {
                let active_label = if active.as_deref() == Some(address.as_str()) {
                    if lang == Language::Ar {
                        " ✅ النشطة"
                    } else {
                        " ✅ active"
                    }
                } else {
                    ""
                };
                format!(
                    "{}. <code>{}</code>{}",
                    i + 1,
                    crate::utils::html_escape(address),
                    active_label
                )
            })
            .collect::<Vec<_>>()
            .join("\n");
        let (title, prompt) = match lang {
            Language::Ar => ("👛 <b>المحافظ</b>", "اختر محفظة لعرض رصيدها وكتلها."),
            Language::En => (
                "👛 <b>Wallets</b>",
                "Choose a wallet to view its balance and blocks.",
            ),
        };
        format!("{title}\n━━━━━━━━━━━━━━━━━━\n{list}\n\n{prompt}")
    };

    let _ = bot
        .edit_message_text(chat_id, message_id, text)
        .parse_mode(ParseMode::Html)
        .reply_markup(wallet::wallet_buttons_markup_for_lang(
            &wallets,
            cid,
            "wallet_panel",
            true,
            lang,
        ))
        .await?;

    Ok(())
}

async fn render_remove_wallet_panel(
    bot: &Bot,
    chat_id: teloxide::types::ChatId,
    message_id: teloxide::types::MessageId,
    ucs: &BotUseCases,
    cid: i64,
    pool: &sqlx::PgPool,
) -> anyhow::Result<()> {
    let lang = crate::presentation::telegram::menus::user_language(pool, cid).await;
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
                    crate::presentation::telegram::menus::TelegramMenus::wallet_menu_markup_lang(
                        lang,
                    ),
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
                lang.label(
                    "📭 <b>No tracked wallets.</b>\nThere is nothing to remove.",
                    "📭 <b>لا توجد محافظ مسجلة.</b>\nليس هناك ما يمكن حذفه.",
                ),
            )
            .parse_mode(ParseMode::Html)
            .reply_markup(
                crate::presentation::telegram::menus::TelegramMenus::wallet_menu_markup_lang(lang),
            )
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
        lang.label("↩️ Back to Wallets", "↩️ العودة للمحافظ"),
        "cmd_wallets",
    )]);

    let text = lang.label(
        "➖ <b>Remove Wallet</b>\nSelect a wallet to remove.",
        "➖ <b>حذف محفظة</b>\nاختر المحفظة المراد حذفها.",
    );

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

#[cfg(test)]
mod global_english_native_command_tests {
    use super::{may_sync_private_native_commands, native_commands_for_user};
    use crate::domain::models::RequestIdentity;
    use crate::presentation::telegram::menus::Language;

    #[test]
    fn public_slash_picker_defaults_to_english_and_switches_only_on_explicit_arabic() {
        let default = native_commands_for_user(false, Language::En);
        assert_eq!(default.len(), 2);
        assert_eq!(default[0].command, "start");
        assert_eq!(default[0].description, "Open the main menu");

        let selected_ar = native_commands_for_user(false, Language::Ar);
        assert_eq!(selected_ar.len(), 2);
        assert_eq!(selected_ar[0].description, "القائمة الرئيسية");

        let owner_en = native_commands_for_user(true, Language::En);
        assert_eq!(owner_en.len(), 2);
        assert_eq!(owner_en[1].description, "Help and instructions");
        let owner_ar = native_commands_for_user(true, Language::Ar);
        assert_eq!(owner_ar.len(), 2);
        assert_eq!(owner_ar[1].description, "المساعدة والتعليمات");
    }

    #[test]
    fn private_chat_locale_never_writes_shared_group_commands() {
        let own_private = RequestIdentity {
            actor_user_id: 1234,
            chat_id: 1234,
            message_id: 1,
            is_private: true,
        };
        assert!(may_sync_private_native_commands(own_private));
        assert!(!may_sync_private_native_commands(RequestIdentity {
            chat_id: -100002,
            is_private: false,
            ..own_private
        }));
        assert!(!may_sync_private_native_commands(RequestIdentity {
            chat_id: 999,
            ..own_private
        }));
        assert!(!may_sync_private_native_commands(RequestIdentity {
            actor_user_id: u64::MAX,
            ..own_private
        }));
    }
}
