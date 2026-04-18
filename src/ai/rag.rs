use sqlx::{PgPool, Postgres};
use tracing::{info, warn};

const NEWS_INTENT: &[&str] = &["news", "update", "latest", "recent", "whats new", "خبر", "اخبار", "جديد", "تحديث"];
const METRIC_INTENT: &[&str] = &["hashrate", "price", "difficulty", "سعر", "صعوبة", "احصائيات", "ssl", "security", "حماية"];

pub async fn get_rag_context(pool: &PgPool, user_query: &str) -> String {
    let lower_query = user_query.to_lowercase();
    let is_news = NEWS_INTENT.iter().any(|&k| lower_query.contains(k));
    
    info!("[RAG] CRITICAL SEARCH: '{}'", user_query);

    // 1. Force Live Intelligence for News (Bypass DB entirely)
    if is_news {
        info!("[RAG] News detected. Bypassing local DB for Tavily.");
        return trigger_autonomous_agent(pool, user_query).await;
    }

    // 2. Enhanced Local Search (Searching all words > 2 chars)
    let words: Vec<&str> = lower_query.split_whitespace().filter(|w| w.len() > 2).collect();
    let mut combined_results = Vec::new();

    for word in words {
        let pattern = format!("%{}%", word);
        if let Ok(mut articles) = sqlx::query_as::<Postgres, (String, String)>(
            "SELECT title, content FROM knowledge_base WHERE content ILIKE $1 OR title ILIKE $1 ORDER BY id DESC LIMIT 2"
        ).bind(pattern).fetch_all(pool).await {
            combined_results.append(&mut articles);
        }
    }

    if !combined_results.is_empty() {
        info!("[RAG] Local Knowledge Found. Injecting...");
        let mut context = String::from("\n[INTERNAL SERVER PROTOCOLS & DATA]:\n");
        combined_results.dedup(); // إزالة التكرار
        for (t, c) in combined_results.iter().take(4) {
            context.push_str(&format!("- {}: {}\n", t, if c.len() > 600 { &c[..600] } else { &c }));
        }
        return context;
    }

    // 3. Fallback to Agent if local search fails
    trigger_autonomous_agent(pool, user_query).await
}

async fn trigger_autonomous_agent(pool: &PgPool, query: &str) -> String {
    if let Some(agent_answer) = crate::agent::search_and_learn(pool, query).await {
        // Auto-Pruning: Keep only fresh data
        let _ = sqlx::query("DELETE FROM knowledge_base WHERE published_at < NOW() - INTERVAL '7 days' AND title NOT LIKE 'Manual Input%'").execute(pool).await;
        format!("\n[LIVE AGENT REPORT]:\n{}\n", agent_answer)
    } else {
        String::new()
    }
}