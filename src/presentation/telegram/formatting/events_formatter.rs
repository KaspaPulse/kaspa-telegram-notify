use crate::domain::models::LiveBlockEvent;
use crate::utils::{format_hash, format_short_wallet};
use chrono::{TimeZone, Utc};

pub fn format_live_event(event: &LiveBlockEvent) -> String {
    let time_str = if event.block_time_ms > 0 {
        if let chrono::LocalResult::Single(dt) =
            Utc.timestamp_millis_opt(event.block_time_ms as i64)
        {
            dt.format("%Y-%m-%d %H:%M:%S.%3f UTC").to_string()
        } else {
            Utc::now().format("%Y-%m-%d %H:%M:%S.%3f UTC").to_string()
        }
    } else {
        Utc::now().format("%Y-%m-%d %H:%M:%S.%3f UTC").to_string()
    };

    let header_emoji = if event.is_coinbase {
        "⚡ <b>Native Node Reward!</b>"
    } else {
        "💸 <b>Incoming Transfer!</b>"
    };
    let acc_block_str = if event.acc_block_hash.is_empty() {
        "<code>Archived</code>".to_string()
    } else {
        format_hash(&event.acc_block_hash, "blocks")
    };
    let mined_block_str = match &event.mined_block_hash {
        Some(hash) => format_hash(hash, "blocks"),
        None => {
            if event.is_coinbase {
                "<code>Unknown</code>".to_string()
            } else {
                "<code>N/A</code>".to_string()
            }
        }
    };

    let mut final_msg = format!("{}\n━━━━━━━━━━━━━━━━━━\n<b>Time:</b> <code>{}</code>\n<b>Wallet:</b> {}\n<b>Amount:</b> <code>+{:.8} KAS</code>\n<b>Balance:</b> <code>{:.8} KAS</code>\n<blockquote expandable>",
        header_emoji, time_str, format_short_wallet(&event.wallet_address), event.amount_kas, event.live_balance_kas);

    final_msg.push_str(&format!(
        "<b>TXID:</b> {}\n",
        format_hash(&event.tx_id, "transactions")
    ));
    if event.is_coinbase {
        final_msg.push_str(&format!(
            "<b>Mined Block:</b> {}\n<b>Accepting Block:</b> {}\n",
            mined_block_str, acc_block_str
        ));
        if let Some(worker) = &event.extracted_worker {
            let safe_worker = teloxide::utils::html::escape(worker);
            final_msg.push_str(&format!("<b>Worker:</b> <code>{}</code>\n", safe_worker));
        }
    } else {
        final_msg.push_str(&format!("<b>Accepting Block:</b> {}\n", acc_block_str));
    }
    final_msg.push_str(&format!(
        "<b>DAA Score:</b> <code>{}</code>\n</blockquote>",
        event.daa_score
    ));
    final_msg
}
