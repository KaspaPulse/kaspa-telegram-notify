use crate::domain::entities::WalletRemovalOutcome;
use crate::wallet::wallet_use_cases::*;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

const WALLET_DATA_UNAVAILABLE_MESSAGE: &str =
    "❌ <b>Wallet data is temporarily unavailable.</b>\nPlease try again.";

pub(super) fn wallet_data_unavailable_message() -> &'static str {
    WALLET_DATA_UNAVAILABLE_MESSAGE
}

pub(super) fn log_wallet_data_error(
    operation: &'static str,
    error: &crate::domain::errors::AppError,
) {
    tracing::error!(
        operation,
        error = %crate::utils::sanitize_for_log(&error.to_string()),
        "[DATABASE ERROR] Wallet data operation failed."
    );
}

const WALLET_CALLBACK_TOKEN_BYTES: usize = 16;

pub fn wallet_callback_token(chat_id: i64, wallet: &str) -> String {
    let digest = Sha256::digest(format!("wallet-callback-v1:{chat_id}:{wallet}").as_bytes());
    digest[..WALLET_CALLBACK_TOKEN_BYTES]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

pub fn resolve_wallet_token<'a>(
    wallets: &'a [String],
    chat_id: i64,
    token: &str,
) -> Option<(usize, &'a str)> {
    if token.len() != WALLET_CALLBACK_TOKEN_BYTES * 2
        || !token.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return None;
    }

    wallets.iter().enumerate().find_map(|(index, wallet)| {
        (wallet_callback_token(chat_id, wallet) == token).then_some((index, wallet.as_str()))
    })
}

fn wallet_callback(prefix: &str, chat_id: i64, wallet: &str) -> String {
    let short_prefix = match prefix {
        "wallet_panel" => "wp",
        "wallet_balance" => "wbal",
        "wallet_blocks" => "wblk",
        "wallet_miner" => "wmin",
        other => other,
    };
    format!("{short_prefix}:{}", wallet_callback_token(chat_id, wallet))
}

pub async fn handle_add(
    bot: Bot,
    msg: Message,
    cid: i64,
    actor_user_id: u64,
    wallet: String,
    wallet_mgt: Arc<WalletManagementUseCase>,
) -> anyhow::Result<()> {
    let clean_wallet = crate::utils::normalize_wallet_input(&wallet);

    if crate::utils::is_add_wallet_rate_limited(actor_user_id) {
        crate::send_logged!(bot, msg, crate::utils::rate_limit_message());
        return Ok(());
    }

    if let Err(reason) = crate::utils::validate_wallet_address_size(&clean_wallet) {
        crate::send_logged!(bot, msg, format!("🚫 <b>Wallet rejected.</b>\n{}", reason));
        return Ok(());
    }
    if clean_wallet.is_empty() {
        crate::send_logged!(
            bot,
            msg,
            "⚠️ <b>Usage:</b> /add <code>kaspa:your_wallet_address</code>"
        );
        return Ok(());
    }

    if let Err(reason) = crate::utils::validate_wallet_security(&clean_wallet) {
        crate::send_logged!(
            bot,
            msg,
            format!(
                "🚫 <b>Invalid wallet address.</b>\n{}",
                crate::utils::html_escape(&reason)
            )
        );
        return Ok(());
    }

    match wallet_mgt.add_wallet(&clean_wallet, cid).await {
        Ok(_) => {
            crate::send_logged!(
                bot,
                msg,
                format!(
                    "✅ <b>Wallet Added</b>\nNow tracking:\n<code>{}</code>",
                    crate::utils::html_escape(&clean_wallet)
                )
            );
        }
        Err(e) => {
            crate::send_logged!(
                bot,
                msg,
                format!(
                    "❌ <b>Error:</b> {}",
                    crate::utils::html_escape(&e.to_string())
                )
            );
        }
    }

    Ok(())
}

