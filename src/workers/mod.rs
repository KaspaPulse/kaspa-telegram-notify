pub mod live;
pub mod sync;
pub mod system;
pub mod rss;

use crate::context::AppContext;
use teloxide::prelude::*;
use tokio_util::sync::CancellationToken;

// Re-export specific functions so handlers.rs and main.rs don't break
pub use sync::{sync_all_wallets_from_pruning_point, sync_single_wallet};

/// Master spawner that triggers all background services
pub fn start_all(ctx: AppContext, bot: Bot, token: CancellationToken) {
    // 1. Core System Monitors
    system::spawn_price_monitor(ctx.clone(), token.clone());
    system::spawn_node_monitor(ctx.clone(), bot.clone(), token.clone());
    live::spawn_utxo_monitor(ctx.clone(), bot, token.clone());
    system::spawn_memory_cleaner(ctx.clone(), token.clone());
    
    // 2. 🕸️ Start RSS Crawler for Dynamic AI Knowledge Base (RAG)
    rss::spawn_rss_crawler(ctx.pool.clone(), token.clone());
    
    // 3. 🧠 Start Autonomous Vectorizer (Indexes DB for Semantic Search)
    crate::ai::rag::spawn_background_vectorizer(ctx, token);
}