pub mod public;
pub mod node;
pub mod admin;

use chrono::Utc;
use teloxide::{prelude::*, types::CallbackQuery};
use crate::commands::Command;
use crate::context::AppContext;

pub async fn handle_command(bot: Bot, msg: Message, cmd: Command, ctx: AppContext) -> anyhow::Result<()> {
    let chat_id = msg.chat.id;
    let user_id = msg.from.as_ref().map(|u| u.id.0 as i64).unwrap_or(0);
    let time = Utc::now().format("%Y-%m-%d %H:%M:%S UTC").to_string();

    match cmd {
        Command::Start => public::handle_start(bot, chat_id).await,
        Command::Help => public::handle_help(bot, chat_id).await,
        Command::Donate => public::handle_donate(bot, chat_id).await,
        Command::Add(w) => public::handle_add(bot, chat_id, w, &ctx).await,
        Command::Remove(w) => public::handle_remove(bot, chat_id, w, &ctx).await,
        Command::List => public::handle_list(bot, chat_id, &ctx).await,
        Command::Balance => node::handle_balance(bot, chat_id, &ctx, time, None).await,
        Command::Sys => admin::handle_sys(bot, chat_id, user_id, &ctx, None, time).await,
        _ => {
            let user_text = msg.text().unwrap_or_default().to_string();
            crate::ai::process_conversational_intent(bot, chat_id, msg.id, user_text, ctx).await?;
        }
    }
    Ok(())
}

pub async fn handle_text_router(bot: Bot, msg: Message, ctx: AppContext) -> anyhow::Result<()> {
    let user_text = msg.text().unwrap_or_default().trim();
    let chat_id = msg.chat.id;

    // 1. AUTO-DETECTION: Is this a Kaspa Address?
    if user_text.starts_with("kaspa:") && user_text.len() > 60 {
        public::handle_add(bot, chat_id, user_text.to_string(), &ctx).await; return Ok(());
    }

    // 2. Otherwise, treat as Gemini Chat
    crate::ai::process_conversational_intent(bot, chat_id, msg.id, user_text.to_string(), ctx).await
}

#[allow(dead_code)]
pub async fn handle_callback(bot: Bot, q: CallbackQuery, _ctx: AppContext) -> anyhow::Result<()> {
    let _ = bot.answer_callback_query(q.id).await;
    Ok(())
}

