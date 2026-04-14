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

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS user_wallets (
            wallet TEXT NOT NULL,
            chat_id INTEGER NOT NULL,
            PRIMARY KEY (wallet, chat_id)
        )",
    )
    .execute(&pool)
    .await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS mined_blocks (
            outpoint TEXT PRIMARY KEY,
            wallet TEXT NOT NULL,
            amount REAL NOT NULL,
            daa_score INTEGER NOT NULL,
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
        )",
    )
    .execute(&pool)
    .await?;

    sqlx::query("CREATE INDEX IF NOT EXISTS idx_wallet_stats ON mined_blocks(wallet, timestamp)")
        .execute(&pool)
        .await?;

    Ok(pool)
}

pub async fn record_mined_block(
    pool: &SqlitePool,
    outpoint: &str,
    wallet: &str,
    amount: f64,
    daa: u64,
) {
    if let Err(e) = sqlx::query(
        "INSERT OR IGNORE INTO mined_blocks (outpoint, wallet, amount, daa_score) VALUES (?1, ?2, ?3, ?4)"
    )
    .bind(outpoint).bind(wallet).bind(amount).bind(daa as i64)
    .execute(pool).await {
        error!("[DB ERROR] Block record failed: {}", e);
    }
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