pub async fn handle_remove(
    bot: Bot,
    msg: Message,
    cid: i64,
    wallet: String,
    wallet_mgt: Arc<WalletManagementUseCase>,
) -> anyhow::Result<()> {
    let clean_wallet = crate::utils::normalize_wallet_input(&wallet);

    if let Err(reason) = crate::utils::validate_wallet_address_size(&clean_wallet) {
        crate::send_logged!(bot, msg, format!("🚫 <b>Wallet rejected.</b>\n{}", reason));
        return Ok(());
    }
    if let Err(reason) = crate::utils::validate_wallet_security(&clean_wallet) {
        crate::send_logged!(
            bot,
            msg,
            format!(
                "🚫 <b>Invalid wallet address.</b>\n{}",
                crate::utils::html_escape(&reason)
            )
        );
        return Ok(());
    }

    match wallet_mgt.remove_wallet(&clean_wallet, cid).await {
        Ok(WalletRemovalOutcome::Removed) => {
            crate::send_logged!(bot, msg, "🗑️ <b>Wallet Removed.</b>");
        }
        Ok(WalletRemovalOutcome::NotFound) => {
            crate::send_logged!(
                bot,
                msg,
                "ℹ️ <b>Wallet not tracked.</b>\nNo wallet was removed."
            );
        }
        Err(e) => {
            crate::send_logged!(
                bot,
                msg,
                format!(
                    "❌ <b>Error:</b> {}",
                    crate::utils::html_escape(&e.to_string())
                )
            );
        }
    }

    Ok(())
}

pub async fn handle_list(
    bot: Bot,
    msg: Message,
    cid: i64,
    wallet_query: Arc<WalletQueriesUseCase>,
) -> anyhow::Result<()> {
    let wallets = match wallet_query.get_list(cid).await {
        Ok(wallets) => wallets,
        Err(error) => {
            log_wallet_data_error("list_wallets_command", &error);
            crate::send_logged!(bot, msg, WALLET_DATA_UNAVAILABLE_MESSAGE);
            return Ok(());
        }
    };

    if wallets.is_empty() {
        crate::send_logged!(
            bot,
            msg,
            "📭 <b>No tracked wallets.</b>\nUse /add or press Add Wallet from the menu."
        );
        return Ok(());
    }

    let text = wallet_list_text(&wallets);
    let markup = wallet_buttons_markup(&wallets, cid, "wallet_panel", true);

    let _ = crate::utils::send_logged_message(&bot, msg.chat.id, Some(msg.id), text, Some(markup))
        .await;

    Ok(())
}

pub async fn handle_balance(
    bot: Bot,
    msg: Message,
    cid: i64,
    wallet_query: Arc<WalletQueriesUseCase>,
    app_context: Arc<crate::domain::models::AppContext>,
) -> anyhow::Result<()> {
    let wallet_details = match wallet_query.get_wallet_balances(cid).await {
        Ok(details) => details,
        Err(error) => {
            log_wallet_data_error("wallet_balance_command", &error);
            crate::send_logged!(bot, msg, WALLET_DATA_UNAVAILABLE_MESSAGE);
            return Ok(());
        }
    };

    if wallet_details.is_empty() {
        crate::send_logged!(
            bot,
            msg,
            "📭 <b>No tracked wallets.</b>\nUse /add or press Add Wallet from the menu."
        );
        return Ok(());
    }

    let kas_price = app_context.price_cache.read().await.0;

    let total_sompi: u64 = wallet_details.iter().map(|w| w.balance_sompi).sum();
    let total_utxos: usize = wallet_details.iter().map(|w| w.utxos).sum();
    let total_kas = total_sompi as f64 / 1e8;
    let total_value = total_kas * kas_price;
    let avg_utxo = if total_utxos > 0 {
        total_kas / total_utxos as f64
    } else {
        0.0
    };

    let wallets: Vec<String> = wallet_details.iter().map(|w| w.address.clone()).collect();

    let text = format!(
        "💰 <b>Wallet Analytics</b>\n\
         Community Mining Alerts\n\
         ━━━━━━━━━━━━━━━━━━\n\
         👛 <b>Tracked Wallets:</b> <code>{}</code>\n\
         💵 <b>Total Balance:</b> <code>{:.2} KAS</code>\n\
         💲 <b>Total Value:</b> <code>${:.2} USD</code>\n\
         🔄 <b>Total UTXOs:</b> <code>{}</code>\n\
         📊 <b>Average UTXO:</b> <code>{:.2} KAS</code>\n\n\
         Select a wallet below to view detailed balance.\n\n\
         ⏱️ <code>{}</code>",
        wallet_details.len(),
        total_kas,
        total_value,
        total_utxos,
        avg_utxo,
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );

    let markup = wallet_buttons_markup(&wallets, cid, "wallet_balance", true);

    let _ = crate::utils::send_reply_or_edit_log(
        &bot,
        msg.chat.id,
        msg.id,
        msg.from.as_ref().filter(|u| u.is_bot).map(|_| msg.id),
        text,
        Some(markup),
    )
    .await;

    Ok(())
}

