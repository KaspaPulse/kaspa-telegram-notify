// ==========================================
// ENTERPRISE CONSOLIDATED BACKGROUND JOBS
// ==========================================

use crate::infrastructure::database::postgres_adapter::PostgresRepository;
use crate::infrastructure::news::rss_adapter::NewsProvider;
use std::sync::Arc;

pub struct CrawlNewsUseCase {
    pub db: Arc<PostgresRepository>,
    pub news: Arc<dyn NewsProvider>,
}

impl CrawlNewsUseCase {
    pub fn new(db: Arc<PostgresRepository>, news: Arc<dyn NewsProvider>) -> Self {
        Self { db, news }
    }

    pub async fn execute(&self) {
        let urls = vec![
            "https://medium.com/feed/kaspa".to_string(),
            "https://kaspa.org/feed/".to_string(),
        ];
        if let Ok(feed) = self.news.fetch_news(urls).await {
            let mut new_items = 0;
            for item in feed {
                if let Ok(()) = self
                    .db
                    .add_to_knowledge_base(&item.title, &item.link, &item.content, &item.source)
                    .await
                {
                    new_items += 1;
                }
            }
            if new_items > 0 {
                tracing::info!(
                    "[RSS CRAWLER] Fetched feed. {} NEW items stored in Knowledge Base.",
                    new_items
                );
            } else {
                tracing::info!(
                    "[RSS CRAWLER] Cycle finished. No new items found (Database is up to date)."
                );
            }
        }
    }
}

use crate::infrastructure::ai::ai_engine_adapter::AiEngineAdapter;
use tracing::{error, info};

pub struct SystemTasksUseCase {
    db: Arc<PostgresRepository>,
    ai: Arc<AiEngineAdapter>,
}

impl SystemTasksUseCase {
    pub fn new(db: Arc<PostgresRepository>, ai: Arc<AiEngineAdapter>) -> Self {
        Self { db, ai }
    }

    pub async fn execute_memory_cleanup(&self) {
        info!("🧹 [MEMORY CLEANER] Starting enterprise garbage collection...");
        if let Err(e) = self.db.run_memory_cleaner().await {
            error!("[DATABASE ERROR] Failed to purge old chats: {}", e);
        } else {
            info!("✅ [MEMORY CLEANER] Garbage collection complete. RAM optimized.");
        }
    }

    pub async fn execute_ai_vectorizer(&self) {
        let is_enabled = self
            .db
            .get_setting("ENABLE_AI_VECTORIZER", "false")
            .await
            .unwrap_or_else(|_| "false".to_string());
        if is_enabled != "true" {
            tokio::time::sleep(std::time::Duration::from_secs(10)).await;
            return;
        }
        info!("🧩 [VECTORIZER] Checking for unindexed Kaspa knowledge...");
        match self.db.get_unindexed_knowledge(50).await {
            Ok(records) => {
                if records.is_empty() {
                    return;
                }
                info!(
                    "[VECTORIZER] Processing batch of {} documents...",
                    records.len()
                );
                for record in records.into_iter() {
                    let id = record.0;
                    let content: String = record.1;
                    match self.ai.get_embedding(&content).await {
                        Ok(vector) => {
                            let _ = self.db.update_knowledge_embedding(id, vector).await;
                        }
                        Err(e) => error!("[VECTORIZER ERROR] Embedding failed: {}", e),
                    }
                    // Prevent hitting API Rate Limits
                    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;
                }
            }
            Err(e) => error!(
                "[DATABASE ERROR] Failed to fetch unindexed knowledge: {}",
                e
            ),
        }
    }
}
