use chrono::{DateTime, Utc};
use reqwest::Client;
use sqlx::SqlitePool;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};

// Authoritative Kaspa Knowledge Sources
const RSS_SOURCES: &[&str] = &[
    "https://medium.com/feed/@kaspa-currency", // Official Kaspa Blog
    "https://github.com/kaspanet/rusty-kaspa/releases.atom", // Core Node Releases
];

pub fn spawn_rss_crawler(pool: SqlitePool, token: CancellationToken) {
    tokio::spawn(async move {
        // 1. Initialize the Knowledge Base table if it doesn't exist
        let _ = sqlx::query(
            "CREATE TABLE IF NOT EXISTS knowledge_base (
                id INTEGER PRIMARY KEY AUTOINCREMENT,
                title TEXT NOT NULL,
                link TEXT UNIQUE NOT NULL,
                content TEXT NOT NULL,
                source TEXT NOT NULL,
                published_at DATETIME
            )",
        )
        .execute(&pool)
        .await;

        info!("🕸️ [RSS CRAWLER] Initialized and active. Preparing to fetch Kaspa intelligence...");

        let client = Client::builder()
            .timeout(Duration::from_secs(15))
            .build()
            .unwrap_or_default();

        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    info!("🛑 [RSS CRAWLER] Shutting down safely.");
                    break;
                }
                _ = fetch_and_store_feeds(&pool, &client) => {
                    info!("⏳ [RSS CRAWLER] Cycle complete. Sleeping for 12 hours...");
                    // Execute crawler every 12 hours to prevent rate-limiting
                    tokio::time::sleep(Duration::from_secs(12 * 3600)).await;
                }
            }
        }
    });
}

async fn fetch_and_store_feeds(pool: &SqlitePool, client: &Client) {
    let mut total_new_articles = 0;

    for &url in RSS_SOURCES {
        match client.get(url).send().await {
            Ok(response) => {
                if let Ok(bytes) = response.bytes().await {
                    match feed_rs::parser::parse(&bytes[..]) {
                        Ok(feed) => {
                            for entry in feed.entries {
                                let title =
                                    entry.title.map_or("Untitled".to_string(), |t| t.content);
                                let link = entry
                                    .links
                                    .first()
                                    .map_or("".to_string(), |l| l.href.clone());

                                // Extract full content or fallback to summary
                                let content = if let Some(content) = entry.content {
                                    content.body.unwrap_or_default()
                                } else if let Some(summary) = entry.summary {
                                    summary.content
                                } else {
                                    "".to_string()
                                };

                                let published_at: Option<DateTime<Utc>> =
                                    entry.published.map(|d| d.into());

                                if link.is_empty() || content.is_empty() {
                                    continue;
                                }

                                // 2. Store the article (Ignore duplicates via UNIQUE constraint)
                                let result = sqlx::query(
                                    "INSERT OR IGNORE INTO knowledge_base (title, link, content, source, published_at) 
                                     VALUES (?1, ?2, ?3, ?4, ?5)"
                                )
                                .bind(&title)
                                .bind(&link)
                                .bind(&content)
                                .bind(url)
                                .bind(published_at)
                                .execute(pool)
                                .await;

                                if let Ok(res) = result {
                                    if res.rows_affected() > 0 {
                                        total_new_articles += 1;
                                        info!(
                                            "🧠 [LEARNING] New Kaspa knowledge acquired: {}",
                                            title
                                        );
                                    }
                                }
                            }
                        }
                        Err(e) => warn!("⚠️ [RSS CRAWLER] Failed to parse XML from {}: {}", url, e),
                    }
                }
            }
            Err(e) => error!("❌ [RSS CRAWLER] Network error fetching {}: {}", url, e),
        }
    }

    if total_new_articles > 0 {
        info!(
            "✅ [RSS CRAWLER] Database updated with {} new articles.",
            total_new_articles
        );
    } else {
        info!("💤 [RSS CRAWLER] No new articles found. Database is up to date.");
    }
}
