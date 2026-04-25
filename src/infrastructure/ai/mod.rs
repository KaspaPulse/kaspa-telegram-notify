pub mod ai_engine_adapter;

// ==========================================
// --- Merged from Autonomous Agent ---
// ==========================================

use crate::infrastructure::database::postgres_adapter::PostgresRepository;
use reqwest::Client;
use serde_json::{json, Value};
use std::env;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};

/// Autonomous Intelligence Agent: Deep Research with Multi-Layer Fallback.
pub struct AiAgent {
    db: Arc<PostgresRepository>,
    client: Client,
}

impl AiAgent {
    pub fn new(db: Arc<PostgresRepository>) -> Self {
        Self {
            db,
            client: Client::builder()
                .timeout(Duration::from_secs(15))
                .build()
                .unwrap_or_else(|_| Client::new()),
        }
    }

    pub async fn search_and_learn(&self, query: &str) -> Option<String> {
        // 🛡️ API Key Validation
        let api_key = match env::var("TAVILY_API_KEY") {
            Ok(key) if !key.is_empty() => key,
            _ => {
                error!("[AI AGENT] CRITICAL: TAVILY_API_KEY is missing from .env!");
                return None;
            }
        };

        info!(
            "[AI AGENT] Initiating Deep Intelligence Gathering for: '{}'",
            query
        );

        // Check if query is technical to expand search window
        let is_tech = query.to_lowercase().contains(".rs")
            || query.to_lowercase().contains("code")
            || query.to_lowercase().contains("ssl")
            || query.to_lowercase().contains("settings");

        let res = self
            .client
            .post("https://api.tavily.com/search")
            .json(&json!({
                "api_key": api_key,
                "query": query,
                "search_depth": "advanced",
                "include_answer": true,
                "max_results": 5,
                "days": if is_tech { 365 } else { 7 }
            }))
            .send()
            .await;

        match res {
            Ok(response) => {
                if let Ok(body) = response.json::<Value>().await {
                    // Tier 1: Direct AI Summary
                    if let Some(answer) = body.get("answer").and_then(|a| a.as_str()) {
                        let answer_str = answer.to_string();
                        self.save_intelligence(query, &body, &answer_str).await;
                        return Some(answer_str);
                    }

                    // Tier 2: Content Snippet Aggregation (If Tier 1 fails)
                    if let Some(results) = body.get("results").and_then(|r| r.as_array()) {
                        if !results.is_empty() {
                            let mut aggregated_intel = String::new();
                            for res in results.iter().take(3) {
                                if let Some(content) = res.get("content").and_then(|c| c.as_str()) {
                                    aggregated_intel.push_str(content);
                                    aggregated_intel.push_str("\n\n");
                                }
                            }
                            if !aggregated_intel.is_empty() {
                                self.save_intelligence(query, &body, &aggregated_intel)
                                    .await;
                                return Some(aggregated_intel);
                            }
                        }
                    }
                    warn!("[AI AGENT] Zero data found for query: {}", query);
                }
            }
            Err(e) => error!("[AI AGENT] Tavily Connection Error: {}", e),
        }
        None
    }

    /// Commits findings to PostgreSQL and keeps the Knowledge Base fresh via Clean Arch Port.
    async fn save_intelligence(&self, query: &str, body: &Value, answer: &str) {
        let source_link = body
            .get("results")
            .and_then(|r| r.as_array())
            .and_then(|arr| arr.first())
            .and_then(|first| first.get("url"))
            .and_then(|url| url.as_str())
            .unwrap_or("https://kaspadns.net/intelligence");

        let title_str = format!("Agent Discovery: {}", query);

        let _ = self
            .db
            .add_to_knowledge_base(
                &title_str,
                source_link,
                answer,
                "Autonomous Agent v2.5 (Deep Search)",
            )
            .await;

        info!(
            "[AI AGENT] Successfully synced intelligence to DB for: {}",
            query
        );
    }
}
