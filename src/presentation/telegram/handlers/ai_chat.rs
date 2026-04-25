use crate::ai::ai_use_cases::AiChatUseCase;
use crate::ai::ai_use_cases::AiRagUseCase;
use crate::domain::models::AppContext;
use crate::infrastructure::ai::ai_engine_adapter::AiEngineAdapter;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use teloxide::net::Download;
use teloxide::prelude::*;

pub async fn handle_raw_message(
    bot: Bot,
    msg: Message,
    ai_chat: Arc<AiChatUseCase>,
    ai_rag: Arc<AiRagUseCase>,
    ai_provider: Arc<AiEngineAdapter>,
    app_context: Arc<AppContext>,
) -> anyhow::Result<()> {
    let cid = msg.chat.id.0;
    if app_context.maintenance_mode.load(Ordering::Relaxed) && cid != app_context.admin_id {
        return Ok(());
    }

    if let Some((_, pending_cmd)) = app_context.admin_sessions.remove(&cid) {
        let _ = bot.delete_message(msg.chat.id, msg.id).await;
        if let Some(text) = msg.text() {
            if crate::infrastructure::security::utils::verify_admin_pin(text) {
                if pending_cmd.starts_with("TOGGLE:") {
                    let flag = pending_cmd.split(':').nth(1).unwrap_or("").to_string();
                    let _ = crate::presentation::telegram::handlers::admin::handle_toggle(
                        bot,
                        msg,
                        flag,
                        app_context,
                    )
                    .await;
                } else {
                    match pending_cmd.as_str() {
                        "PAUSE" => {
                            let _ = crate::presentation::telegram::handlers::admin::handle_pause(
                                bot,
                                msg,
                                app_context,
                            )
                            .await;
                        }
                        "RESUME" => {
                            let _ = crate::presentation::telegram::handlers::admin::handle_resume(
                                bot,
                                msg,
                                app_context,
                            )
                            .await;
                        }
                        "RESTART" => {
                            let _ = crate::presentation::telegram::handlers::admin::handle_restart(
                                bot, msg,
                            )
                            .await;
                        }
                        _ => {}
                    }
                }
            } else {
                let _ = bot
                    .send_message(
                        msg.chat.id,
                        "⛔ <b>Security Alert:</b> Invalid Admin PIN. Session terminated.",
                    )
                    .parse_mode(teloxide::types::ParseMode::Html)
                    .await;
            }
        }
        return Ok(());
    }
    if !app_context.ai_chat_enabled.load(Ordering::Relaxed) {
        crate::send_logged!(
            bot,
            msg,
            "🤖 <i>AI Assistant is currently offline for maintenance.</i>"
        );
        return Ok(());
    }

    // 📝 TEXT PROCESSING
    if let Some(text) = msg.text() {
        if text.starts_with('/') {
            return Ok(());
        }

        // 🛡️ SECURITY LAYER 1: Prompt Validation (Blocklist)
        if !crate::infrastructure::security::ai_firewall::AiFirewall::validate_prompt(text) {
            crate::send_logged!(bot, msg, "🛡️ <b>Enterprise AI Firewall:</b> Malicious prompt injection detected and neutralized.");
            return Ok(());
        }

        let loading_msg = bot
            .send_message(msg.chat.id, "🤖 <i>Thinking... (Secured RAG Active)</i>")
            .parse_mode(teloxide::types::ParseMode::Html)
            .reply_parameters(teloxide::types::ReplyParameters::new(msg.id))
            .await?;

        // 🧠 KNOWLEDGE LAYER: Fetch RAG Context
        let context = ai_rag
            .build_context_for_query(text)
            .await
            .unwrap_or_default();

        // 🛡️ SECURITY LAYER 2: Structural Sanitization
        let safe_text =
            crate::infrastructure::security::ai_firewall::AiFirewall::sanitize_prompt(text);

        let enriched_prompt = if context.is_empty() {
            safe_text
        } else {
            format!(
                "System: Answer the user using this verified Kaspa context:\n{}\n\nUser: {}",
                context, safe_text
            )
        };

        match ai_chat.execute_text(&enriched_prompt).await {
            Ok(reply) => {
                let _ = bot
                    .edit_message_text(msg.chat.id, loading_msg.id, reply)
                    .await;
            }
            Err(e) => {
                let _ = bot
                    .edit_message_text(msg.chat.id, loading_msg.id, format!("❌ AI Error: {}", e))
                    .await;
            }
        }
        return Ok(());
    }

    // 🎙️ VOICE PROCESSING
    if let Some(voice) = msg.voice() {
        if !app_context.ai_voice_enabled.load(Ordering::Relaxed) {
            crate::send_logged!(
                bot,
                msg,
                "🎙️ <i>Voice processing is currently disabled.</i>"
            );
            return Ok(());
        }
        let loading_msg = bot
            .send_message(msg.chat.id, "🎙️ <i>Listening & Transcribing...</i>")
            .parse_mode(teloxide::types::ParseMode::Html)
            .reply_parameters(teloxide::types::ReplyParameters::new(msg.id))
            .await?;

        if let Ok(file) = bot.get_file(&voice.file.id).await {
            let temp_path = format!("temp_voice_{}.ogg", msg.id.0);
            if let Ok(mut dst) = tokio::fs::File::create(&temp_path).await {
                if bot.download_file(&file.path, &mut dst).await.is_ok() {
                    let mut buffer = Vec::new();
                    if let Ok(mut src) = tokio::fs::File::open(&temp_path).await {
                        use tokio::io::AsyncReadExt;
                        let _ = src.read_to_end(&mut buffer).await;
                    }
                    let _ = tokio::fs::remove_file(&temp_path).await;

                    match ai_provider.process_voice(buffer, "").await {
                        Ok(transcription) => {
                            let _ = bot
                                .edit_message_text(
                                    msg.chat.id,
                                    loading_msg.id,
                                    format!(
                                        "🗣️ <b>You:</b> <i>{}</i>\n\n🤖 <i>Thinking...</i>",
                                        transcription
                                    ),
                                )
                                .parse_mode(teloxide::types::ParseMode::Html)
                                .await;

                            // 🛡️ SECURITY & RAG FOR VOICE
                            if !crate::infrastructure::security::ai_firewall::AiFirewall::validate_prompt(&transcription) {
                                let _ = bot.edit_message_text(msg.chat.id, loading_msg.id, "🛡️ <b>Firewall:</b> Malicious audio payload neutralized.").parse_mode(teloxide::types::ParseMode::Html).await;
                                return Ok(());
                            }
                            let safe_voice_text = crate::infrastructure::security::ai_firewall::AiFirewall::sanitize_prompt(&transcription);
                            let context = ai_rag
                                .build_context_for_query(&transcription)
                                .await
                                .unwrap_or_default();
                            let enriched_prompt = if context.is_empty() {
                                safe_voice_text
                            } else {
                                format!(
                                    "System: Answer using this context:\n{}\n\nUser: {}",
                                    context, safe_voice_text
                                )
                            };

                            match ai_chat.execute_text(&enriched_prompt).await {
                                Ok(reply) => {
                                    let _ = bot
                                        .edit_message_text(
                                            msg.chat.id,
                                            loading_msg.id,
                                            format!(
                                                "🗣️ <b>You:</b> <i>{}</i>\n\n🤖 {}",
                                                transcription, reply
                                            ),
                                        )
                                        .parse_mode(teloxide::types::ParseMode::Html)
                                        .await;
                                }
                                Err(e) => {
                                    let _ = bot
                                        .edit_message_text(
                                            msg.chat.id,
                                            loading_msg.id,
                                            format!("❌ AI Error: {}", e),
                                        )
                                        .await;
                                }
                            }
                        }
                        Err(e) => {
                            let _ = bot
                                .edit_message_text(
                                    msg.chat.id,
                                    loading_msg.id,
                                    format!("❌ Voice Parsing Error: {}", e),
                                )
                                .await;
                        }
                    }
                } else {
                    let _ = bot
                        .edit_message_text(
                            msg.chat.id,
                            loading_msg.id,
                            "❌ Error downloading audio.",
                        )
                        .await;
                }
            }
        }
    }
    Ok(())
}
