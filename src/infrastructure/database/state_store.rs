use dashmap::DashMap;
use sqlx::postgres::{PgPool, PgPoolOptions};
use std::collections::HashSet;
use tracing::{error, info};

/// Initializes an enterprise-grade connection pool for PostgreSQL.
pub async fn init_db(url: &str) -> Result<PgPool, sqlx::Error> {
    info!("[DATABASE] Initializing robust connection pool...");
    PgPoolOptions::new()
        .max_connections(20) // Handle high concurrency safely
        .min_connections(2) // Keep warm connections ready
        .acquire_timeout(std::time::Duration::from_secs(10)) // Prevent infinite hanging
        .connect(url)
        .await
}

/// Registers a new wallet tracking subscription.
pub async fn add_wallet_to_db(pool: &PgPool, wallet: &str, chat_id: i64) {
    if let Err(e) = sqlx::query!(
        "INSERT INTO user_wallets (wallet, chat_id) VALUES ($1, $2) ON CONFLICT (wallet, chat_id) DO UPDATE SET last_active = CURRENT_TIMESTAMP",
        wallet,
        chat_id
    )
    .execute(pool)
    .await
    {
        error!("[DATABASE ERROR] Failed to add wallet subscription: {}", e);
    }
}

/// Loads all active wallet subscriptions into the in-memory DashMap.
pub async fn load_state_from_db(
    pool: &PgPool,
    state: &DashMap<String, HashSet<i64>>,
) -> Result<(), sqlx::Error> {
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

/// Retrieves the latest DAA score for a wallet to determine the sync starting point.
pub async fn get_sync_checkpoint(pool: &PgPool, wallet: &str) -> u64 {
    sqlx::query_scalar!(
        "SELECT daa_score FROM mined_blocks WHERE wallet = $1 ORDER BY daa_score DESC LIMIT 1",
        wallet
    )
    .fetch_optional(pool)
    .await
    .unwrap_or(None)
    .map(|v| v as u64)
    .unwrap_or(0)
}

/// Checkpoint is naturally maintained by the latest daa_score in mined_blocks
pub async fn update_sync_checkpoint(_pool: &PgPool, _wallet: &str, _daa_score: u64) {}

/// Records a mined block using 'outpoint' to match the database schema.
pub async fn record_mined_block(
    pool: &PgPool,
    wallet: &str,
    outpoint: &str,
    amount: i64,
    daa_score: u64,
) {
    if let Err(e) = sqlx::query!(
        "INSERT INTO mined_blocks (wallet, outpoint, amount, daa_score) VALUES ($1, $2, $3, $4) ON CONFLICT (outpoint) DO NOTHING",
        wallet,
        outpoint,
        amount,
        daa_score as i64
    )
    .execute(pool)
    .await
    {
        error!("[DATABASE ERROR] Failed to record mined block: {}", e);
    }
}

/// Helper function required by sync.rs to record blocks during recovery.
pub async fn record_recovery_block(
    pool: &PgPool,
    wallet: &str,
    outpoint: &str,
    amount: i64,
    daa_score: u64,
) {
    record_mined_block(pool, wallet, outpoint, amount, daa_score).await;
}
