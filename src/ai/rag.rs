use sqlx::{PgPool, Postgres};
use tracing::{error, info};
use crate::context::AppContext;
use tokio::time::Duration;
use tokio_util::sync::CancellationToken;

/// Enterprise RAG Engine: Semantic Vector Search
pub async fn get_rag_context(pool: &PgPool, user_query: &str, engine: &crate::ai::LocalAiEngine) -> String {
    info!("🧠 [RAG] Semantic Search INITIATED for: '{}'", user_query);

    // 1. Convert user query to vector
    let query_vector = match engine.get_embedding(user_query).await {
        Ok(v) => v,
        Err(e) => {
            error!("⚠️ [RAG] Vectorization failed: {}. Falling back to Agent.", e);
            return trigger_autonomous_agent(pool, user_query).await;
        }
    };

    // 2. Format vector for pgvector '[0.1, 0.2, ...]'
    let vector_str = format!("{:?}", query_vector);

    // 3. Search via Cosine Distance (<=>)
    let articles: Result<Vec<(String, String)>, _> = sqlx::query_as::<Postgres, (String, String)>(
        "SELECT title, content FROM knowledge_base 
         WHERE embedding IS NOT NULL 
         ORDER BY embedding <=> $1::vector 
         LIMIT 3"
    )
    .bind(vector_str)
    .fetch_all(pool)
    .await;

    if let Ok(results) = articles {
        if !results.is_empty() {
            info!("✅ [RAG] Semantic Match Found. Injecting context.");
            let mut context = String::from("\n[INTERNAL SERVER PROTOCOLS & DATA]:\n");
            for (title, content) in results {
                let snippet = if content.len() > 800 { &content[..800] } else { &content };
                context.push_str(&format!("- {}: {}\n", title, snippet));
            }
            return context;
        }
    }

    info!("🌐 [RAG] Local search silent. Engaging Autonomous Agent.");
    trigger_autonomous_agent(pool, user_query).await
}

async fn trigger_autonomous_agent(pool: &PgPool, query: &str) -> String {
    if let Some(agent_answer) = crate::agent::search_and_learn(pool, query).await {
        let _ = sqlx::query("DELETE FROM knowledge_base WHERE published_at < NOW() - INTERVAL '7 days' AND title NOT LIKE 'Manual Input%'").execute(pool).await;
        format!("\n[LIVE AGENT REPORT]:\n{}\n", agent_answer)
    } else {
        String::new()
    }
}

/// 🤖 Autonomous Background Vectorizer (Enterprise Microservice)
pub fn spawn_background_vectorizer(ctx: AppContext, token: CancellationToken) {
    tokio::spawn(async move {
        info!("⚙️ [VECTORIZER] Background worker started. Scanning for raw knowledge...");
        loop {
            tokio::select! {
                _ = token.cancelled() => break,
                _ = tokio::time::sleep(Duration::from_secs(15)) => {
                    // Find up to 5 rows missing embeddings
                    let unindexed: Result<Vec<(i32, String)>, _> = sqlx::query_as(
                        "SELECT id, content FROM knowledge_base WHERE embedding IS NULL LIMIT 5"
                    ).fetch_all(&ctx.pool).await;

                    if let Ok(rows) = unindexed {
                        if !rows.is_empty() {
                            let engine = ctx.ai_engine.lock().await;
                            for (id, content) in rows {
                                if let Ok(vector) = engine.get_embedding(&content).await {
                                    let vec_str = format!("{:?}", vector);
                                    let _ = sqlx::query("UPDATE knowledge_base SET embedding = $1::vector WHERE id = $2")
                                        .bind(vec_str)
                                        .bind(id)
                                        .execute(&ctx.pool).await;
                                    info!("✅ [VECTORIZER] Embedded & Indexed Document ID: {}", id);
                                }
                                tokio::time::sleep(Duration::from_millis(500)).await; // Rate limit safety
                            }
                        }
                    }
                }
            }
        }
    });
}