use dashmap::DashMap;
use sqlx::sqlite::{SqlitePool, SqlitePoolOptions};
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{error, info};

pub type SharedState = Arc<DashMap<String, HashSet<i64>>>;
pub type UtxoState = Arc<DashMap<String, HashSet<String>>>;

pub async fn init_db() -> Result<SqlitePool, sqlx::Error> {
    let pool = SqlitePoolOptions::new()
        .max_connections(10)
        .connect("sqlite://enterprise.db?mode=rwc")
        .await?;

    // Safely enable WAL mode using explicit PRAGMA queries (sqlx compliant)
    let _ = sqlx::query("PRAGMA journal_mode = WAL;")
        .execute(&pool)
        .await;
    let _ = sqlx::query("PRAGMA synchronous = NORMAL;")
        .execute(&pool)
        .await;
    let _ = sqlx::query("PRAGMA busy_timeout = 5000;")
        .execute(&pool)
        .await;

    // 1. Wallets table
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS user_wallets (
            wallet TEXT NOT NULL,
            chat_id INTEGER NOT NULL,
            PRIMARY KEY (wallet, chat_id)
        )",
    )
    .execute(&pool)
    .await?;

    // 2. Mined blocks table (with sync_source)
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS mined_blocks (
            outpoint TEXT PRIMARY KEY,
            wallet TEXT NOT NULL,
            amount REAL NOT NULL,
            daa_score INTEGER NOT NULL,
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP,
            sync_source TEXT DEFAULT 'LIVE'
        )",
    )
    .execute(&pool)
    .await?;

    // Safe schema update for older databases
    let _ = sqlx::query("ALTER TABLE mined_blocks ADD COLUMN sync_source TEXT DEFAULT 'LIVE'")
        .execute(&pool)
        .await;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_wallet_stats ON mined_blocks(wallet, timestamp)")
        .execute(&pool)
        .await?;

    // 3. Sync checkpoint table
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS sync_checkpoint (
            wallet TEXT PRIMARY KEY,
            last_daa_score INTEGER NOT NULL
        )",
    )
    .execute(&pool)
    .await?;

    // 4. AI Knowledge Base (RAG System)
    sqlx::query(
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
    .await?;

    Ok(pool)
}

// 📌 Used by the live monitor
pub async fn record_mined_block(
    pool: &SqlitePool,
    outpoint: &str,
    wallet: &str,
    amount: f64,
    daa: u64,
) {
    if let Err(e) = sqlx::query(
        "INSERT OR IGNORE INTO mined_blocks (outpoint, wallet, amount, daa_score, sync_source) VALUES (?1, ?2, ?3, ?4, 'LIVE')"
    )
    .bind(outpoint).bind(wallet).bind(amount).bind(daa as i64)
    .execute(pool).await {
        error!("[DB ERROR] Block record failed: {}", e);
    }
}

// 📌 Dedicated to the Admin sync process
pub async fn record_recovery_block(
    pool: &SqlitePool,
    outpoint: &str,
    wallet: &str,
    amount: f64,
    daa: u64,
) {
    if let Err(e) = sqlx::query(
        "INSERT OR IGNORE INTO mined_blocks (outpoint, wallet, amount, daa_score, sync_source) VALUES (?1, ?2, ?3, ?4, 'RECOVERY')"
    )
    .bind(outpoint).bind(wallet).bind(amount).bind(daa as i64)
    .execute(pool).await {
        error!("[DB ERROR] Recovery block record failed: {}", e);
    }
}

pub async fn get_sync_checkpoint(pool: &SqlitePool, wallet: &str) -> u64 {
    let result: Option<i64> =
        sqlx::query_scalar("SELECT last_daa_score FROM sync_checkpoint WHERE wallet = ?1")
            .bind(wallet)
            .fetch_optional(pool)
            .await
            .unwrap_or(None);

    result.unwrap_or(0) as u64
}

pub async fn update_sync_checkpoint(pool: &SqlitePool, wallet: &str, daa_score: u64) {
    let _ = sqlx::query(
        "INSERT INTO sync_checkpoint (wallet, last_daa_score) VALUES (?1, ?2)
         ON CONFLICT(wallet) DO UPDATE SET last_daa_score = excluded.last_daa_score",
    )
    .bind(wallet)
    .bind(daa_score as i64)
    .execute(pool)
    .await;
}

pub async fn get_lifetime_stats(
    pool: &SqlitePool,
    wallet: &str,
) -> Result<(i64, f64), sqlx::Error> {
    let row: (i64, f64) = sqlx::query_as(
        "SELECT COUNT(*), COALESCE(SUM(amount), 0.0) FROM mined_blocks WHERE wallet = ?1",
    )
    .bind(wallet)
    .fetch_one(pool)
    .await?;
    Ok(row)
}

pub async fn load_state_from_db(pool: &SqlitePool, state: &SharedState) -> Result<(), sqlx::Error> {
    let rows: Vec<(String, i64)> = sqlx::query_as("SELECT wallet, chat_id FROM user_wallets")
        .fetch_all(pool)
        .await?;
    for (wallet, chat_id) in rows {
        state
            .entry(wallet)
            .or_insert_with(HashSet::new)
            .insert(chat_id);
    }
    info!("[DB] Synchronized {} active wallets.", state.len());
    Ok(())
}

pub async fn add_wallet_to_db(pool: &SqlitePool, wallet: &str, chat_id: i64) {
    let _ = sqlx::query("INSERT OR IGNORE INTO user_wallets (wallet, chat_id) VALUES (?1, ?2)")
        .bind(wallet)
        .bind(chat_id)
        .execute(pool)
        .await;
}

pub async fn remove_wallet_from_db(pool: &SqlitePool, wallet: &str, chat_id: i64) {
    let _ = sqlx::query("DELETE FROM user_wallets WHERE wallet = ?1 AND chat_id = ?2")
        .bind(wallet)
        .bind(chat_id)
        .execute(pool)
        .await;
}

pub async fn remove_all_user_data(pool: &SqlitePool, state: &SharedState, chat_id: i64) {
    let _ = sqlx::query("DELETE FROM user_wallets WHERE chat_id = ?1")
        .bind(chat_id)
        .execute(pool)
        .await;
    let mut empty_wallets = Vec::new();
    for mut entry in state.iter_mut() {
        entry.value_mut().remove(&chat_id);
        if entry.value().is_empty() {
            empty_wallets.push(entry.key().clone());
        }
    }
    for wallet in empty_wallets {
        state.remove(&wallet);
    }
}
