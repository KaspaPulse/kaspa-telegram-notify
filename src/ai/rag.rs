use sqlx::SqlitePool;
use tracing::info;

pub async fn get_rag_context(pool: &SqlitePool, user_query: &str) -> String {
    // 1. Extract the most meaningful keyword from the query (Rudimentary NLP)
    let search_term = user_query
        .split_whitespace()
        .filter(|w| w.len() > 4) // Skip stop words like "what", "is", "the"
        .max_by_key(|w| w.len())
        .unwrap_or("kaspa")
        .replace(&['\'', '"', '%', '_', '?', '!'][..], ""); // Sanitize SQL

    let search_pattern = format!("%{}%", search_term);

    // 2. Query the Knowledge Base for the top 3 relevant articles
    let records: Result<Vec<(String, String, String)>, _> = sqlx::query_as(
        "SELECT title, content, source 
         FROM knowledge_base 
         WHERE content LIKE ?1 OR title LIKE ?1
         ORDER BY published_at DESC 
         LIMIT 3",
    )
    .bind(&search_pattern)
    .fetch_all(pool)
    .await;

    // 3. Format the Context for the LLM
    match records {
        Ok(articles) if !articles.is_empty() => {
            info!(
                "🧠 [RAG ENGINE] Retrieved {} context articles for keyword: '{}'",
                articles.len(),
                search_term
            );

            let mut context = String::from("\n\n=== RECENT KASPA KNOWLEDGE BASE (Use this strictly to answer the user if relevant) ===\n");
            for (title, content, source) in articles {
                // Truncate to save tokens (approx 1000 chars per article)
                let truncated = if content.len() > 1000 {
                    &content[..1000]
                } else {
                    &content
                };
                context.push_str(&format!(
                    "Title: {}\nSource: {}\nSnippet: {}...\n\n",
                    title, source, truncated
                ));
            }
            context.push_str(
                "==============================================================================\n",
            );
            context
        }
        _ => {
            info!("🔍 [RAG ENGINE] No specific context found for keyword: '{}'. Relying on base model knowledge.", search_term);
            String::new()
        }
    }
}
