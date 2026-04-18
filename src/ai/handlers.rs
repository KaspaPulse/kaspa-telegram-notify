use super::context::inject_live_wallet_context;
use crate::context::AppContext;
use teloxide::net::Download;
use teloxide::prelude::*;
use tracing::{info, error};

/// Master Handler for conversational AI logic.
/// Forces the AI to use local RAG context and live intelligence.
pub async fn process_conversational_intent(
    bot: Bot,
    chat_id: teloxide::types::ChatId,
    msg_id: teloxide::types::MessageId,
    _user_id: i64,
    user_text: String,
    ctx: AppContext,
) -> anyhow::Result<()> {
    info!("🗣️ [USER ASKED]: {}", user_text);

    // Initial response to show the bot is working
    let initial_msg = bot
        .send_message(chat_id, "⏳ <b>Kaspa AI:</b> Syncing Intelligence... (Sovereign Engine)")
        .reply_parameters(teloxide::types::ReplyParameters::new(msg_id))
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;

    let engine_arc = ctx.ai_engine.clone();

    // Ensure chat history table exists for persistent memory
    let _ = sqlx::query(
        "CREATE TABLE IF NOT EXISTS chat_history (
            id SERIAL PRIMARY KEY, 
            chat_id BIGINT, 
            role TEXT, 
            message TEXT, 
            timestamp TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
        )"
    ).execute(&ctx.pool).await;
    
    // Retrieve the last 6 messages for short-term memory
    let records: Result<Vec<(String, String)>, _> = sqlx::query_as(
        "SELECT role, message FROM (
            SELECT id, role, message, timestamp FROM chat_history 
            WHERE chat_id = $1 ORDER BY id DESC LIMIT 6
         ) AS sub ORDER BY id ASC"
    ).bind(chat_id.0).fetch_all(&ctx.pool).await;

    let mut history_str = String::new();
    if let Ok(rows) = records {
        for (role, msg) in rows {
            history_str.push_str(&format!("{}: {}\n", role.to_uppercase(), msg));
        }
    }

    // 🧠 MASTER SYSTEM PROMPT: Forces the AI to prioritize RAG context.
    // This solves the "I don't have information" failure.
    let system_instruction = "SYSTEM INSTRUCTION: \
        You are the 'Kaspa Sovereign Intelligence', an expert in blockchain and server infrastructure. \
        1. PRIORITIZE the [INTERNAL KNOWLEDGE] and [LIVE DATA] sections above everything else. \
        2. NEVER say 'I don't have information' if data about SSL, Rust, or Kaspa is present in the context. \
        3. Explain technical settings (Nginx, Cloudflare, sync.rs) as the absolute owner of this system. \
        4. Use a professional, sharp, and authoritative tone.";

    // Construct the final prompt with history and command
    let enriched_prompt = format!(
        "{}\n\n[CONVERSATION HISTORY]\n{}\n\n[CURRENT QUESTION]\n{}",
        system_instruction, history_str, user_text
    );

    // Fetch live wallet and network data (Difficulty, Balance, DAA)
    let live_ctx_data = inject_live_wallet_context(chat_id.0, &ctx).await;
    let user_text_for_db = user_text.clone();

    let engine = engine_arc.lock().await;
    let response = match engine
        .generate(&ctx.pool, &enriched_prompt, &live_ctx_data, None)
        .await
    {
        Ok(text) => {
            info!("🧠 [AI REPLIED]: {}", text);
            
            // Log interaction into history
            let _ = sqlx::query("INSERT INTO chat_history (chat_id, role, message) VALUES ($1, 'user', $2)").bind(chat_id.0).bind(&user_text_for_db).execute(&ctx.pool).await;
            let _ = sqlx::query("INSERT INTO chat_history (chat_id, role, message) VALUES ($1, 'assistant', $2)").bind(chat_id.0).bind(&text).execute(&ctx.pool).await;
            
            format!("🤖 <b>Kaspa AI:</b>\n{}", text)
        }
        Err(e) => {
            error!("⚠️ [AI ERROR]: {}", e);
            format!("⚠️ <b>AI Error:</b>\nAn error occurred while processing your request. Please check the logs.")
        }
    };

    // Edit the initial message with the final AI response
    let _ = bot
        .edit_message_text(chat_id, initial_msg.id, response)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await;

    Ok(())
}

/// Handler for voice-to-text and AI analysis.
pub async fn process_voice_message(bot: Bot, msg: Message, ctx: AppContext) -> anyhow::Result<()> {
    let chat_id = msg.chat.id;
    let voice = match msg.voice() {
        Some(v) => v,
        None => return Ok(()),
    };

    info!("🎙️ [USER SENT VOICE MESSAGE]");

    let initial_msg = bot
        .send_message(chat_id, "🎙️ <b>Kaspa AI:</b> Analyzing Voice... (Whisper Engine)")
        .reply_parameters(teloxide::types::ReplyParameters::new(msg.id))
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;

    // Download voice file
    let file = bot.get_file(voice.file.id.clone()).await?;
    let mut audio_bytes = Vec::new();
    bot.download_file(&file.path, &mut audio_bytes).await?;

    let live_ctx_data = inject_live_wallet_context(chat_id.0, &ctx).await;
    let engine = ctx.ai_engine.lock().await;

    // Direct instructions for voice processing
    let voice_instruction = "SYSTEM: Transcribe and answer the query in this audio. Use local server context for accuracy.";

    let response = match engine
        .generate(
            &ctx.pool,
            voice_instruction,
            &live_ctx_data,
            Some(audio_bytes),
        )
        .await
    {
        Ok(reply) => {
            info!("🧠 [AI REPLIED TO VOICE]: {}", reply);
            format!("🎙️ <b>Voice Analysis Complete</b>\n\n🤖 <b>Kaspa AI:</b>\n{}", reply)
        }
        Err(e) => {
            error!("⚠️ [VOICE ERROR]: {}", e);
            format!("⚠️ <b>Voice Error:</b>\nFailed to process audio message.")
        }
    };

    let _ = bot
        .edit_message_text(chat_id, initial_msg.id, response)
        .parse_mode(teloxide::types::ParseMode::Html)
        .await;

    Ok(())
}