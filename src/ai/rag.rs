use sqlx::{PgPool, Postgres};
use tracing::info;

/// Keywords to identify user intent for global news/updates.
const NEWS_INTENT: &[&str] = &[
    "news", "update", "latest", "recent", "whats new", 
    "خبر", "اخبار", "جديد", "تحديث", "مستجدات"
];

/// Keywords for live network metrics and security infrastructure.
const _METRIC_INTENT: &[&str] = &[
    "hashrate", "price", "difficulty", "سعر", "صعوبة", "احصائيات", 
    "ssl", "security", "حماية"
];

/// Enterprise RAG Engine: Multi-word anchor search with live fallback.
pub async fn get_rag_context(pool: &PgPool, user_query: &str) -> String {
    let lower_query = user_query.to_lowercase();
    let is_news = NEWS_INTENT.iter().any(|&k| lower_query.contains(k));
    
    info!("[RAG] CRITICAL SEARCH INITIATED: '{}'", user_query);

    // 1. Force Live Intelligence for News (Bypass DB entirely for fresh data)
    if is_news {
        info!("[RAG] News intent detected. Bypassing local DB for Tavily.");
        return trigger_autonomous_agent(pool, user_query).await;
    }

    // 2. Enhanced Local Search: Iterating through all significant words (> 2 chars)
    // This ensures that even short technical terms like 'SSL' are caught.
    let words: Vec<&str> = lower_query.split_whitespace().filter(|w| w.len() > 2).collect();
    let mut combined_results = Vec::new();

    for word in words {
        let pattern = format!("%{}%", word);
        if let Ok(mut articles) = sqlx::query_as::<Postgres, (String, String)>(
            "SELECT title, content FROM knowledge_base 
             WHERE content ILIKE $1 OR title ILIKE $1 
             ORDER BY CASE WHEN title LIKE 'Manual Input%' THEN 0 ELSE 1 END, id DESC 
             LIMIT 2"
        ).bind(pattern).fetch_all(pool).await {
            combined_results.append(&mut articles);
        }
    }

    if !combined_results.is_empty() {
        info!("[RAG] Local Knowledge Found. Injecting into context.");
        let mut context = String::from("\n[INTERNAL SERVER PROTOCOLS & DATA]:\n");
        
        // Remove duplicates if multiple words hit the same article
        combined_results.dedup(); 
        
        for (title, content) in combined_results.iter().take(4) {
            let snippet = if content.len() > 600 { &content[..600] } else { &content };
            context.push_str(&format!("- {}: {}\n", title, snippet));
        }
        return context;
    }

    // 3. Fallback to Agent if local search yields no relevant results
    info!("[RAG] Local search silent. Engaging Autonomous Agent.");
    trigger_autonomous_agent(pool, user_query).await
}

/// Helper to trigger the Autonomous Agent and perform database maintenance.
async fn trigger_autonomous_agent(pool: &PgPool, query: &str) -> String {
    if let Some(agent_answer) = crate::agent::search_and_learn(pool, query).await {
        // Auto-Pruning: Clean old data while protecting user-added 'Manual Input'
        let _ = sqlx::query(
            "DELETE FROM knowledge_base 
             WHERE published_at < NOW() - INTERVAL '7 days' 
             AND title NOT LIKE 'Manual Input%'"
        ).execute(pool).await;
        
        format!("\n[LIVE AGENT REPORT]:\n{}\n", agent_answer)
    } else {
        String::new()
    }
}