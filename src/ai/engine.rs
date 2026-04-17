use reqwest::Client;
use serde_json::json;
use sqlx::SqlitePool;
use std::sync::Arc;
use tokio::sync::Mutex;

pub struct LocalAiEngine {
    pub client: Client,
    pub api_key: String,
    pub base_url: String,
    pub chat_model: String,
    pub audio_model: String,
}

pub type SharedAiEngine = Arc<Mutex<LocalAiEngine>>;

impl LocalAiEngine {
    pub fn new() -> anyhow::Result<Self> {
        tracing::info!("[AI ENGINE] Initializing Universal OpenAI-Standard API Engine...");

        // 🔑 Configuration
        let api_key = std::env::var("AI_API_KEY").expect("⚠️ AI_API_KEY is missing in .env");
        let base_url = std::env::var("AI_BASE_URL")
            .unwrap_or_else(|_| "https://api.groq.com/openai/v1".to_string());
        let chat_model = std::env::var("AI_CHAT_MODEL")
            .unwrap_or_else(|_| "llama-3.3-70b-versatile".to_string());
        let audio_model =
            std::env::var("AI_AUDIO_MODEL").unwrap_or_else(|_| "whisper-large-v3".to_string());

        tracing::info!("[AI ENGINE] Target: {} | Model: {}", base_url, chat_model);

        Ok(Self {
            client: Client::new(),
            api_key,
            base_url,
            chat_model,
            audio_model,
        })
    }

    pub async fn generate(
        &self,
        pool: &SqlitePool, // 🆕 Added Pool for RAG access
        prompt: &str,
        live_context: &str,
        audio_bytes: Option<Vec<u8>>,
    ) -> anyhow::Result<String> {
        let mut final_prompt = prompt.to_string();

        // 🎙️ STEP 1: Handle Audio Transcriptions
        if let Some(bytes) = audio_bytes {
            tracing::info!("[AI ENGINE] Transcribing audio via {}...", self.audio_model);
            let url = format!("{}/audio/transcriptions", self.base_url);

            let part = reqwest::multipart::Part::bytes(bytes)
                .file_name("audio.ogg")
                .mime_str("audio/ogg")?;

            let form = reqwest::multipart::Form::new()
                .part("file", part)
                .text("model", self.audio_model.clone());

            let res = self
                .client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .multipart(form)
                .send()
                .await?;

            let status = res.status();
            let json_res: serde_json::Value = res.json().await?;

            if status.is_success() {
                if let Some(transcription) = json_res["text"].as_str() {
                    tracing::info!("[AUDIO PARSED]: {}", transcription);
                    // Use the transcription as the primary prompt if it exists
                    final_prompt = transcription.to_string();
                }
            } else {
                tracing::error!("[AUDIO ERROR] HTTP {}: {}", status, json_res);
                return Err(anyhow::anyhow!(
                    "Audio transcription failed. Ensure the provider supports Whisper API."
                ));
            }
        }

        // 🧠 STEP 2: Semantic RAG Execution (Retrieval-Augmented Generation)
        // We pass the parsed text (or original text) to find relevant RSS news.
        let rag_context = crate::ai::rag::get_rag_context(pool, &final_prompt).await;

        // 🏗️ STEP 3: Construct the Enterprise System Prompt
        let system_message = format!(
            "You are Kaspa Pulse, an uncompromisingly accurate, highly professional Kaspa AI Assistant.

[ABSOLUTE RULES]
1. DO NOT invent, hallucinate, or assume facts. If you don't know, state it clearly.
2. Kaspa is a pure Proof-of-Work (PoW) BlockDAG. It has NO CEO, NO PoS, NO Smart Contracts.
3. Keep answers concise, factual, and formatted beautifully using Telegram HTML tags (<b>, <i>, <code>).
4. Always prioritize the Knowledge Base and Live Data provided below over your base training data.

[LIVE NODE DATA]
(Use this to answer questions about the user's balance, hashrate, or current network state)
{}

{}",
            live_context, rag_context
        );

        // 🌐 STEP 4: Execute Chat Completion Request
        let url = format!("{}/chat/completions", self.base_url);
        let body = json!({
            "model": self.chat_model,
            "messages": [
                {"role": "system", "content": system_message},
                {"role": "user", "content": final_prompt}
            ],
            "temperature": 0.2 // Lower temperature for high factual accuracy
        });

        // 🔄 STEP 5: Enterprise Exponential Backoff & Retry Logic
        let mut attempts = 0;
        let max_attempts = 4;
        let mut last_error = String::new();

        while attempts < max_attempts {
            let res = self
                .client
                .post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .json(&body)
                .send()
                .await?;

            let status = res.status();
            let json_res: serde_json::Value = res.json().await?;

            if status.is_success() {
                if let Some(text) = json_res["choices"][0]["message"]["content"].as_str() {
                    return Ok(text.trim().to_string());
                } else {
                    tracing::error!(
                        "[API ERROR] Missing standard text choice in response: {}",
                        json_res
                    );
                    return Err(anyhow::anyhow!(
                        "Failed to parse standard API response structure."
                    ));
                }
            } else if status.as_u16() == 503 || status.as_u16() == 429 {
                attempts += 1;
                tracing::warn!(
                    "⚠️ [API OVERLOAD] Servers busy ({}). Attempt {}/{}...",
                    status,
                    attempts,
                    max_attempts
                );
                // Exponential backoff
                tokio::time::sleep(tokio::time::Duration::from_secs(2 * attempts as u64)).await;
                last_error = json_res.to_string();
                continue;
            } else {
                tracing::error!("[API ERROR] HTTP {}: {}", status, json_res);
                return Err(anyhow::anyhow!(
                    "API Error {}: {}",
                    status,
                    json_res["error"]["message"]
                        .as_str()
                        .unwrap_or("Unknown error")
                ));
            }
        }

        Err(anyhow::anyhow!(
            "AI servers are currently overloaded after multiple retries. Details: {}",
            last_error
        ))
    }
}
