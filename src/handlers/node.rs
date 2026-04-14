use teloxide::{prelude::*, types::ChatId};
use kaspa_addresses::Address;

use crate::context::AppContext;
use crate::utils::{f_num, format_short_wallet, send_or_edit_log, refresh_markup};

pub async fn handle_balance(bot: Bot, chat_id: ChatId, ctx: &AppContext, current_time: String, edit_id: Option<teloxide::types::MessageId>) {
    let mut total = 0.0;
    let mut text = format!("💰 <b>Live Balance</b>\n⏱️ <code>{}</code>\n━━━━━━━━━━━━━━━━━━\n", current_time);
    let wallets: Vec<String> = ctx.state.iter().filter(|e| e.value().contains(&chat_id.0)).map(|e| e.key().clone()).collect();
    
    for w in wallets {
        if let Ok(a) = Address::try_from(w.as_str()) {
            if let Ok(utxos) = ctx.rpc.get_utxos_by_addresses(vec![a]).await {
                let bal = utxos.iter().map(|u| u.utxo_entry.amount as f64).sum::<f64>() / 1e8;
                total += bal;
                text.push_str(&format!("├ <code>{}</code>: {:.8} KAS\n", format_short_wallet(&w), bal));
            }
        }
    }
    text.push_str(&format!("━━━━━━━━━━━━━━━━━━\n💎 <b>Total:</b> <code>{} KAS</code>", f_num(total)));
    let _ = send_or_edit_log(&bot, chat_id, edit_id, text, Some(refresh_markup("refresh_balance"))).await;
}
// (Additional node functions like blocks, miner, network are abstracted to use the AI engine for speed)

