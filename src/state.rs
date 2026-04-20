use dashmap::DashMap;
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{error, info};

/// Shared memory state for wallet tracking
pub type SharedState = Arc<DashMap<String, HashSet<i64>>>;
/// Shared memory state for UTXO tracking
pub type UtxoState = Arc<DashMap<String, HashSet<String>>>;

/// Initializes the database connection pool and runs schema migrations.
pub async fn init_db(db_url: &str) -> Result<PgPool, sqlx::Error> {
    let pool = PgPoolOptions::new()
        .max_connections(50)
        .connect(db_url)
        .await?;

    // Initialize user_wallets table
    sqlx::query!(
        "CREATE TABLE IF NOT EXISTS user_wallets (
            wallet VARCHAR(255) NOT NULL,
            chat_id BIGINT NOT NULL,
            last_active TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (wallet, chat_id)
        )"
    )
    .execute(&pool)
    .await?;

    // Safe migration for the last_active column
    if let Err(e) = sqlx::query!(
        "ALTER TABLE user_wallets ADD COLUMN IF NOT EXISTS last_active TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP"
    )
    .execute(&pool)
    .await { tracing::error!("[DATABASE ERROR] Query execution failed: {}", e); }

    // Initialize mined_blocks table
    sqlx::query!(
        "CREATE TABLE IF NOT EXISTS mined_blocks (
            outpoint VARCHAR(255) PRIMARY KEY,
            wallet VARCHAR(255) NOT NULL,
            amount DOUBLE PRECISION NOT NULL,
            daa_score BIGINT NOT NULL,
            timestamp TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
            sync_source VARCHAR(50) DEFAULT 'LIVE'
        )"
    )
    .execute(&pool)
    .await?;

    // Create index for faster analytics queries
    sqlx::query!("CREATE INDEX IF NOT EXISTS idx_wallet_stats ON mined_blocks(wallet, timestamp)")
        .execute(&pool)
        .await?;

    // Initialize sync_checkpoint table
    sqlx::query!("CREATE TABLE IF NOT EXISTS sync_checkpoint (wallet VARCHAR(255) PRIMARY KEY, last_daa_score BIGINT NOT NULL)")
        .execute(&pool)
        .await?;

    // Initialize knowledge_base table for AI context
    sqlx::query!("CREATE TABLE IF NOT EXISTS knowledge_base (
            id SERIAL PRIMARY KEY, 
            title TEXT NOT NULL, 
            link TEXT UNIQUE NOT NULL, 
            content TEXT NOT NULL, 
            source TEXT NOT NULL, 
            published_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
        )")
        .execute(&pool)
        .await?;
        
    // Initialize chat_history table for AI memory
    sqlx::query!(
        "CREATE TABLE IF NOT EXISTS chat_history (
            id SERIAL PRIMARY KEY,
            chat_id BIGINT NOT NULL,
            role VARCHAR(20) NOT NULL,
            message TEXT NOT NULL,
            timestamp TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
        )"
    )
    .execute(&pool)
    .await?;
    
    // Initialize system_settings table for dynamic configuration
    sqlx::query!(
        "CREATE TABLE IF NOT EXISTS system_settings (
            key_name VARCHAR(50) PRIMARY KEY,
            value_data TEXT NOT NULL,
            updated_at TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP
        )"
    )
    .execute(&pool)
    .await?;

    Ok(pool)
}

/// Updates the last active timestamp for a user to prevent data pruning.
#[allow(dead_code)]
pub async fn update_user_activity(pool: &PgPool, chat_id: i64) {
    if let Err(e) = sqlx::query!(
        "UPDATE user_wallets SET last_active = CURRENT_TIMESTAMP WHERE chat_id = $1",
        chat_id
    )
    .execute(pool)
    .await {
        error!("[DATABASE ERROR] Failed to update user activity: {}", e);
    }
}

/// Enforces GDPR/Privacy data retention policies by removing inactive records.
#[allow(dead_code)]
pub async fn enforce_retention_policy(pool: &PgPool) {
    match sqlx::query!(
        "DELETE FROM user_wallets WHERE last_active < CURRENT_TIMESTAMP - INTERVAL '90 days'"
    )
    .execute(pool)
    .await
    {
        Ok(res) => {
            if res.rows_affected() > 0 {
                info!(
                    "[PRIVACY] Retention Policy Enforced: Deleted {} inactive user linkages.",
                    res.rows_affected()
                );
            }
        }
        Err(e) => error!("[PRIVACY ERROR] Failed to enforce retention policy: {}", e),
    }
}

