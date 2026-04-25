use chrono::{DateTime, Utc};
use reqwest::header::{ACCEPT, USER_AGENT};
use reqwest::Client;
use sqlx::PgPool;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

// ✅ REMOVED: const RSS_SOURCES array to avoid hardcoded infrastructure configurations.

pub fn spawn_rss_crawler(pool: PgPool, token: CancellationToken) {
    tokio::spawn(async move {
        let client = Client::builder()
            .timeout(Duration::from_secs(30))
            .build()
            .unwrap_or_default();

        loop {
            tokio::select! {
                _ = token.cancelled() => { break; }
                _ = fetch_and_store_feeds(&pool, &client) => {
                    info!("[RSS] Cycle finished. Sleeping for 6 hours...");
                    tokio::time::sleep(Duration::from_secs(6 * 3600)).await;
                }
            }
        }
    });
}

async fn fetch_and_store_feeds(pool: &PgPool, client: &Client) {
    // ✅ Enterprise Patch: Read URLs dynamically from the environment
    // If the .env is missing or the variable isn't set, it falls back safely to the default URLs.
    let feeds_env = std::env::var("RSS_FEEDS")
        .expect("CRITICAL SECURITY: RSS_FEEDS must be explicitly defined in .env!");

    // Split the comma-separated string into a list of valid URLs
    let rss_sources: Vec<&str> = feeds_env
        .split(',')
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .collect();

    for url in rss_sources {
        let request = client
            .get(url)
            .header(
                USER_AGENT,
                "Mozilla/5.0 (Windows NT 10.0; Win64; x64) KaspaPulse/1.0",
            )
            .header(ACCEPT, "application/rss+xml, application/xml");

        if let Ok(response) = request.send().await {
            if let Ok(bytes) = response.bytes().await {
                if let Ok(feed) = feed_rs::parser::parse(&bytes[..]) {
                    for entry in feed.entries {
                        let title = entry.title.map_or("Untitled".to_string(), |t| t.content);
                        let link = entry
                            .links
                            .first()
                            .map_or("".to_string(), |l| l.href.clone());
                        let content = entry
                            .summary
                            .map(|s| s.content)
                            .unwrap_or_else(|| title.clone());
                        let published_at: Option<DateTime<Utc>> = entry.published;

                        if let Err(e) = sqlx::query(
                            "INSERT INTO knowledge_base (title, link, content, source, published_at) VALUES ($1, $2, $3, $4, $5) ON CONFLICT (link) DO NOTHING"
                        )
                        .bind(&title).bind(&link).bind(&content).bind(url).bind(published_at)
                        .execute(pool).await {
                            tracing::error!("[DATABASE ERROR] Query execution failed: {}", e);
                        }
                    }
                }
            }
        } else {
            error!("[RSS] Network failure fetching source: {}", url);
        }
    }
}
