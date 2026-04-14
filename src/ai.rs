#![allow(dead_code)]
use crate::context::AppContext;
use base64::{engine::general_purpose, Engine as _};
use kaspa_addresses::Address;
use kaspa_rpc_core::api::rpc::RpcApi;
use reqwest::Client;
use serde_json::json;
use std::sync::Arc;
use teloxide::net::Download;
use teloxide::prelude::*;
use tokio::sync::Mutex;

pub struct LocalAiEngine {
    pub client: Client,
    pub api_key: String,
}

impl LocalAiEngine {
    pub fn new() -> anyhow::Result<Self> {
        tracing::info!("[AI ENGINE] Initializing Cloud Gemini 2.5 Flash Engine with Backoff...");
        let api_key = std::env::var("GEMINI_API_KEY").unwrap_or_default();
        Ok(Self {
            client: Client::new(),
            api_key,
        })
    }

    pub async fn generate(
        &self,
        prompt: &str,
        live_context: &str,
        audio_bytes: Option<Vec<u8>>,
    ) -> anyhow::Result<String> {
        let rag_context = crate::rag::search_kaspa_docs(prompt).await;

        let system_message = format!(
            "You are an uncompromisingly accurate Kaspa AI Assistant.
[ABSOLUTE RULES]
1. DO NOT invent, hallucinate, or assume facts.
2. Kaspa is a PoW BlockDAG. NO CEO, NO PoS, NO Smart Contracts.
3. Keep answers highly professional, factual, and brief.

[LIVE DATA]
{}

[KNOWLEDGE BASE]
{}",
            live_context, rag_context
        );

        let url = format!(
            "https://generativelanguage.googleapis.com/v1beta/models/gemini-2.5-flash:generateContent?key={}",
            self.api_key
        );

        let mut parts = vec![json!({"text": prompt})];

        if let Some(bytes) = audio_bytes {
            let b64 = general_purpose::STANDARD.encode(&bytes);
            parts.push(json!({
                "inline_data": {
                    "mime_type": "audio/ogg",
                    "data": b64
                }
            }));
        }

        let body = json!({
            "systemInstruction": {
                "parts": [{"text": system_message}]
            },
            "contents": [{
                "role": "user",
                "parts": parts
            }]
        });

        // 🔄 Enterprise Retry Logic with Exponential Backoff
        let mut attempts = 0;
        let max_attempts = 4;
        let mut last_error = String::new();

        while attempts < max_attempts {
            let res = self.client.post(&url).json(&body).send().await?;
            let status = res.status();
            let json_res: serde_json::Value = res.json().await?;

            if status.is_success() {
                if let Some(text) =
                    json_res["candidates"][0]["content"]["parts"][0]["text"].as_str()
                {
                    return Ok(text.trim().to_string());
                } else {
                    tracing::error!("[GEMINI ERROR] Missing text in response: {}", json_res);
                    return Err(anyhow::anyhow!("Failed to parse Gemini response structure"));
                }
            } else if status.as_u16() == 503 || status.as_u16() == 429 {
                attempts += 1;
                tracing::warn!(
                    "⚠️ [API OVERLOAD] Google Servers busy ({}). Attempt {}/{}...",
                    status,
                    attempts,
                    max_attempts
                );
                tokio::time::sleep(tokio::time::Duration::from_secs(2 * attempts as u64)).await;
                last_error = json_res.to_string();
                continue;
            } else {
                tracing::error!("[GEMINI ERROR] HTTP {}: {}", status, json_res);
                return Err(anyhow::anyhow!(
                    "API Error {}: {}",
                    status,
                    json_res["error"]["message"].as_str().unwrap_or("Unknown")
                ));
            }
        }

        Err(anyhow::anyhow!("Google AI servers are currently overloaded. Please try again in a few minutes. (Failed after {} attempts). Details: {}", max_attempts, last_error))
    }
}

pub type SharedAiEngine = Arc<Mutex<LocalAiEngine>>;

pub async fn inject_live_wallet_context(chat_id: i64, ctx: &crate::context::AppContext) -> String {
    let mut live_data = String::new();

    if let Ok(dag_info) = ctx.rpc.get_block_dag_info().await {
        live_data.push_str(&format!(
            "Network Difficulty: {}. \nDAA Score: {}. \n",
            crate::kaspa_features::format_difficulty(dag_info.difficulty),
            dag_info.virtual_daa_score
        ));
    }

    let price = ctx.price_cache.read().await.0;
    if price > 0.0 {
        live_data.push_str(&format!("KAS Price: ${:.4} USD. \n", price));
    }

    let wallets: Vec<String> = ctx
        .state
        .iter()
        .filter(|e| e.value().contains(&chat_id))
        .map(|e| e.key().clone())
        .collect();

    if !wallets.is_empty() {
        let mut total = 0.0;
        for w in &wallets {
            if let Ok(addr) = Address::try_from(w.as_str()) {
                if let Ok(utxos) = ctx.rpc.get_utxos_by_addresses(vec![addr]).await {
                    total += utxos
                        .iter()
                        .map(|u| u.utxo_entry.amount as f64)
                        .sum::<f64>()
                        / 1e8;
                }
            }
        }
        live_data.push_str(&format!("User Balance: {:.8} KAS.\n", total));
    } else {
        live_data.push_str("User Balance: 0 KAS (No wallet tracked).\n");
    }

    live_data
}

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
        .send_message(
            chat_id,
            "⏳ <b>Kaspa AI:</b> Analyzing... (Gemini 2.5 Flash)",
        )
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
        .generate(&enriched_prompt, &live_ctx_data, None)
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
        .send_message(
            chat_id,
            "⏳ <b>Kaspa AI:</b> Processing Audio with Gemini...",
        )
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
            "Transcribe this audio precisely, then answer any questions asked in it.",
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
