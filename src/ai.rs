use crate::context::AppContext;
use serde_json::json;
use teloxide::prelude::*;
use kaspa_addresses::Address;
use kaspa_rpc_core::api::rpc::RpcApi;

pub async fn inject_live_wallet_context(chat_id: i64, ctx: &AppContext) -> String {
    let mut live_data = String::new();
    if let Ok(dag_info) = ctx.rpc.get_block_dag_info().await {
        live_data.push_str(&format!("Network Difficulty: {}. DAA Score: {}. ", 
            crate::kaspa_features::format_difficulty(dag_info.difficulty), dag_info.virtual_daa_score));
    }
    let price = ctx.price_cache.read().await.0;
    if price > 0.0 { live_data.push_str(&format!("KAS Price: ${:.4} USD. ", price)); }

    let wallets: Vec<String> = ctx.state.iter().filter(|e| e.value().contains(&chat_id)).map(|e| e.key().clone()).collect();
    if !wallets.is_empty() {
        let mut total = 0.0;
        for w in &wallets {
            if let Ok(addr) = Address::try_from(w.as_str()) {
                if let Ok(utxos) = ctx.rpc.get_utxos_by_addresses(vec![addr]).await {
                    total += utxos.iter().map(|u| u.utxo_entry.amount as f64).sum::<f64>() / 1e8;
                }
            }
        }
        live_data.push_str(&format!("User Balance: {:.8} KAS.", total));
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
    
    // 1. Database Memory
    let _ = sqlx::query("CREATE TABLE IF NOT EXISTS chat_history (id INTEGER PRIMARY KEY AUTOINCREMENT, chat_id INTEGER, role TEXT, message TEXT, timestamp DATETIME DEFAULT CURRENT_TIMESTAMP)").execute(&ctx.pool).await;
    let records: Vec<(String, String)> = sqlx::query_as("SELECT role, message FROM (SELECT role, message, id FROM chat_history WHERE chat_id = ?1 ORDER BY id DESC LIMIT 6) ORDER BY id ASC")
        .bind(chat_id.0).fetch_all(&ctx.pool).await.unwrap_or_default();

    let mut history = String::new();
    for (role, msg) in records { history.push_str(&format!("{}: {}\n", role.to_uppercase(), msg)); }

    let live_data = inject_live_wallet_context(chat_id.0, &ctx).await;
    let rag_docs = crate::rag::search_kaspa_docs(&user_text);

    let initial_msg = bot.send_message(chat_id, "⏳ <b>Gemini Intelligence:</b> Processing via Cloud...").reply_parameters(teloxide::types::ReplyParameters::new(msg_id)).parse_mode(teloxide::types::ParseMode::Html).await?;

    // 2. HTTP Request to Google Gemini 1.5
    let api_key = std::env::var("GEMINI_API_KEY").unwrap_or_else(|_| "MISSING_KEY".to_string());
    if api_key == "MISSING_KEY" {
        bot.edit_message_text(chat_id, initial_msg.id, "⚠️ <b>Error:</b> GEMINI_API_KEY is not set in .env file!").parse_mode(teloxide::types::ParseMode::Html).await?;
        return Ok(());
    }

    let url = format!("https://generativelanguage.googleapis.com/v1beta/models/gemini-1.5-flash:generateContent?key={}", api_key);
    let sys_prompt = format!("You are an expert Kaspa Enterprise AI. Use [HISTORY] and [LIVE DATA] to answer accurately. Never invent facts. Be concise.\n\n[LIVE DATA]: {}\n\n[DOCS]: {}", live_data, rag_docs);

    let client = reqwest::Client::new();
    let payload = json!({
        "contents": [{ "parts": [{ "text": format!("{}\n\n[HISTORY]:\n{}\n\nQuestion: {}", sys_prompt, history, user_text) }] }]
    });

    let res = client.post(&url).json(&payload).send().await?;
    let res_json: serde_json::Value = res.json().await?;
    let ai_reply = res_json["candidates"][0]["content"]["parts"][0]["text"].as_str().unwrap_or("⚠️ Gemini Generation Error").to_string();

    // 3. Save to Memory
    let _ = sqlx::query("INSERT INTO chat_history (chat_id, role, message) VALUES (?1, 'user', ?2)").bind(chat_id.0).bind(&user_text).execute(&ctx.pool).await;
    let _ = sqlx::query("INSERT INTO chat_history (chat_id, role, message) VALUES (?1, 'assistant', ?2)").bind(chat_id.0).bind(&ai_reply).execute(&ctx.pool).await;

    bot.edit_message_text(chat_id, initial_msg.id, format!("🤖 <b>Kaspa Intelligence:</b>\n{}", ai_reply)).parse_mode(teloxide::types::ParseMode::Html).await?;

    Ok(())
}
