pub mod public;
pub mod node;
pub mod admin;

use chrono::Utc;
use std::collections::HashSet;
use teloxide::{prelude::*, types::ChatId};
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
        _ => { let _ = bot.send_message(chat_id, "Command routed via Gemini AI.").await; }
    }
    Ok(())
}

pub async fn handle_text_router(bot: Bot, msg: Message, ctx: AppContext) -> anyhow::Result<()> {
    let raw_text = msg.text().unwrap_or("").trim();
    let user_id = msg.from.as_ref().map(|u| u.id.0 as i64).unwrap_or(0);
    crate::ai::process_conversational_intent(bot, msg.chat.id, msg.id, user_id, raw_text.to_string(), ctx).await
}
