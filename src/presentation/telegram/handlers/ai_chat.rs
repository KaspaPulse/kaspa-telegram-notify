use crate::ai::ai_use_cases::{AiChatUseCase, AiRagUseCase};
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
            if cid == app_context.admin_id { // Enterprise Auth: Trust Telegram Secure Session ID
                if pending_cmd.starts_with("TOGGLE:") {
                    let flag = pending_cmd.split(':').nth(1).unwrap_or("").to_string();
                    let _ = crate::presentation::telegram::handlers::admin::handle_toggle(bot, msg, flag, app_context).await;
                } else {
                    match pending_cmd.as_str() {
                        "PAUSE" => { let _ = crate::presentation::telegram::handlers::admin::handle_pause(bot, msg, app_context).await; }
                        "RESUME" => { let _ = crate::presentation::telegram::handlers::admin::handle_resume(bot, msg, app_context).await; }
                        "RESTART" => { let _ = crate::presentation::telegram::handlers::admin::handle_restart(bot, msg).await; }
                        _ => {}
                    }
                }
            } else {
                let _ = bot.send_message(msg.chat.id, "⛔ <b>Security Alert:</b> Invalid Admin PIN. Session terminated.")
                    .parse_mode(teloxide::types::ParseMode::Html).await;
            }
        }
        return Ok(());
    }

    if let Some(text) = msg.text() {
        if text.starts_with('/') {
            return Ok(());
        }

        let clean_text = text.trim();
        
        // --- ENTERPRISE AI MODEL SWITCHER ---
        if clean_text.starts_with("/setmodel ") {
            if cid != app_context.admin_id {
                crate::send_logged!(bot, msg.clone(), "⛔ <b>Access Denied:</b> Admin only.");
                return Ok(());
            }
            let new_model = clean_text.replace("/setmodel ", "").trim().to_string();
            let _ = sqlx::query("INSERT INTO system_settings (key_name, value_data) VALUES ($1, $2) ON CONFLICT (key_name) DO UPDATE SET value_data = EXCLUDED.value_data, updated_at = CURRENT_TIMESTAMP")
                .bind("active_ai_model")
                .bind(&new_model)
                .execute(&app_context.pool)
                .await;
            crate::send_logged!(bot, msg.clone(), format!("✅ <b>AI Engine Switched!</b>\nAll future queries will route to: <code>{}</code>", new_model));
            return Ok(());
        }
        // ------------------------------------
        if clean_text.starts_with("kaspa:") {
            let db_instance = crate::infrastructure::database::postgres_adapter::PostgresRepository::new(app_context.pool.clone());
            let all_wallets = db_instance.get_all_tracked_wallets().await.unwrap_or_default();
            let is_tracked = all_wallets.iter().any(|w| w.address == clean_text && w.chat_id == cid);

            if is_tracked {
                crate::send_logged!(bot, msg.clone(), format!("ℹ️ <b>Wallet is already tracked!</b>\n<code>{}</code>", clean_text));
            } else {
                let wallet_mgt = crate::wallet::wallet_use_cases::WalletManagementUseCase::new(std::sync::Arc::new(db_instance));
                match wallet_mgt.add_wallet(clean_text, cid).await {
                    Ok(_) => {
                        crate::send_logged!(bot, msg.clone(), format!("✅ <b>Wallet Auto-Added!</b>\nNow tracking: <code>{}</code>", clean_text));
                    }
                    Err(e) => {
                        crate::send_logged!(bot, msg.clone(), format!("❌ <b>Error:</b> {}", e));
                    }
                }
            }
            return Ok(()); 
        }
    }

    if !app_context.ai_chat_enabled.load(Ordering::Relaxed) {
        crate::send_logged!(bot, msg, "🤖 <i>AI Assistant is currently offline for maintenance.</i>");
        return Ok(());
    }

        let persona = "[SYSTEM ENFORCEMENT]: You are Kaspa Pulse Enterprise. Reply ONLY in English or Standard Arabic. Use flawless grammar and clean HTML formatting. Be highly professional.";

    if let Some(text) = msg.text() {
        if !crate::infrastructure::security::ai_firewall::AiFirewall::validate_prompt(text) {
            crate::send_logged!(bot, msg, "🛡️ <b>Enterprise AI Firewall:</b> Malicious prompt injection detected and neutralized.");
            return Ok(());
        }

        let loading_msg = bot.send_message(msg.chat.id, "🤖 <i>Analyzing query...</i>")
            .parse_mode(teloxide::types::ParseMode::Html)
            .reply_parameters(teloxide::types::ReplyParameters::new(msg.id))
            .await?;

        // 1. Fetch from RSS / Database
        let mut context = ai_rag.build_context_for_query(text).await.unwrap_or_default();
        if !context.is_empty() {
            context = format!("[📰 INTERNAL DATABASE (RSS)]:\n{}\n", context);
        }
        
        let safe_text = crate::infrastructure::security::ai_firewall::AiFirewall::sanitize_prompt(text);
        let query_lower = safe_text.to_lowercase();
        
        // 2. Active Intent Routing for Tavily Live Web Search
        let needs_live_data = query_lower.contains("news") || query_lower.contains("price") 
            || query_lower.contains("update") || query_lower.contains("today") || query_lower.contains("now")
            || text.contains("اخبار") || text.contains("أخبار") || text.contains("سعر") 
            || text.contains("تحديث") || text.contains("اليوم") || text.contains("الان") || text.contains("جديد");

        if needs_live_data {
            let _ = bot.edit_message_text(msg.chat.id, loading_msg.id, "🌐 <i>Fetching live data from Tavily...</i>").parse_mode(teloxide::types::ParseMode::Html).await;
            
            let tavily_key = std::env::var("TAVILY_API_KEY").unwrap_or_default();
            if !tavily_key.is_empty() {
                let client = reqwest::Client::new();
                let body = serde_json::json!({
                    "api_key": tavily_key,
                    "query": format!("Kaspa crypto {}", safe_text),
                    "search_depth": "basic",
                    "include_answer": true
                });
                
                if let Ok(res) = client.post("https://api.tavily.com/search").json(&body).send().await {
                    if let Ok(json) = res.json::<serde_json::Value>().await {
                        if let Some(answer) = json.get("answer").and_then(|a| a.as_str()) {
                            context = format!("{}\n[📡 LIVE WEB SEARCH RESULTS (TAVILY)]:\n{}\n", context, answer);
                        }
                    }
                }
            }
        }

        let _ = bot.edit_message_text(msg.chat.id, loading_msg.id, "🤖 <i>Processing intelligence...</i>").parse_mode(teloxide::types::ParseMode::Html).await;

        let enriched_prompt = if context.trim().is_empty() {
            format!("{}\n\nUser: {}", persona, safe_text)
        } else {
            format!("{}\n\n{}\nUser: {}", persona, context, safe_text)
        };

        match ai_chat.execute_text(&enriched_prompt).await {
            Ok(reply) => { crate::utils::log_multiline("🤖 [FULL AI RESPONSE]:", &reply, false); let _ = bot.edit_message_text(msg.chat.id, loading_msg.id, reply).await;
                let markup = teloxide::types::InlineKeyboardMarkup::new(vec![vec![teloxide::types::InlineKeyboardButton::callback("🔄 Regenerate", "regenerate_ai")]]);
                let _ = bot.edit_message_reply_markup(msg.chat.id, loading_msg.id).reply_markup(markup).await; }
            Err(e) => { let _ = bot.edit_message_text(msg.chat.id, loading_msg.id, format!("❌ AI Error: {}", e)).await; }
        }
        return Ok(());
    }

    if let Some(voice) = msg.voice() {
        if !app_context.ai_voice_enabled.load(Ordering::Relaxed) {
            crate::send_logged!(bot, msg, "🎙️ <i>Voice processing is currently disabled.</i>");
            return Ok(());
        }
        let loading_msg = bot.send_message(msg.chat.id, "🎙️ <i>Listening & Transcribing...</i>")
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
                            let _ = bot.edit_message_text(msg.chat.id, loading_msg.id, format!("🗣️ <b>You:</b> <i>{}</i>\n\n🤖 <i>Thinking...</i>", transcription))
                                .parse_mode(teloxide::types::ParseMode::Html).await;

                            if !crate::infrastructure::security::ai_firewall::AiFirewall::validate_prompt(&transcription) {
                                let _ = bot.edit_message_text(msg.chat.id, loading_msg.id, "🛡️ <b>Firewall:</b> Malicious audio payload neutralized.").parse_mode(teloxide::types::ParseMode::Html).await;
                                return Ok(());
                            }
                            let safe_voice_text = crate::infrastructure::security::ai_firewall::AiFirewall::sanitize_prompt(&transcription);
                            let context = ai_rag.build_context_for_query(&transcription).await.unwrap_or_default();
                            
                            let enriched_prompt = if context.trim().is_empty() {
                                format!("{}\n\nUser: {}", persona, safe_voice_text)
                            } else {
                                format!("{}\n\n[KNOWLEDGE BASE]:\n{}\n\nUser: {}", persona, context, safe_voice_text)
                            };

                            match ai_chat.execute_text(&enriched_prompt).await {
                                Ok(reply) => { crate::utils::log_multiline("🤖 [FULL AI RESPONSE]:", &reply, false); let _ = bot.edit_message_text(msg.chat.id, loading_msg.id, format!("🗣️ <b>You:</b> <i>{}</i>\n\n🤖 {}", transcription, reply)).parse_mode(teloxide::types::ParseMode::Html).await;
                                let markup = teloxide::types::InlineKeyboardMarkup::new(vec![vec![teloxide::types::InlineKeyboardButton::callback("🔄 Regenerate", "regenerate_ai")]]);
                                let _ = bot.edit_message_reply_markup(msg.chat.id, loading_msg.id).reply_markup(markup).await; }
                                Err(e) => { let _ = bot.edit_message_text(msg.chat.id, loading_msg.id, format!("❌ AI Error: {}", e)).await; }
                            }
                        }
                        Err(e) => { let _ = bot.edit_message_text(msg.chat.id, loading_msg.id, format!("❌ Voice Parsing Error: {}", e)).await; }
                    }
                } else { let _ = bot.edit_message_text(msg.chat.id, loading_msg.id, "❌ Error downloading audio.").await; }
            }
        }
    }
    Ok(())
}