/// Records a newly mined block from the live node stream.
pub async fn record_mined_block(pool: &PgPool, outpoint: &str, wallet: &str, amount: f64, daa: u64) {
    let daa_i64 = daa as i64;
    if let Err(e) = sqlx::query!(
        "INSERT INTO mined_blocks (outpoint, wallet, amount, daa_score, sync_source) 
         VALUES ($1, $2, $3, $4, 'LIVE') ON CONFLICT (outpoint) DO NOTHING",
        outpoint, wallet, amount, daa_i64
    )
    .execute(pool)
    .await { 
        error!("[DATABASE ERROR] Failed to record mined block: {}", e); 
    }
}

/// Records a historically recovered block from a node sync operation.
pub async fn record_recovery_block(pool: &PgPool, outpoint: &str, wallet: &str, amount: f64, daa: u64) {
    let daa_i64 = daa as i64;
    if let Err(e) = sqlx::query!(
        "INSERT INTO mined_blocks (outpoint, wallet, amount, daa_score, sync_source) 
         VALUES ($1, $2, $3, $4, 'RECOVERY') ON CONFLICT (outpoint) DO NOTHING",
        outpoint, wallet, amount, daa_i64
    )
    .execute(pool)
    .await { 
        error!("[DATABASE ERROR] Failed to record recovery block: {}", e); 
    }
}

/// Retrieves the highest DAA score synced for a given wallet.
pub async fn get_sync_checkpoint(pool: &PgPool, wallet: &str) -> u64 {
    sqlx::query_scalar!(
        "SELECT last_daa_score FROM sync_checkpoint WHERE wallet = $1",
        wallet
    )
    .fetch_optional(pool)
    .await
    .unwrap_or(None)
    .unwrap_or(0) as u64
}

/// Updates the sync checkpoint for a wallet after a successful node scan.
pub async fn update_sync_checkpoint(pool: &PgPool, wallet: &str, daa_score: u64) {
    let daa_i64 = daa_score as i64;
    if let Err(e) = sqlx::query!(
        "INSERT INTO sync_checkpoint (wallet, last_daa_score) VALUES ($1, $2) 
         ON CONFLICT (wallet) DO UPDATE SET last_daa_score = EXCLUDED.last_daa_score",
        wallet, daa_i64
    )
    .execute(pool)
    .await { 
        error!("[DATABASE ERROR] Failed to update checkpoint: {}", e); 
    }
}

/// Aggregates lifetime mining statistics for a specific wallet.
pub async fn get_lifetime_stats(pool: &PgPool, wallet: &str) -> Result<(i64, f64), sqlx::Error> {
    let res = sqlx::query!(
        "SELECT COUNT(*) as count, COALESCE(SUM(amount), 0.0) as sum FROM mined_blocks WHERE wallet = $1",
        wallet
    )
    .fetch_one(pool)
    .await?;

    Ok((res.count.unwrap_or(0), res.sum.unwrap_or(0.0)))
}

/// Loads all active wallet subscriptions into the in-memory DashMap.
pub async fn load_state_from_db(pool: &PgPool, state: &SharedState) -> Result<(), sqlx::Error> {
    let rows = sqlx::query!("SELECT wallet, chat_id FROM user_wallets")
        .fetch_all(pool)
        .await?;

    for row in rows {
        state
            .entry(row.wallet)
            .or_insert_with(HashSet::new)
            .insert(row.chat_id);
    }
    info!("[SYSTEM] Synchronized {} active wallets from database.", state.len());
    Ok(())
}

/// Registers a new wallet tracking subscription.
pub async fn add_wallet_to_db(pool: &PgPool, wallet: &str, chat_id: i64) {
    if let Err(e) = sqlx::query!(
        "INSERT INTO user_wallets (wallet, chat_id) VALUES ($1, $2) 
         ON CONFLICT (wallet, chat_id) DO UPDATE SET last_active = CURRENT_TIMESTAMP",
        wallet, chat_id
    )
    .execute(pool)
    .await { 
        error!("[DATABASE ERROR] Failed to add wallet subscription: {}", e); 
    }
}

