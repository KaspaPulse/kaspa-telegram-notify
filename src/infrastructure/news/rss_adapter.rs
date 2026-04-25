use crate::domain::entities::NewsItem;
use crate::domain::errors::AppError;
use reqwest::Client;

pub struct RssAdapter {
    client: Client,
}

impl Default for RssAdapter {
    fn default() -> Self {
        Self::new()
    }
}

impl RssAdapter {
    pub fn new() -> Self {
        Self {
            client: Client::new(),
        }
    }
}

#[async_trait]
impl NewsProvider for RssAdapter {
    async fn fetch_news(&self, urls: Vec<String>) -> Result<Vec<NewsItem>, AppError> {
        let mut items = Vec::new();
        for url in urls {
            if let Ok(res) = self
                .client
                .get(&url)
                .header("User-Agent", "KaspaPulse/1.0")
                .send()
                .await
            {
                if let Ok(bytes) = res.bytes().await {
                    if let Ok(feed) = feed_rs::parser::parse(&bytes[..]) {
                        for entry in feed.entries {
                            let title = entry.title.map_or("Untitled".to_string(), |t| t.content);
                            let link = entry
                                .links
                                .first()
                                .map_or("".to_string(), |l| l.href.clone());
                            let content = entry.summary.map_or(title.clone(), |s| s.content);
                            items.push(NewsItem {
                                title,
                                link,
                                content,
                                source: url.clone(),
                            });
                        }
                    }
                }
            }
        }
        Ok(items)
    }
}

// --- Merged Trait (Formerly in ports) ---

use async_trait::async_trait;

#[async_trait]
pub trait NewsProvider: Send + Sync {
    async fn fetch_news(&self, urls: Vec<String>) -> Result<Vec<NewsItem>, AppError>;
}
