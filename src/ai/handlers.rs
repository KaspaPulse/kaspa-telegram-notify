use super::context::inject_live_wallet_context;
use crate::context::AppContext;
use teloxide::net::Download;
use teloxide::prelude::*;

pub async fn process_conversational_intent(
    bot: Bot,
    chat_id: teloxide::types::ChatId,
    msg_id: teloxide::types::MessageId,
    _user_id: i64,
    user_text: String,
    ctx: AppContext,
) -> anyhow::Result<()> {
    tracing::info!("🗣️ [USER ASKED]: {}", user_text);

    let initial_msg = bot
        .send_message(chat_id, "⏳ <b>Kaspa AI:</b> Analyzing... (Universal API)")
        .reply_parameters(teloxide::types::ReplyParameters::new(msg_id))
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;

    let engine_arc = ctx.ai_engine.clone();

    let _ = sqlx::query("CREATE TABLE IF NOT EXISTS chat_history (id INTEGER PRIMARY KEY AUTOINCREMENT, chat_id INTEGER, role TEXT, message TEXT, timestamp DATETIME DEFAULT CURRENT_TIMESTAMP)").execute(&ctx.pool).await;
    let records: Result<Vec<(String, String)>, _> = sqlx::query_as("SELECT role, message FROM (SELECT role, message, timestamp FROM chat_history WHERE chat_id = ?1 ORDER BY id DESC LIMIT 6) ORDER BY id ASC").bind(chat_id.0).fetch_all(&ctx.pool).await;

    let mut history_str = String::new();
    if let Ok(rows) = records {
        for (role, msg) in rows {
            history_str.push_str(&format!("{}: {}\n", role.to_uppercase(), msg));
        }
    }

    let enriched_prompt = if history_str.is_empty() {
        user_text.clone()
    } else {
        format!(
            "[CONVERSATION HISTORY]\n{}\n\n[CURRENT QUESTION]\n{}",
            history_str, user_text
        )
    };

    let live_ctx_data = inject_live_wallet_context(chat_id.0, &ctx).await;
    let user_text_for_db = user_text.clone();

    let engine = engine_arc.lock().await;
    let response = match engine
        .generate(&ctx.pool, &enriched_prompt, &live_ctx_data, None)
        .await
    {
        Ok(text) => {
            tracing::info!("🧠 [AI REPLIED]: {}", text);
            let _ = sqlx::query(
                "INSERT INTO chat_history (chat_id, role, message) VALUES (?1, 'user', ?2)",
            )
            .bind(chat_id.0)
            .bind(&user_text_for_db)
            .execute(&ctx.pool)
            .await;
            let _ = sqlx::query(
                "INSERT INTO chat_history (chat_id, role, message) VALUES (?1, 'assistant', ?2)",
            )
            .bind(chat_id.0)
            .bind(&text)
            .execute(&ctx.pool)
            .await;
            format!("🤖 <b>Kaspa AI:</b>\n{}", text)
        }
        Err(e) => {
            tracing::error!("⚠️ [AI ERROR]: {}", e);
            format!("⚠️ <b>AI Error:</b>\n{}", e)
        }
    };

    let _ = bot
        .edit_message_text(chat_id, initial_msg.id, response)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await;
    Ok(())
}

pub async fn process_voice_message(bot: Bot, msg: Message, ctx: AppContext) -> anyhow::Result<()> {
    let chat_id = msg.chat.id;
    let voice = match msg.voice() {
        Some(v) => v,
        None => return Ok(()),
    };

    tracing::info!("🎙️ [USER SENT VOICE MESSAGE]");

    let initial_msg = bot
        .send_message(chat_id, "⏳ <b>Kaspa AI:</b> Processing Audio...")
        .reply_parameters(teloxide::types::ReplyParameters::new(msg.id))
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;

    let file = bot.get_file(voice.file.id.clone()).await?;
    let mut audio_bytes = Vec::new();
    bot.download_file(&file.path, &mut audio_bytes).await?;

    let live_ctx_data = inject_live_wallet_context(chat_id.0, &ctx).await;
    let engine = ctx.ai_engine.lock().await;

    let response = match engine
        .generate(
            &ctx.pool,
            "Answer any questions asked in this audio transcript contextually.",
            &live_ctx_data,
            Some(audio_bytes),
        )
        .await
    {
        Ok(reply) => {
            tracing::info!("🧠 [AI REPLIED TO VOICE]: {}", reply);
            format!(
                "🎙️ <b>Voice Analysis Complete</b>\n\n🤖 <b>Kaspa AI:</b>\n{}",
                reply
            )
        }
        Err(e) => {
            tracing::error!("⚠️ [VOICE ERROR]: {}", e);
            format!("⚠️ <b>Voice Error:</b>\n{}", e)
        }
    };

    let _ = bot
        .edit_message_text(chat_id, initial_msg.id, response)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await;
    Ok(())
}