/// Removes a specific wallet subscription for a user.
pub async fn remove_wallet_from_db(pool: &PgPool, wallet: &str, chat_id: i64) {
    if let Err(e) = sqlx::query!(
        "DELETE FROM user_wallets WHERE wallet = $1 AND chat_id = $2",
        wallet, chat_id
    )
    .execute(pool)
    .await { 
        error!("[DATABASE ERROR] Failed to remove wallet subscription: {}", e); 
    }
}

/// Completely purges all tracking data associated with a specific user.
pub async fn remove_all_user_data(pool: &PgPool, _state: &SharedState, chat_id: i64) {
    if let Err(e) = sqlx::query!("DELETE FROM user_wallets WHERE chat_id = $1", chat_id)
        .execute(pool)
        .await { 
            error!("[DATABASE ERROR] Failed to wipe user data: {}", e); 
        }
}

// --- AI KNOWLEDGE BASE EXTENSIONS ---

/// Indexes a new article or fact into the AI's Retrieval-Augmented Generation (RAG) database.
pub async fn add_to_knowledge_base(pool: &PgPool, title: &str, link: &str, content: &str, source: &str) {
    if let Err(e) = sqlx::query!(
        "INSERT INTO knowledge_base (title, link, content, source) 
         VALUES ($1, $2, $3, $4) ON CONFLICT (link) DO NOTHING",
        title, link, content, source
    )
    .execute(pool)
    .await { 
        error!("[DATABASE ERROR] Failed to index knowledge base entry: {}", e); 
    }
}

/// Performs a basic semantic keyword search within the RAG knowledge base.
#[allow(dead_code)]
pub async fn get_knowledge_context(pool: &PgPool, keyword: &str) -> Option<String> {
    let search_term = format!("%{}%", keyword);
    sqlx::query_scalar!(
        "SELECT content FROM knowledge_base 
         WHERE title ILIKE $1 OR content ILIKE $1 
         ORDER BY published_at DESC LIMIT 1",
        search_term
    )
    .fetch_optional(pool)
    .await
    .unwrap_or(None)
}

// --- CACHE LAYER FOR SETTINGS (O(1) Memory Access) ---
static SETTINGS_CACHE: std::sync::OnceLock<dashmap::DashMap<String, String>> = std::sync::OnceLock::new();

fn get_settings_cache() -> &'static dashmap::DashMap<String, String> {
    SETTINGS_CACHE.get_or_init(|| dashmap::DashMap::new())
}

/// Retrieves a dynamic setting from the cache or database, initializing it if needed.
pub async fn get_setting(pool: &sqlx::PgPool, key: &str, default: &str) -> String {
    let cache = get_settings_cache();
    if let Some(val) = cache.get(key) {
        return val.clone();
    }

    let res: Option<String> = sqlx::query_scalar("SELECT value_data FROM system_settings WHERE key_name = $1")
        .bind(key)
        .fetch_optional(pool)
        .await
        .unwrap_or(None);

    let final_val = match res {
        Some(val) => val,
        None => {
            let _ = sqlx::query("INSERT INTO system_settings (key_name, value_data) VALUES ($1, $2) ON CONFLICT DO NOTHING")
                .bind(key)
                .bind(default)
                .execute(pool).await;
            default.to_string()
        }
    };

    cache.insert(key.to_string(), final_val.clone());
    final_val
}

/// Safely updates a dynamic setting in the database and invalidates/updates the cache.
pub async fn update_setting(pool: &sqlx::PgPool, key: &str, value: &str) -> Result<(), sqlx::Error> {
    sqlx::query("INSERT INTO system_settings (key_name, value_data) VALUES ($1, $2) ON CONFLICT (key_name) DO UPDATE SET value_data = EXCLUDED.value_data, updated_at = CURRENT_TIMESTAMP")
        .bind(key)
        .bind(value)
        .execute(pool).await?;

    let cache = get_settings_cache();
    cache.insert(key.to_string(), value.to_string());

    Ok(())
}