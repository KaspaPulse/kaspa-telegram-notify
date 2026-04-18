use reqwest::Client;
use serde_json::{json, Value};
use sqlx::PgPool;
use std::env;
use tracing::{error, info, warn};

/// Autonomous Intelligence Agent: Searches the web and updates the local Knowledge Base.
pub async fn search_and_learn(pool: &PgPool, query: &str) -> Option<String> {
    // 🛡️ Strict Error Handling: Ensure the API key is actually present.
    let api_key = match env::var("TAVILY_API_KEY") {
        Ok(key) if !key.is_empty() => key,
        _ => {
            error!("[AI AGENT] CRITICAL: TAVILY_API_KEY is not set or empty in .env!");
            return None;
        }
    };

    info!("[AI AGENT] Engaging Deep Research for query: '{}'", query);
    
    let client = Client::new();
    
    // 🧠 Dynamic Query Optimization:
    // If the query mentions code or Rust files, we pivot the search to technical sources.
    let search_query = if query.to_lowercase().contains(".rs") || query.to_lowercase().contains("code") {
        format!("Kaspa Rust source code implementation and logic: {}", query)
    } else {
        format!("Kaspa network official technical news and data: {}", query)
    };

    let res = client.post("https://api.tavily.com/search")
        .json(&json!({
            "api_key": api_key,
            "query": search_query,
            "search_depth": "advanced",
            "include_answer": true,
            "include_raw_content": false,
            "max_results": 3,
            "days": 7 // Prioritize fresh information (last 7 days)
        }))
        .send()
        .await;

    match res {
        Ok(response) => {
            if let Ok(body) = response.json::<Value>().await {
                // Priority 1: Use the AI-generated direct answer
                if let Some(answer) = body.get("answer").and_then(|a| a.as_str()) {
                    let answer_str = answer.to_string();
                    
                    // 🔗 Real Link Extraction: Get the actual source URL from results.
                    let source_link = body["results"][0]["url"]
                        .as_str()
                        .unwrap_or("https://kaspadns.net/intelligence");

                    // Step 3: Synchronize with PostgreSQL Knowledge Base
                    crate::state::add_to_knowledge_base(
                        pool, 
                        query, 
                        source_link, 
                        &answer_str, 
                        "Autonomous Internet Search (v2.0)"
                    ).await;

                    info!("[AI AGENT] Intel Acquired. Knowledge Base updated for '{}'", query);
                    return Some(answer_str);
                } else {
                    warn!("[AI AGENT] Search completed but no definitive answer was found.");
                }
            }
        }
        Err(e) => error!("[AI AGENT] External API failure: {}", e),
    }
    None
}