pub async fn handle_wallet_panel(
    bot: Bot,
    chat_id: teloxide::types::ChatId,
    message_id: teloxide::types::MessageId,
    cid: i64,
    wallet_token: &str,
    wallet_query: Arc<WalletQueriesUseCase>,
) -> anyhow::Result<()> {
    let wallets = match wallet_query.get_list(cid).await {
        Ok(wallets) => wallets,
        Err(error) => {
            log_wallet_data_error("wallet_panel", &error);
            edit_text(
                &bot,
                chat_id,
                message_id,
                WALLET_DATA_UNAVAILABLE_MESSAGE.to_string(),
                crate::presentation::telegram::menus::TelegramMenus::wallet_menu_markup(),
            )
            .await;
            return Err(error.into());
        }
    };

    let Some((index, address)) = resolve_wallet_token(&wallets, cid, wallet_token) else {
        restore_stale_wallet_button(&bot, chat_id, message_id).await;
        return Ok(());
    };

    let text = format!(
        "{} <b>Wallet {}</b>\n\
         ━━━━━━━━━━━━━━━━━━\n\
         <code>{}</code>\n\n\
         Choose what you want to view.",
        wallet_number_emoji(index + 1),
        index + 1,
        address
    );

    edit_text(
        &bot,
        chat_id,
        message_id,
        text,
        wallet_panel_markup(cid, address),
    )
    .await;
    Ok(())
}

pub async fn handle_wallet_balance_detail(
    bot: Bot,
    chat_id: teloxide::types::ChatId,
    message_id: teloxide::types::MessageId,
    cid: i64,
    wallet_token: &str,
    wallet_query: Arc<WalletQueriesUseCase>,
    app_context: Arc<crate::domain::models::AppContext>,
) -> anyhow::Result<()> {
    let details = match wallet_query.get_wallet_balances(cid).await {
        Ok(details) => details,
        Err(error) => {
            log_wallet_data_error("wallet_balance_detail", &error);
            edit_text(
                &bot,
                chat_id,
                message_id,
                WALLET_DATA_UNAVAILABLE_MESSAGE.to_string(),
                crate::presentation::telegram::menus::TelegramMenus::wallet_menu_markup(),
            )
            .await;
            return Err(error.into());
        }
    };

    let Some((index, detail)) = details
        .iter()
        .enumerate()
        .find(|(_, detail)| wallet_callback_token(cid, &detail.address) == wallet_token)
    else {
        restore_stale_wallet_button(&bot, chat_id, message_id).await;
        return Ok(());
    };

    let kas_price = app_context.price_cache.read().await.0;
    let balance_kas = detail.balance_sompi as f64 / 1e8;
    let fiat_value = balance_kas * kas_price;
    let avg_utxo = if detail.utxos > 0 {
        balance_kas / detail.utxos as f64
    } else {
        0.0
    };
    let status = if detail.is_online {
        "Online 🟢"
    } else {
        "Unavailable 🔴"
    };

    let text = format!(
        "💰 <b>Wallet {} Balance</b>\n\
         ━━━━━━━━━━━━━━━━━━\n\
         <code>{}</code>\n\n\
         💵 <b>Balance:</b> <code>{:.2} KAS</code>\n\
         💲 <b>Value:</b> <code>${:.2} USD</code>\n\
         🔄 <b>UTXOs:</b> <code>{}</code>\n\
         📊 <b>Average UTXO:</b> <code>{:.2} KAS</code>\n\
         🩺 <b>Status:</b> <code>{}</code>\n\n\
         ⏱️ <code>{}</code>",
        index + 1,
        detail.address,
        balance_kas,
        fiat_value,
        detail.utxos,
        avg_utxo,
        status,
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );

    edit_text(
        &bot,
        chat_id,
        message_id,
        text,
        wallet_panel_markup(cid, &detail.address),
    )
    .await;
    Ok(())
}

