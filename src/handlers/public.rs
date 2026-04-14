use teloxide::{prelude::*, types::ChatId};
use std::collections::HashSet;
use crate::context::AppContext;

pub async fn handle_start(bot: Bot, chat_id: ChatId) {
    let text = "🤖 <b>Kaspa Enterprise Intelligence</b>\n━━━━━━━━━━━━━━━━━━\nPowered by Google Gemini AI & Kaspa Node.\n\n🎙️ <b>Voice:</b> Send audio.\n🧠 <b>Chat:</b> Ask anything.\n⚡ <b>Track:</b> Paste a kaspa: address.";
    let _ = bot.send_message(chat_id, text).parse_mode(teloxide::types::ParseMode::Html).reply_markup(crate::kaspa_features::main_menu_markup()).await;
}

pub async fn handle_help(bot: Bot, chat_id: ChatId) {
    let text = "📚 <b>Guide</b>\nTalk naturally to me! Or use commands like /balance, /network, /miner.";
    let _ = bot.send_message(chat_id, text).parse_mode(teloxide::types::ParseMode::Html).await;
}

pub async fn handle_donate(bot: Bot, chat_id: ChatId) {
    let _ = bot.send_message(chat_id, "❤️ <b>Donate KAS:</b>\n<code>kaspa:qz0yqq8z3twwgg7lq2mjzg6w4edqys45w2wslz7tym2tc6s84580vvx9zr44g</code>").parse_mode(teloxide::types::ParseMode::Html).await;
}

pub async fn handle_add(bot: Bot, chat_id: ChatId, wallet: String, ctx: &AppContext) {
    let w = wallet.trim().to_string();
    if w.is_empty() || !w.starts_with("kaspa:") { return; }
    crate::state::add_wallet_to_db(&ctx.pool, &w, chat_id.0).await;
    ctx.state.entry(w.clone()).or_insert_with(HashSet::new).insert(chat_id.0);
    let _ = bot.send_message(chat_id, format!("✅ <b>Tracked:</b>\n<code>{}</code>", w)).parse_mode(teloxide::types::ParseMode::Html).await;
}

pub async fn handle_remove(bot: Bot, chat_id: ChatId, wallet: String, ctx: &AppContext) {
    let w = wallet.trim().to_string();
    crate::state::remove_wallet_from_db(&ctx.pool, &w, chat_id.0).await;
    if let Some(mut u) = ctx.state.get_mut(&w) { u.remove(&chat_id.0); }
    let _ = bot.send_message(chat_id, "🗑️ <b>Removed.</b>").parse_mode(teloxide::types::ParseMode::Html).await;
}

pub async fn handle_list(bot: Bot, chat_id: ChatId, ctx: &AppContext) {
    let mut tr = String::new();
    for e in ctx.state.iter().filter(|e| e.value().contains(&chat_id.0)) { tr.push_str(&format!("• <code>{}</code>\n", e.key())); }
    let text = if tr.is_empty() { "📂 <b>No wallets.</b>".to_string() } else { format!("📂 <b>Tracked:</b>\n{}", tr) };
    let _ = bot.send_message(chat_id, text).parse_mode(teloxide::types::ParseMode::Html).await;
}
