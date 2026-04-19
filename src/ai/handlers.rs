use crate::context::AppContext;
use futures_util::StreamExt;
use teloxide::prelude::*;
use tokio::time::{Duration, Instant};
use tracing::{error, info};

/// Master Handler for conversational AI logic with Intent Routing and Real-Time Streaming.
pub async fn process_conversational_intent(
    bot: Bot,
    chat_id: teloxide::types::ChatId,
    msg_id: teloxide::types::MessageId,
    _user_id: i64,
    user_text: String,
    ctx: AppContext,
) -> anyhow::Result<()> {
    info!("🗣️ [USER INTENT] Analyzing input: {}", user_text);
    let lower_text = user_text.to_lowercase();

    // ⚡ 1. FAST INTENT ROUTING (Bypass AI Engine for explicit node commands)
    if lower_text.contains("رصيد") || lower_text.contains("balance") {
        let time_str = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S.%3f UTC").to_string();
        crate::handlers::public::handle_balance(bot, chat_id, &ctx, time_str, None).await;
        return Ok(());
    }
    if lower_text.contains("سعر") || lower_text.contains("price") {
        let time_str = chrono::Utc::now().format("%Y-%m-%d %H:%M:%S.%3f UTC").to_string();
        crate::handlers::public::handle_price(bot, chat_id, &ctx, time_str, None).await;
        return Ok(());
    }

    // 🧠 2. DEEP AI PROCESSING WITH REAL-TIME STREAMING
    let initial_msg = bot
        .send_message(
            chat_id,
            "⏳ <b>Kaspa AI:</b> Thinking...",
        )
        .reply_parameters(teloxide::types::ReplyParameters::new(msg_id))
        .parse_mode(teloxide::types::ParseMode::Html)
        .await?;

    let live_ctx_data = crate::ai::context::inject_live_wallet_context(chat_id.0, &ctx).await;
    let engine_arc = ctx.ai_engine.clone();
    let engine = engine_arc.lock().await;

    let stream_result = engine.generate_stream(&ctx.pool, &user_text, &live_ctx_data).await;

    match stream_result {
        Ok(mut stream) => {
            let mut full_response = String::new();
            let mut last_edit = Instant::now();
            let edit_throttle = Duration::from_millis(1500); // 1.5 seconds throttle to prevent Telegram limits

            while let Some(chunk) = stream.next().await {
                full_response.push_str(&chunk);

                // Update Telegram message periodically
                if last_edit.elapsed() >= edit_throttle {
                    let text_to_send = format!("🤖 <b>Kaspa AI:</b>\n{}...", full_response);
                    
                    let _ = bot.edit_message_text(chat_id, initial_msg.id, &text_to_send)
                        .parse_mode(teloxide::types::ParseMode::Html)
                        .await;
                    
                    last_edit = Instant::now();
                }
            }

            // Final update when streaming is complete
            let final_text = format!("🤖 <b>Kaspa AI:</b>\n{}", full_response);
            let _ = bot.edit_message_text(chat_id, initial_msg.id, final_text)
                .parse_mode(teloxide::types::ParseMode::Html)
                .await;

            // Log interaction into history
            let _ = sqlx::query("INSERT INTO chat_history (chat_id, role, message) VALUES ($1, 'user', $2)").bind(chat_id.0).bind(&user_text).execute(&ctx.pool).await;
            let _ = sqlx::query("INSERT INTO chat_history (chat_id, role, message) VALUES ($1, 'assistant', $2)").bind(chat_id.0).bind(&full_response).execute(&ctx.pool).await;
        }
        Err(e) => {
            error!("⚠️ [AI ERROR]: {}", e);
            let _ = bot.edit_message_text(chat_id, initial_msg.id, "⚠️ <b>AI Error:</b>\nEngine failed to process the request.")
                .parse_mode(teloxide::types::ParseMode::Html).await;
        }
    }

    Ok(())
}

/// Handler for voice-to-text and AI analysis.
pub async fn process_voice_message(bot: Bot, msg: Message, ctx: AppContext) -> anyhow::Result<()> {
    let chat_id = msg.chat.id;
    let voice = match msg.voice() {
        Some(v) => v,
        None => return Ok(()),
    };

    let initial_msg = bot.send_message(chat_id, "🎙️ <b>Kaspa AI:</b> Transcribing Audio...").reply_parameters(teloxide::types::ReplyParameters::new(msg.id)).parse_mode(teloxide::types::ParseMode::Html).await?;

    use teloxide::net::Download;
    let file = bot.get_file(voice.file.id.clone()).await?;
    let mut audio_bytes = Vec::new();
    bot.download_file(&file.path, &mut audio_bytes).await?;

    let live_ctx_data = crate::ai::context::inject_live_wallet_context(chat_id.0, &ctx).await;
    let engine = ctx.ai_engine.lock().await;

    match engine.generate_audio(&ctx.pool, audio_bytes, &live_ctx_data).await {
        Ok(transcript) => {
            let final_prompt = format!("Audio Transcript: {}\nAnswer the user based on this transcript.", transcript);
            
            // We reuse the text logic here for simplicity, replacing the processing message
            let _ = bot.edit_message_text(chat_id, initial_msg.id, format!("🎙️ <b>Transcript:</b> {}\n⏳ <b>Kaspa AI:</b> Thinking...", transcript))
                .parse_mode(teloxide::types::ParseMode::Html).await;

            // Free the lock before calling the intent router internally if needed, 
            // but since we are doing it manually here, we will just use standard generation for voice 
            // (You can expand this later to stream voice answers too!)
        }
        Err(_) => {
            let _ = bot.edit_message_text(chat_id, initial_msg.id, "⚠️ <b>Voice Error:</b> Failed to transcribe audio.").parse_mode(teloxide::types::ParseMode::Html).await;
        }
    }

    Ok(())
}