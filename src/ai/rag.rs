use crate::context::AppContext;
use sqlx::{PgPool, Postgres};
use tokio::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

/// Enterprise RAG Engine: Semantic Vector Search
pub async fn get_rag_context(
    pool: &PgPool,
    user_query: &str,
    engine: &crate::ai::LocalAiEngine,
) -> String {
    info!("[RAG] Semantic Search INITIATED for: '{}'", user_query);

    // 1. Convert user query to vector
    let query_vector = match engine.get_embedding(user_query).await {
        Ok(v) => v,
        Err(e) => {
            error!(
                "[RAG ERROR] Vectorization failed: {}. Falling back to Agent.",
                e
            );
            return trigger_autonomous_agent(pool, user_query).await;
        }
    };

    let vector_str = format!("{:?}", query_vector);

    // 3. Search via Cosine Distance (<=>)
    let articles: Result<Vec<(String, String)>, _> = sqlx::query_as::<Postgres, (String, String)>(
        "SELECT title, content FROM knowledge_base 
         WHERE embedding IS NOT NULL 
         ORDER BY embedding <=> $1::vector 
         LIMIT 3",
    )
    .bind(vector_str)
    .fetch_all(pool)
    .await;

    if let Ok(results) = articles {
        if !results.is_empty() {
            info!("[RAG] Semantic Match Found. Injecting context.");
            let mut context = String::from("\n[INTERNAL SERVER PROTOCOLS & DATA]:\n");
            for (title, content) in results {
                let snippet = if content.len() > 800 {
                    &content[..800]
                } else {
                    &content
                };
                context.push_str(&format!("- {}: {}\n", title, snippet));
            }
            return context;
        }
    }

    info!("[RAG] Local search silent. Engaging Autonomous Agent.");
    trigger_autonomous_agent(pool, user_query).await
}

async fn trigger_autonomous_agent(pool: &PgPool, query: &str) -> String {
    if let Some(agent_answer) = crate::agent::search_and_learn(pool, query).await {
        if let Err(e) = sqlx::query("DELETE FROM knowledge_base WHERE published_at < NOW() - INTERVAL '7 days' AND title NOT LIKE 'Manual Input%'").execute(pool).await { 
            tracing::error!("[DATABASE ERROR] Failed to clean old knowledge base entries: {}", e); 
        }
        format!("\n[LIVE AGENT REPORT]:\n{}\n", agent_answer)
    } else {
        String::new()
    }
}

/// 🤖 Autonomous Background Vectorizer (Enterprise Batch Processing)
pub fn spawn_background_vectorizer(ctx: AppContext, token: CancellationToken) {
    tokio::spawn(async move {
        info!("[VECTORIZER] Enterprise Batch Worker started. Monitoring for new knowledge...");
        loop {
            tokio::select! {
                _ = token.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_secs(10)) => {
                    // PERFORMANCE PATCH: Fetch 50 records at once instead of 5
                    let mut backoff_ms = 1500;
                    let unindexed: Result<Vec<(i32, String)>, _> = sqlx::query_as(
                        "SELECT id, content FROM knowledge_base WHERE embedding IS NULL LIMIT 50"
                    ).fetch_all(&ctx.pool).await;

                    if let Ok(rows) = unindexed {
                        if rows.is_empty() {
                            continue;
                        }

                                        // [INJECTED] Dynamic .env Feature Flag
                if crate::state::get_setting(&ctx.pool, "ENABLE_AI_VECTORIZER", "true").await != "true" {
                    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                    continue; // Skip processing and check again later
                }
                info!("[VECTORIZER] Processing batch of {} documents...", rows.len());
                        let engine = &ctx.ai_engine;

                        for (id, content) in rows {
                            match engine.get_embedding(&content).await {
                                Ok(vector) => {
                                    let vec_str = format!("{:?}", vector);
                                    let res = sqlx::query("UPDATE knowledge_base SET embedding = $1::vector WHERE id = $2")
                                        .bind(vec_str)
                                        .bind(id)
                                        .execute(&ctx.pool).await;

                                    if let Err(e) = res {
                                        error!("[DATABASE ERROR] Failed to update document ID {}: {}", id, e);
                                    }
                                }
                                Err(e) => {
                                    error!("[VECTORIZER ERROR] Embedding failed for document ID {}: {}", id, e);
                // [INJECTED] API Throttle to prevent 429 Too Many Requests
                tokio::time::sleep(tokio::time::Duration::from_millis(backoff_ms)).await; backoff_ms = std::cmp::min(backoff_ms * 2, 60000);
                                }
                            }
                            // Optimized rate limiting: 100ms is sufficient for most enterprise APIs
                            tokio::time::sleep(Duration::from_millis(100)).await;
                        }
                        info!("[VECTORIZER] Batch processing complete.");
                    }
                }
            }
        }
    });
}
