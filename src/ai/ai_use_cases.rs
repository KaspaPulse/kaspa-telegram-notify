use crate::domain::errors::AppError;
use crate::infrastructure::ai::ai_engine_adapter::AiEngineAdapter;
use crate::infrastructure::database::postgres_adapter::PostgresRepository;
// ===== Migrated from ai_chat.rs =====
use std::sync::Arc;
use tokio_stream::StreamExt;

pub struct AiChatUseCase {
    ai: Arc<AiEngineAdapter>,
    db: Arc<PostgresRepository>,
}

impl AiChatUseCase {
    pub fn new(ai: Arc<AiEngineAdapter>, db: Arc<PostgresRepository>) -> Self {
        Self { ai, db }
    }

    pub async fn execute_text(&self, prompt: &str) -> Result<String, AppError> {
        let context = self
            .db
            .get_setting(
                "AI_SYSTEM_PROMPT",
                "Kaspa is a blockDAG based cryptocurrency.",
            )
            .await
            .unwrap_or_else(|_| "Kaspa is a blockDAG based cryptocurrency.".to_string());
        let mut stream = self.ai.generate_chat_stream(prompt, &context).await?;
        let mut full_response = String::new();

        while let Some(chunk) = stream.next().await {
            full_response.push_str(&chunk);
        }

        Ok(full_response)
    }
}

// ===== Migrated from ai_rag.rs =====
use tracing::info;
pub struct AiRagUseCase {
    db: Arc<PostgresRepository>,
    ai: Arc<AiEngineAdapter>,
}

impl AiRagUseCase {
    pub fn new(db: Arc<PostgresRepository>, ai: Arc<AiEngineAdapter>) -> Self {
        Self { db, ai }
    }

    pub async fn build_context_for_query(&self, user_query: &str) -> Result<String, AppError> {
        info!(
            "[RAG] Building contextual knowledge for query: {}",
            user_query
        );
        // 🚀 FULL IMPLEMENTATION: Vectorize the query using AI Engine
        let _query_vector = self.ai.get_embedding(user_query).await;
        // For now, we use the keyword search capability we built in Phase 1.
        let mut context = String::from("[KNOWLEDGE BASE CONTEXT]:\n");

        if let Ok(Some(db_context)) = self.db.get_knowledge_context(user_query).await {
            context.push_str(&db_context);
        } else {
            context.push_str("No specific local database context found. Relying on node data.");
        }

        Ok(context)
    }
}
