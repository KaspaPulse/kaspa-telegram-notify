use crate::domain::errors::AppError;
use crate::domain::models::AppContext;
use crate::network::stats_use_cases::GetMinerStatsUseCase;
use crate::wallet::wallet_use_cases::WalletQueriesUseCase;
use std::sync::Arc;
use teloxide::prelude::*;

const SOMPI_PER_KAS: i128 = 100_000_000;

const MINING_DATA_UNAVAILABLE_MESSAGE: &str =
    "❌ <b>Mining statistics are temporarily unavailable.</b>\nPlease try again.";

fn log_mining_data_error(operation: &'static str, error: &AppError) {
    tracing::error!(
        operation,
        error = %crate::utils::sanitize_for_log(&error.to_string()),
        "[MINING DATA ERROR] Required mining data operation failed."
    );
}

fn format_with_thousands(value: f64) -> String {
    let sign = if value < 0.0 { "-" } else { "" };
    let rendered = format!("{:.2}", value.abs());
    let mut parts = rendered.split('.');
    let whole = parts.next().unwrap_or("0");
    let frac = parts.next().unwrap_or("00");

    let mut grouped_rev = String::new();
    for (idx, ch) in whole.chars().rev().enumerate() {
        if idx > 0 && idx % 3 == 0 {
            grouped_rev.push(',');
        }
        grouped_rev.push(ch);
    }

    let grouped: String = grouped_rev.chars().rev().collect();
    format!("{sign}{grouped}.{frac}")
}

fn format_kas_from_sompi(total_sompi: i64) -> String {
    let value = total_sompi as f64 / SOMPI_PER_KAS as f64;
    format_with_thousands(value)
}

fn format_usd(value: f64) -> String {
    format!("${}", format_with_thousands(value))
}

fn format_usd_from_sompi(total_sompi: i64, price_usd: f64) -> String {
    let kas_value = total_sompi as f64 / SOMPI_PER_KAS as f64;
    format_usd(kas_value * price_usd)
}

fn format_kas_with_optional_usd(total_sompi: i64, price_usd: Option<f64>) -> String {
    let kas = format!("<code>{}</code> KAS", format_kas_from_sompi(total_sompi));

    match price_usd {
        Some(price) if price.is_finite() && price > 0.0 => {
            format!(
                "{kas} | <code>{}</code>",
                format_usd_from_sompi(total_sompi, price)
            )
        }
        _ => kas,
    }
}
fn format_avg_per_hour(blocks: i64, hours: f64) -> String {
    if hours <= 0.0 {
        return "0.0".to_string();
    }

    format!("{:.1}", blocks as f64 / hours)
}

fn format_daily_blocks_history_row(
    day: &str,
    blocks: i64,
    total_sompi: i64,
    price_usd: Option<f64>,
) -> String {
    format!(
        "<code>{}</code> | <code>{}</code> blk | <code>{}/hr</code> | {}\n",
        day,
        blocks,
        format_avg_per_hour(blocks, 24.0),
        format_kas_with_optional_usd(total_sompi, price_usd)
    )
}

