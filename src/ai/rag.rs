use sqlx::{PgPool, Postgres};
use tracing::{info, warn};

/// Keywords to identify user intent for global news/updates.
const NEWS_INTENT: &[&str] = &[
    "news", "update", "latest", "recent", "announcement", "release", "roadmap", "whats new",
    "خبر", "اخبار", "جديد", "اخر", "مستجدات", "تحديث", "تطورات", "اعلان",
];

/// Keywords to identify technical protocol discussions.
const TECH_INTENT: &[&str] = &[
    "protocol", "algorithm", "mining", "consensus", "dagknight", "smart", "pow", "kheavyhash",
    "تقني", "بروتوكول", "خوارزمية", "تعدين", "اجماع", "بلوك", "داج",
];

/// Keywords for live network metrics.
const METRIC_INTENT: &[&str] = &[
    "hashrate", "price", "difficulty", "supply", "market", "daa", "tps", "bps",
    "سعر", "هاشريت", "صعوبة", "امداد", "سوق", "اداء", "احصائيات",
];

/// Enterprise RAG Engine: Retrieves local context or triggers the Autonomous Agent.
pub async fn get_rag_context(pool: &PgPool, user_query: &str) -> String {
    let lower_query = user_query.to_lowercase();

    // Intent Detection Logic
    let is_news = NEWS_INTENT.iter().any(|&k| lower_query.contains(k));
    let is_metric = METRIC_INTENT.iter().any(|&k| lower_query.contains(k));
    let _is_tech = TECH_INTENT.iter().any(|&k| lower_query.contains(k));

    info!("[RAG] Analyzing query intent for: '{}'", user_query);

    // Step 1: Attempt to fetch from Local PostgreSQL Knowledge Base
    let result: Result<Vec<(String, String)>, sqlx::Error> = if is_news || is_metric {
        info!("[RAG] Priority Intent: Global Context Refresh.");
        sqlx::query_as::<Postgres, (String, String)>(
            "SELECT title, content FROM knowledge_base ORDER BY published_at DESC LIMIT 5",
        )
        .fetch_all(pool)
        .await
    } else {
        // Extract the most significant word for anchor-based search
        let search_anchor = user_query
            .split_whitespace()
            .filter(|w| w.len() > 3)
            .max_by_key(|w| w.len())
            .unwrap_or("kaspa");

        info!("[RAG] Search Anchor identified: '{}'", search_anchor);
        sqlx::query_as::<Postgres, (String, String)>(
            "SELECT title, content FROM knowledge_base 
             WHERE content ILIKE $1 OR title ILIKE $1 
             ORDER BY published_at DESC LIMIT 3"
        )
        .bind(format!("%{}%", search_anchor))
        .fetch_all(pool).await
    };

    // Step 2: Evaluate Results and Handle Fallbacks
    match result {
        // If DB has relevant data, format and return it immediately.
        Ok(articles) if !articles.is_empty() => {
            info!("[RAG] Local Knowledge found. Injecting context.");
            let mut context_buffer = String::from("\n[OFFICIAL KASPA KNOWLEDGE & UPDATES]:\n");
            for (title, content) in articles {
                let snippet = if content.len() > 600 {
                    &content[..600]
                } else {
                    &content
                };
                context_buffer.push_str(&format!("- Source: {}\n  Details: {}\n", title, snippet));
            }
            context_buffer
        }

        // Step 3: Critical Fallback - Trigger Autonomous Agent (Tavily)
        _ => {
            info!("[RAG] CACHE MISS: Local DB is silent. Engaging Autonomous Agent for Live Intelligence...");
            
            // Invoke the Agent to search the web and update the Knowledge Base
            if let Some(agent_answer) = crate::agent::search_and_learn(pool, user_query).await {
                info!("[RAG] Agent successfully retrieved external data.");
                format!("\n[AUTONOMOUS LIVE SEARCH RESULT]:\n{}\n", agent_answer)
            } else {
                warn!("[RAG] Final Fallback: Agent failed. Proceeding with base AI knowledge.");
                String::new()
            }
        }
    }
}