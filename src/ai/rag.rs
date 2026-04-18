use sqlx::{PgPool, Postgres};
use tracing::{info, warn};

/// Keywords to identify user intent for global news/updates.
const NEWS_INTENT: &[&str] = &[
    "news", "update", "latest", "recent", "announcement", "release", "roadmap", "whats new",
    "خبر", "اخبار", "جديد", "اخر", "مستجدات", "تحديث", "تطورات", "اعلان",
];

/// Keywords for live network metrics.
const METRIC_INTENT: &[&str] = &[
    "hashrate", "price", "difficulty", "supply", "market", "daa", "tps", "bps",
    "سعر", "هاشريت", "صعوبة", "امداد", "سوق", "اداء", "احصائيات",
];

/// Enterprise RAG Engine: Retrieves local context with manual override priority.
pub async fn get_rag_context(pool: &PgPool, user_query: &str) -> String {
    let lower_query = user_query.to_lowercase();

    // Intent Detection
    let is_news = NEWS_INTENT.iter().any(|&k| lower_query.contains(k));
    let is_metric = METRIC_INTENT.iter().any(|&k| lower_query.contains(k));

    info!("[RAG] Analyzing query intent for: '{}'", user_query);

    // Step 1: Attempt to fetch from Local PostgreSQL Knowledge Base
    // We use a CASE statement in SQL to prioritize "Manual Input" from the /learn command.
    let result: Result<Vec<(String, String)>, sqlx::Error> = if is_news || is_metric {
        info!("[RAG] Priority Intent: Global Context Refresh.");
        sqlx::query_as::<Postgres, (String, String)>(
            "SELECT title, content FROM knowledge_base 
             ORDER BY 
                CASE WHEN title LIKE 'Manual Input%' THEN 0 ELSE 1 END, 
                published_at DESC 
             LIMIT 5"
        )
        .fetch_all(pool)
        .await
    } else {
        // Multi-word Anchor Extraction to improve Arabic/English matching
        let search_anchor = user_query
            .split_whitespace()
            .filter(|w| w.len() > 2) // Match shorter keywords like 'S9' or 'Pi'
            .max_by_key(|w| w.len())
            .unwrap_or("kaspa");

        info!("[RAG] Search Anchor identified: '{}'", search_anchor);
        
        sqlx::query_as::<Postgres, (String, String)>(
            "SELECT title, content FROM knowledge_base 
             WHERE content ILIKE $1 OR title ILIKE $1 
             ORDER BY 
                CASE WHEN title LIKE 'Manual Input%' THEN 0 ELSE 1 END, 
                published_at DESC 
             LIMIT 5"
        )
        .bind(format!("%{}%", search_anchor))
        .fetch_all(pool)
        .await
    };

    // Step 2: Evaluate Results and Handle Fallbacks
    match result {
        Ok(articles) if !articles.is_empty() => {
            info!("[RAG] Local Knowledge found ({} entries). Injecting context.", articles.len());
            let mut context_buffer = String::from("\n[VERIFIED KNOWLEDGE BASE]:\n");
            for (title, content) in articles {
                let snippet = if content.len() > 800 { &content[..800] } else { &content };
                context_buffer.push_str(&format!("- Category: {}\n  Data: {}\n", title, snippet));
            }
            context_buffer
        }

        // Step 3: Critical Fallback - Trigger Autonomous Agent (Tavily)
        _ => {
            info!("[RAG] CACHE MISS: Local DB silent or irrelevant. Engaging Autonomous Agent...");
            if let Some(agent_answer) = crate::agent::search_and_learn(pool, user_query).await {
                info!("[RAG] Agent successfully retrieved live external data.");
                format!("\n[AUTONOMOUS LIVE SEARCH RESULT]:\n{}\n", agent_answer)
            } else {
                warn!("[RAG] Final Fallback: Agent failed. Proceeding with base AI knowledge.");
                String::new()
            }
        }
    }
}