pub async fn handle_wallet_remove_confirm(
    bot: Bot,
    chat_id: teloxide::types::ChatId,
    message_id: teloxide::types::MessageId,
    cid: i64,
    wallet_token: &str,
    wallet_query: Arc<WalletQueriesUseCase>,
) -> anyhow::Result<()> {
    let wallets = match wallet_query.get_list(cid).await {
        Ok(wallets) => wallets,
        Err(error) => {
            log_wallet_data_error("wallet_remove_confirm", &error);
            edit_text(
                &bot,
                chat_id,
                message_id,
                WALLET_DATA_UNAVAILABLE_MESSAGE.to_string(),
                crate::presentation::telegram::menus::TelegramMenus::wallet_menu_markup(),
            )
            .await;
            return Err(error.into());
        }
    };

    let Some((index, address)) = resolve_wallet_token(&wallets, cid, wallet_token) else {
        restore_stale_wallet_button(&bot, chat_id, message_id).await;
        return Ok(());
    };

    let text = format!(
        "⚠️ <b>Confirm Remove Wallet {}</b>\n\
         ━━━━━━━━━━━━━━━━━━\n\
         <code>{}</code>\n\n\
         Are you sure you want to remove this exact wallet?",
        index + 1,
        address
    );

    edit_text(
        &bot,
        chat_id,
        message_id,
        text,
        confirm_remove_markup(cid, address),
    )
    .await;
    Ok(())
}

