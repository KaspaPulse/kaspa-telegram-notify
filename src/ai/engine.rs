use reqwest::Client;
use serde_json::json;
use sqlx::PgPool;
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
        tracing::info!("[AI ENGINE] Initializing Sovereign OpenAI-Standard Engine...");

        let api_key = std::env::var("AI_API_KEY").expect("⚠️ AI_API_KEY is missing in .env");
        let base_url = std::env::var("AI_BASE_URL")
            .unwrap_or_else(|_| "https://api.groq.com/openai/v1".to_string());
        let chat_model = std::env::var("AI_CHAT_MODEL")
            .unwrap_or_else(|_| "llama-3.3-70b-versatile".to_string());
        let audio_model =
            std::env::var("AI_AUDIO_MODEL").unwrap_or_else(|_| "whisper-large-v3".to_string());

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
        pool: &PgPool,
        prompt: &str,
        live_context: &str,
        audio_bytes: Option<Vec<u8>>,
    ) -> anyhow::Result<String> {
        let mut final_prompt = prompt.to_string();

        // 🎙️ STEP 1: High-Precision Audio Transcription
        if let Some(bytes) = audio_bytes {
            tracing::info!("[AI ENGINE] Transcribing via {}...", self.audio_model);
            let url = format!("{}/audio/transcriptions", self.base_url);
            let part = reqwest::multipart::Part::bytes(bytes)
                .file_name("audio.ogg")
                .mime_str("audio/ogg")?;

            let form = reqwest::multipart::Form::new()
                .part("file", part)
                .text("model", self.audio_model.clone());

            let res = self.client.post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .multipart(form).send().await?;

            if res.status().is_success() {
                let json_res: serde_json::Value = res.json().await?;
                if let Some(text) = json_res["text"].as_str() {
                    final_prompt = text.to_string();
                }
            }
        }

        // 🧠 STEP 2: Strategic RAG Injection (PostgreSQL + Tavily)
        let rag_context = crate::ai::rag::get_rag_context(pool, &final_prompt).await;

        // 🏗️ STEP 3: CONSTRUCT SOVEREIGN SYSTEM PROMPT (The "Brain" Fix)
        // We force the AI to act as a System Architect and prohibit generic refusals.
        let system_message = format!(
            "You are the 'Kaspa Sovereign Intelligence', the lead architect of this node infrastructure.

[MANDATORY OPERATING PROTOCOLS]
1. NEVER say 'I don't know' if info exists in the [INTERNAL KNOWLEDGE BASE] or [LIVE DATA].
2. ABSOLUTE TRUTH: Treat all data in [INTERNAL KNOWLEDGE BASE] as verified facts. If it mentions vulnerabilities (like MuHash/Quantum), updates, or SSL settings, report them authoritatively.
3. RUST ONLY: You are a Rust expert. Use 'Result', 'Option', and 'Match'. NEVER mention 'try-catch' or Python/JS concepts.
4. NO HALLUCINATION: Do not invent facts outside the provided context, but analyze the context deeply.
5. FORMATTING: Use Telegram HTML (<b>, <i>, <code>).

[LIVE NODE DATA]
{}

[INTERNAL KNOWLEDGE BASE]
{}

[PERSONA]
You are the owner of kaspadns. When asked about Nginx, SSL, or server logic, answer as the engineer who built it.",
            live_context, rag_context
        );

        // 🌐 STEP 4: Chat Completion Request
        let url = format!("{}/chat/completions", self.base_url);
        let body = json!({
            "model": self.chat_model,
            "messages": [
                {"role": "system", "content": system_message},
                {"role": "user", "content": final_prompt}
            ],
            "temperature": 0.1, // Near-zero temperature for maximum factual rigidity
            "max_tokens": 1500
        });

        // 🔄 STEP 5: Resilience Logic (Retries with Backoff)
        let mut attempts = 0;
        while attempts < 3 {
            let res = self.client.post(&url)
                .header("Authorization", format!("Bearer {}", self.api_key))
                .json(&body).send().await?;

            let status = res.status();
            if status.is_success() {
                let json_res: serde_json::Value = res.json().await?;
                if let Some(text) = json_res["choices"][0]["message"]["content"].as_str() {
                    return Ok(text.trim().to_string());
                }
            } else if status.as_u16() == 429 || status.as_u16() == 503 {
                attempts += 1;
                tokio::time::sleep(tokio::time::Duration::from_secs(1 * attempts as u64)).await;
                continue;
            } else {
                return Err(anyhow::anyhow!("AI Engine Error: {}", status));
            }
        }

        Err(anyhow::anyhow!("Sovereign Engine failed after multiple retries."))
    }
}