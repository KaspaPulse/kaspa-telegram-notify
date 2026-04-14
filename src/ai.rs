use crate::context::AppContext;
use serde_json::json;
use teloxide::prelude::*;

pub async fn process_conversational_intent(
    bot: Bot,
    chat_id: teloxide::types::ChatId,
    msg_id: teloxide::types::MessageId,
    _user_id: i64,
    user_text: String,
    ctx: AppContext,
) -> anyhow::Result<()> {
    let _ = sqlx::query("CREATE TABLE IF NOT EXISTS chat_history (id INTEGER PRIMARY KEY AUTOINCREMENT, chat_id INTEGER, role TEXT, message TEXT, timestamp DATETIME DEFAULT CURRENT_TIMESTAMP)").execute(&ctx.pool).await;
    let records: Vec<(String, String)> = sqlx::query_as("SELECT role, message FROM (SELECT role, message, id FROM chat_history WHERE chat_id = ?1 ORDER BY id DESC LIMIT 6) ORDER BY id ASC")
        .bind(chat_id.0).fetch_all(&ctx.pool).await.unwrap_or_default();

    let mut history = String::new();
    for (role, msg) in records { history.push_str(&format!("{}: {}\n", role.to_uppercase(), msg)); }

    let api_key = std::env::var("GEMINI_API_KEY").unwrap_or_default();
    let url = format!("https://generativelanguage.googleapis.com/v1beta/models/gemini-1.5-flash:generateContent?key={}", api_key);

    let initial_msg = bot.send_message(chat_id, "⏳ <b>Gemini AI:</b> Thinking...")
        .reply_parameters(teloxide::types::ReplyParameters::new(msg_id))
        .parse_mode(teloxide::types::ParseMode::Html).await?;

    let client = reqwest::Client::new();
    let payload = json!({
        "contents": [{ "parts": [{ "text": format!("You are an expert Kaspa AI. Use the following history to answer:\n{}\n\nQuestion: {}", history, user_text) }] }]
    });

    if let Ok(res) = client.post(&url).json(&payload).send().await {
        if let Ok(res_json) = res.json::<serde_json::Value>().await {
            let ai_reply = res_json["candidates"][0]["content"]["parts"][0]["text"].as_str().unwrap_or("⚠️ Gemini Error").to_string();
            let _ = sqlx::query("INSERT INTO chat_history (chat_id, role, message) VALUES (?1, 'user', ?2)").bind(chat_id.0).bind(&user_text).execute(&ctx.pool).await;
            let _ = sqlx::query("INSERT INTO chat_history (chat_id, role, message) VALUES (?1, 'assistant', ?2)").bind(chat_id.0).bind(&ai_reply).execute(&ctx.pool).await;
            bot.edit_message_text(chat_id, initial_msg.id, format!("🤖 <b>Kaspa Intelligence:</b>\n{}", ai_reply)).parse_mode(teloxide::types::ParseMode::Html).await?;
        }
    }
    Ok(())
}