async fn restore_wallet_removal_state(
    bot: &Bot,
    chat_id: teloxide::types::ChatId,
    message_id: teloxide::types::MessageId,
    text: impl Into<String>,
) {
    let text = text.into();
    let markup = crate::presentation::telegram::menus::TelegramMenus::wallet_menu_markup();
    let fallback_markup = markup.clone();

    if let Err(edit_error) = bot
        .edit_message_text(chat_id, message_id, text.clone())
        .parse_mode(teloxide::types::ParseMode::Html)
        .reply_markup(markup)
        .await
    {
        tracing::warn!(
            "[CALLBACK UI] Failed to restore wallet removal message {} in chat {}: {}",
            message_id.0,
            chat_id.0,
            edit_error
        );

        if let Err(send_error) = bot
            .send_message(chat_id, text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .reply_markup(fallback_markup)
            .await
        {
            tracing::error!(
                "[CALLBACK UI] Failed to send wallet removal replacement panel in chat {}: {}",
                chat_id.0,
                send_error
            );
        }
    }
}

async fn restore_stale_wallet_button(
    bot: &Bot,
    chat_id: teloxide::types::ChatId,
    message_id: teloxide::types::MessageId,
) {
    restore_wallet_removal_state(
        bot,
        chat_id,
        message_id,
        "⏳ <b>This wallet button is stale or no longer valid.</b>\nOpen Wallets again.",
    )
    .await;
}

pub async fn handle_wallet_remove_do(
    bot: Bot,
    chat_id: teloxide::types::ChatId,
    message_id: teloxide::types::MessageId,
    cid: i64,
    wallet_token: &str,
    wallet_query: Arc<WalletQueriesUseCase>,
    wallet_mgt: Arc<WalletManagementUseCase>,
) -> anyhow::Result<()> {
    let wallets = wallet_query.get_list(cid).await?;
    let Some((index, address)) = resolve_wallet_token(&wallets, cid, wallet_token) else {
        restore_wallet_removal_state(
            &bot,
            chat_id,
            message_id,
            "⏳ <b>This wallet removal confirmation is stale.</b>\nNo wallet was removed.",
        )
        .await;
        return Ok(());
    };

    match wallet_mgt.remove_wallet(address, cid).await? {
        WalletRemovalOutcome::Removed => {}
        WalletRemovalOutcome::NotFound => {
            restore_wallet_removal_state(
                &bot,
                chat_id,
                message_id,
                "ℹ️ <b>Wallet was already absent.</b>\nNo additional wallet was removed.",
            )
            .await;
            return Ok(());
        }
    }

    let text = format!(
        "🗑️ <b>Wallet Removed</b>\n\
         ━━━━━━━━━━━━━━━━━━\n\
         Wallet {} was removed.",
        index + 1
    );
    restore_wallet_removal_state(&bot, chat_id, message_id, text).await;
    Ok(())
}

pub fn wallet_buttons_markup(
    wallets: &[String],
    chat_id: i64,
    callback_prefix: &str,
    include_main_menu: bool,
) -> InlineKeyboardMarkup {
    let mut rows: Vec<Vec<InlineKeyboardButton>> = Vec::new();

    for (index, wallet) in wallets.iter().enumerate() {
        rows.push(vec![InlineKeyboardButton::callback(
            format!(
                "{} Wallet {} - {}",
                wallet_number_emoji(index + 1),
                index + 1,
                crate::utils::format_short_wallet(wallet)
            ),
            wallet_callback(callback_prefix, chat_id, wallet),
        )]);
    }

    if include_main_menu {
        rows.push(vec![InlineKeyboardButton::callback(
            "🔙 Main Menu",
            "cmd_start",
        )]);
    }

    InlineKeyboardMarkup::new(rows)
}

pub fn wallet_panel_markup(chat_id: i64, wallet: &str) -> InlineKeyboardMarkup {
    let token = wallet_callback_token(chat_id, wallet);
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("💰 Balance", format!("wbal:{token}")),
            InlineKeyboardButton::callback("🧱 Blocks", format!("wblk:{token}")),
        ],
        vec![
            InlineKeyboardButton::callback("⛏️ Miner", format!("wmin:{token}")),
            InlineKeyboardButton::callback("➖ Remove", format!("wrc:{token}")),
        ],
        vec![
            InlineKeyboardButton::callback("👛 All Wallets", "cmd_wallets"),
            InlineKeyboardButton::callback("🔙 Main Menu", "cmd_start"),
        ],
    ])
}

fn confirm_remove_markup(chat_id: i64, wallet: &str) -> InlineKeyboardMarkup {
    let token = wallet_callback_token(chat_id, wallet);
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback("✅ Yes, remove wallet", format!("wrd:{token}")),
            InlineKeyboardButton::callback("❌ Cancel", format!("wp:{token}")),
        ],
        vec![InlineKeyboardButton::callback("🔙 Main Menu", "cmd_start")],
    ])
}

fn wallet_list_text(wallets: &[String]) -> String {
    let list = wallets
        .iter()
        .enumerate()
        .map(|(index, wallet)| {
            format!(
                "{} <b>Wallet {}:</b> <code>{}</code>",
                wallet_number_emoji(index + 1),
                index + 1,
                crate::utils::format_short_wallet(wallet)
            )
        })
        .collect::<Vec<_>>()
        .join("\n");

    format!(
        "👛 <b>Tracked Wallets</b>\n\
         ━━━━━━━━━━━━━━━━━━\n\
         {}\n\n\
         Select a wallet below to open its panel.",
        list
    )
}

fn wallet_number_emoji(number: usize) -> &'static str {
    match number {
        1 => "1️⃣",
        2 => "2️⃣",
        3 => "3️⃣",
        4 => "4️⃣",
        5 => "5️⃣",
        6 => "6️⃣",
        7 => "7️⃣",
        8 => "8️⃣",
        9 => "9️⃣",
        10 => "🔟",
        _ => "🔹",
    }
}

async fn edit_text(
    bot: &Bot,
    chat_id: teloxide::types::ChatId,
    message_id: teloxide::types::MessageId,
    text: String,
    markup: InlineKeyboardMarkup,
) {
    let _ = crate::utils::edit_logged_message(bot, chat_id, message_id, text, Some(markup)).await;
}