pub async fn handle_blocks(
    bot: Bot,
    msg: Message,
    cid: i64,
    wallet_query: Arc<WalletQueriesUseCase>,
    _app_context: Arc<AppContext>,
) -> anyhow::Result<()> {
    let details = match wallet_query.get_wallet_blocks_details(cid).await {
        Ok(details) => details,
        Err(error) => {
            log_mining_data_error("blocks_command", &error);
            crate::send_logged!(bot, msg, MINING_DATA_UNAVAILABLE_MESSAGE);
            return Ok(());
        }
    };

    if details.is_empty() {
        crate::send_logged!(
            bot,
            msg,
            "📭 <b>No tracked wallets.</b>\nUse /add or press Add Wallet from the menu."
        );
        return Ok(());
    }

    let total_1h: i64 = details.iter().map(|w| w.blocks_1h).sum();
    let total_1h_sompi: i64 = details.iter().map(|w| w.blocks_1h_sompi).sum();
    let total_24h: i64 = details.iter().map(|w| w.blocks_24h).sum();
    let total_24h_sompi: i64 = details.iter().map(|w| w.blocks_24h_sompi).sum();
    let total_7d: i64 = details.iter().map(|w| w.blocks_7d).sum();
    let total_7d_sompi: i64 = details.iter().map(|w| w.blocks_7d_sompi).sum();
    let total_lifetime: i64 = details.iter().map(|w| w.lifetime_blocks).sum();
    let total_lifetime_sompi: i64 = details.iter().map(|w| w.lifetime_sompi).sum();
    let kas_price_usd = details.iter().find_map(|w| w.kas_price_usd);

    let status = if total_1h > 0 {
        "Active 🟢"
    } else {
        "Idle 🟡"
    };

    let wallets: Vec<String> = details.iter().map(|w| w.address.clone()).collect();

    let text = format!(
        "🧱 <b>Mined Blocks</b> | 👛 <code>{}</code> | {}\n\n\
         ⏱ 1H: <code>{}</code> blk | <code>{}/hr</code> | {}\n\
         ⏳ 24H: <code>{}</code> blk | <code>{}/hr</code> | {}\n\
         📆 7D: <code>{}</code> blk | <code>{}/hr</code> | {}\n\
         🏆 All: <code>{}</code> blk | {}\n\n\
         🕒 <code>{}</code>",
        details.len(),
        status,
        total_1h,
        format_avg_per_hour(total_1h, 1.0),
        format_kas_with_optional_usd(total_1h_sompi, kas_price_usd),
        total_24h,
        format_avg_per_hour(total_24h, 24.0),
        format_kas_with_optional_usd(total_24h_sompi, kas_price_usd),
        total_7d,
        format_avg_per_hour(total_7d, 168.0),
        format_kas_with_optional_usd(total_7d_sompi, kas_price_usd),
        total_lifetime,
        format_kas_with_optional_usd(total_lifetime_sompi, kas_price_usd),
        chrono::Utc::now().format("%Y-%m-%d %H:%M UTC")
    );

    let markup = crate::presentation::telegram::handlers::wallet::wallet_buttons_markup(
        &wallets,
        "wallet_blocks",
        true,
    );

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

pub async fn handle_miner(
    bot: Bot,
    msg: Message,
    cid: i64,
    wallet_query: Arc<WalletQueriesUseCase>,
    miner_stats: Arc<GetMinerStatsUseCase>,
) -> anyhow::Result<()> {
    let tracked = match wallet_query.get_list(cid).await {
        Ok(wallets) => wallets,
        Err(error) => {
            log_mining_data_error("miner_command_wallet_list", &error);
            crate::send_logged!(bot, msg, MINING_DATA_UNAVAILABLE_MESSAGE);
            return Ok(());
        }
    };

    if tracked.is_empty() {
        crate::send_logged!(
            bot,
            msg,
            "📭 <b>No tracked wallets.</b>\nUse /add or press Add Wallet from the menu."
        );
        return Ok(());
    }

    let mut global_hashrate = "Unknown".to_string();

    if let Some(first_wallet) = tracked.first() {
        match miner_stats.execute(first_wallet).await {
            Ok(stats) => global_hashrate = stats.global_network_hashrate,
            Err(error) => log_mining_data_error("miner_command_global_hashrate", &error),
        }
    }

    let text = format!(
        "⛏️ <b>Solo-Miner Hashrate</b>\n\
         Community Mining Alerts\n\
         ━━━━━━━━━━━━━━━━━━\n\
         👛 <b>Tracked Wallets:</b> <code>{}</code>\n\
         🌐 <b>Global Hashrate:</b> <code>{}</code>\n\n\
         Select a wallet below to view detailed miner hashrate.\n\n\
         ⏱️ <code>{}</code>",
        tracked.len(),
        global_hashrate,
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );

    let markup = crate::presentation::telegram::handlers::wallet::wallet_buttons_markup(
        &tracked,
        "wallet_miner",
        true,
    );

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

pub async fn handle_wallet_blocks_detail(
    bot: Bot,
    chat_id: teloxide::types::ChatId,
    message_id: teloxide::types::MessageId,
    cid: i64,
    index: usize,
    history_page: usize,
    wallet_query: Arc<WalletQueriesUseCase>,
) -> anyhow::Result<()> {
    let details = match wallet_query.get_wallet_blocks_details(cid).await {
        Ok(details) => details,
        Err(error) => {
            log_mining_data_error("wallet_blocks_detail", &error);
            edit_text(
                &bot,
                chat_id,
                message_id,
                MINING_DATA_UNAVAILABLE_MESSAGE.to_string(),
                crate::presentation::telegram::menus::TelegramMenus::wallet_menu_markup(),
            )
            .await;
            return Err(error.into());
        }
    };

    let Some(detail) = details.get(index) else {
        edit_text(
            &bot,
            chat_id,
            message_id,
            "⚠️ Wallet not found.".to_string(),
            crate::presentation::telegram::menus::TelegramMenus::main_menu_markup(),
        )
        .await;
        return Ok(());
    };

    let status = if detail.blocks_1h > 0 {
        "Active 🟢"
    } else {
        "Idle 🟡"
    };

    const DEFAULT_BLOCKS_HISTORY_PAGE_SIZE: usize = 15;

    let page_size: usize = std::env::var("BLOCKS_HISTORY_PAGE_SIZE")
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .map(|value| value.clamp(5, 50))
        .unwrap_or(DEFAULT_BLOCKS_HISTORY_PAGE_SIZE);

    let total_days = detail.daily_blocks.len();
    let total_pages = if total_days == 0 {
        1
    } else {
        total_days.div_ceil(page_size)
    };

    let history_page = history_page.min(total_pages.saturating_sub(1));
    let start = history_page.saturating_mul(page_size);
    let end = (start + page_size).min(total_days);

    let mut daily_text = format!(
        "\u{1F4CA} <b>Blocks, Rates & KAS</b>\n\
         1H: <code>{}</code> blk | <code>{}/hr</code> | {}\n\
         24H: <code>{}</code> blk | <code>{}/hr</code> | {}\n\
         7D: <code>{}</code> blk | <code>{}/hr</code> | {}\n\
         Lifetime: <code>{}</code> blk | {}\n\
         \u{1F4C8} <b>Status:</b> {}\n\n",
        detail.blocks_1h,
        format_avg_per_hour(detail.blocks_1h, 1.0),
        format_kas_with_optional_usd(detail.blocks_1h_sompi, detail.kas_price_usd),
        detail.blocks_24h,
        format_avg_per_hour(detail.blocks_24h, 24.0),
        format_kas_with_optional_usd(detail.blocks_24h_sompi, detail.kas_price_usd),
        detail.blocks_7d,
        format_avg_per_hour(detail.blocks_7d, 168.0),
        format_kas_with_optional_usd(detail.blocks_7d_sompi, detail.kas_price_usd),
        detail.lifetime_blocks,
        format_kas_with_optional_usd(detail.lifetime_sompi, detail.kas_price_usd),
        status
    );

    if !detail.daily_blocks.is_empty() {
        daily_text.push_str(&format!(
            "\u{1F4C5} <b>Full History:</b> page <code>{}/{}</code> | days <code>{}</code>\n",
            history_page + 1,
            total_pages,
            total_days
        ));

        for (day, count, total_sompi, price_usd) in detail.daily_blocks[start..end].iter() {
            daily_text.push_str(&format_daily_blocks_history_row(
                day,
                *count,
                *total_sompi,
                *price_usd,
            ));
        }
    } else {
        daily_text.push_str("\u{1F4C5} <b>Full History:</b> <code>No blocks</code>\n");
    }
    let text = format!(
        "🧱 <b>Wallet {} Blocks</b>\n\
         ━━━━━━━━━━━━━━━━━━\n\
         <code>{}</code>\n\n\
         {}\
         ⏱️ <code>{}</code>",
        index + 1,
        detail.address,
        daily_text,
        chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
    );

    edit_text(
        &bot,
        chat_id,
        message_id,
        text,
        blocks_history_markup(index, history_page, total_pages),
    )
    .await;

    Ok(())
}

fn blocks_history_markup(
    index: usize,
    history_page: usize,
    total_pages: usize,
) -> teloxide::types::InlineKeyboardMarkup {
    use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

    let mut rows = vec![vec![InlineKeyboardButton::callback(
        "\u{1F504} Refresh",
        format!("wallet_blocks_{}_{}", index, history_page),
    )]];

    let mut page_nav = Vec::new();

    if history_page > 0 {
        page_nav.push(InlineKeyboardButton::callback(
            "\u{2B05}\u{FE0F} Previous",
            format!("wallet_blocks_{}_{}", index, history_page - 1),
        ));
    }

    if history_page + 1 < total_pages {
        page_nav.push(InlineKeyboardButton::callback(
            "Next \u{27A1}\u{FE0F}",
            format!("wallet_blocks_{}_{}", index, history_page + 1),
        ));
    }

    if !page_nav.is_empty() {
        rows.push(page_nav);
    }

    rows.push(vec![
        InlineKeyboardButton::callback("\u{1F45B} Wallet Panel", format!("wallet_panel_{}", index)),
        InlineKeyboardButton::callback("\u{2B05}\u{FE0F} Back", "cmd_blocks"),
    ]);

    InlineKeyboardMarkup::new(rows)
}
pub async fn handle_wallet_miner_detail(
    bot: Bot,
    chat_id: teloxide::types::ChatId,
    message_id: teloxide::types::MessageId,
    cid: i64,
    index: usize,
    wallet_query: Arc<WalletQueriesUseCase>,
    miner_stats: Arc<GetMinerStatsUseCase>,
) -> anyhow::Result<()> {
    let tracked = match wallet_query.get_list(cid).await {
        Ok(wallets) => wallets,
        Err(error) => {
            log_mining_data_error("wallet_miner_detail_wallet_list", &error);
            edit_text(
                &bot,
                chat_id,
                message_id,
                MINING_DATA_UNAVAILABLE_MESSAGE.to_string(),
                crate::presentation::telegram::menus::TelegramMenus::wallet_menu_markup(),
            )
            .await;
            return Err(error.into());
        }
    };

    let Some(wallet) = tracked.get(index) else {
        edit_text(
            &bot,
            chat_id,
            message_id,
            "⚠️ Wallet not found.".to_string(),
            crate::presentation::telegram::menus::TelegramMenus::main_menu_markup(),
        )
        .await;
        return Ok(());
    };

    let text = match miner_stats.execute(wallet).await {
        Ok(stats) => format!(
            "⛏️ <b>Wallet {} Miner</b>\n\
             ━━━━━━━━━━━━━━━━━━\n\
             <code>{}</code>\n\n\
             🌐 <b>Global Hashrate:</b> <code>{}</code>\n\
             📊 <b>Actual Hashrate:</b>\n\
             ├ 1H: <code>{}</code>\n\
             ├ 24H: <code>{}</code>\n\
             └ 7D: <code>{}</code>\n\n\
             ⚡ <b>Unspent Hashrate:</b>\n\
             ├ 1H: <code>{}</code>\n\
             ├ 24H: <code>{}</code>\n\
             └ 7D: <code>{}</code>\n\n\
             ⏱️ <code>{}</code>",
            index + 1,
            wallet,
            stats.global_network_hashrate,
            stats.actual_hashrate_1h,
            stats.actual_hashrate_24h,
            stats.actual_hashrate_7d,
            stats.unspent_hashrate_1h,
            stats.unspent_hashrate_24h,
            stats.unspent_hashrate_7d,
            chrono::Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
        ),
        Err(error) => {
            log_mining_data_error("wallet_miner_detail_stats", &error);
            MINING_DATA_UNAVAILABLE_MESSAGE.to_string()
        }
    };

    edit_text(
        &bot,
        chat_id,
        message_id,
        text,
        crate::presentation::telegram::handlers::wallet::wallet_panel_markup(index),
    )
    .await;

    Ok(())
}

async fn edit_text(
    bot: &Bot,
    chat_id: teloxide::types::ChatId,
    message_id: teloxide::types::MessageId,
    text: String,
    markup: teloxide::types::InlineKeyboardMarkup,
) {
    let _ = crate::utils::edit_logged_message(bot, chat_id, message_id, text, Some(markup)).await;
}
