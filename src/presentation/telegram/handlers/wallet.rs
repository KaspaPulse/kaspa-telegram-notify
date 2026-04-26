use crate::wallet::wallet_use_cases::*;
use std::sync::Arc;
use teloxide::prelude::*;

pub async fn handle_add(
    bot: Bot,
    msg: Message,
    cid: i64,
    wallet: String,
    wallet_mgt: Arc<WalletManagementUseCase>,
) -> anyhow::Result<()> {
    if wallet.is_empty() {
        crate::send_logged!(bot, msg, "⚠️ Usage: /add <code>wallet_address</code>");
        return Ok(());
    }
    match wallet_mgt.add_wallet(&wallet, cid).await {
        Ok(_) => {
            crate::send_logged!(
                bot,
                msg,
                format!("✅ <b>Wallet Added!</b>\nTracking: {}", wallet)
            );
        }
        Err(e) => {
            crate::send_logged!(bot, msg, format!("❌ <b>Error:</b> {}", e));
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
    // [STRICT VALIDATION] Kaspa Address Firewall
    let clean_addr = wallet.trim();
    let is_valid = clean_addr.starts_with("kaspa:")
        && clean_addr
            .chars()
            .skip(6)
            .all(|c| c.is_ascii_alphanumeric());

    if !is_valid {
        crate::send_logged!(
            bot,
            msg,
            "🚫 <b>Security Alert:</b> Invalid address format."
        );
        return Ok(());
    }
    match wallet_mgt.remove_wallet(&wallet, cid).await {
        Ok(_) => {
            crate::send_logged!(bot, msg, "🗑️ <b>Wallet Removed.</b>");
        }
        Err(e) => {
            crate::send_logged!(bot, msg, format!("❌ <b>Error:</b> {}", e));
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
    match wallet_query.get_list(cid).await {
        Ok(list) => {
            if list.is_empty() {
                crate::send_logged!(bot, msg, "📭 You are not tracking any wallets.");
            } else {
                crate::send_logged!(
                    bot,
                    msg,
                    format!(
                        "📋 <b>Tracked Wallets:\n<code>{}</code></b>",
                        list.join("\n")
                    )
                );
            }
        }
        Err(e) => {
            crate::send_logged!(bot, msg, format!("❌ {}", e));
        }
    }
    Ok(())
}

pub async fn handle_balance(
    bot: teloxide::prelude::Bot,
    msg: teloxide::prelude::Message,
    cid: i64,
    wallet_query: std::sync::Arc<crate::wallet::wallet_use_cases::WalletQueriesUseCase>,
    _app_context: std::sync::Arc<crate::domain::models::AppContext>,
) -> anyhow::Result<()> {
    match wallet_query.get_balance(cid).await {
        Ok((bal, utxos)) => {
            let total_bal = bal as f64 / 1e8;
            let total_utxos = utxos;
            let avg_utxo = if total_utxos > 0 {
                total_bal / total_utxos as f64
            } else {
                0.0
            };

            let mut fiat_price = 0.0;
            if let Ok(r) = reqwest::get("https://api.kaspa.org/info/price").await {
                if let Ok(j) = r.json::<serde_json::Value>().await {
                    fiat_price = j["price"].as_f64().unwrap_or(0.0);
                }
            }
            let fiat_val = total_bal * fiat_price;

            let text = format!("💰 <b>Enterprise Wallet Analytics</b>\n━━━━━━━━━━━━━━━━━━\n💵 <b>Total Balance:</b> <code>{:.2} KAS</code>\n💲 <b>Fiat Value:</b> <code>${:.2} USD</code>\n🔄 <b>Active UTXOs:</b> <code>{}</code>\n📊 <b>Avg UTXO Size:</b> <code>{:.2} KAS</code>", total_bal, fiat_val, total_utxos, avg_utxo);
            let markup = crate::utils::refresh_markup("refresh_balance");
            let _ = crate::utils::send_or_edit_log(&bot, msg.chat.id, msg.from.as_ref().filter(|u| u.is_bot).map(|_| msg.id), text, Some(markup)).await;
        }
        Err(e) => {
            crate::send_logged!(bot, msg, format!("❌ Error: {}", e));
        }
    }
    Ok(())
}

