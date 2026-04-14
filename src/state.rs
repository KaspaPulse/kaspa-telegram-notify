#![allow(dead_code)]
use sqlx::{sqlite::SqlitePoolOptions, SqlitePool};
use std::sync::Arc;
use dashmap::DashMap;
use std::collections::HashSet;

pub type SharedState = Arc<DashMap<String, HashSet<i64>>>;

pub async fn init_db() -> anyhow::Result<SqlitePool> {
    let db_url = "sqlite://kaspa_bot.db";
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .connect(db_url)
        .await?;

    // إنشاء الجداول برمجياً إذا لم تكن موجودة
    sqlx::query(
        "CREATE TABLE IF NOT EXISTS tracked_wallets (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            wallet TEXT NOT NULL,
            chat_id INTEGER NOT NULL,
            UNIQUE(wallet, chat_id)
        );"
    ).execute(&pool).await?;

    sqlx::query(
        "CREATE TABLE IF NOT EXISTS chat_history (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            chat_id INTEGER NOT NULL,
            role TEXT NOT NULL,
            message TEXT NOT NULL,
            timestamp DATETIME DEFAULT CURRENT_TIMESTAMP
        );"
    ).execute(&pool).await?;

    Ok(pool)
}

pub async fn load_state(pool: &SqlitePool) -> anyhow::Result<SharedState> {
    let rows: Vec<(String, i64)> = sqlx::query_as("SELECT wallet, chat_id FROM tracked_wallets")
        .fetch_all(pool).await?;
    
    let map = Arc::new(DashMap::new());
    for (wallet, chat_id) in rows {
        map.entry(wallet).or_insert_with(HashSet::new).insert(chat_id);
    }
    Ok(map)
}

pub async fn add_wallet_to_db(pool: &SqlitePool, wallet: &str, chat_id: i64) {
    let _ = sqlx::query("INSERT OR IGNORE INTO tracked_wallets (wallet, chat_id) VALUES (?1, ?2)")
        .bind(wallet)
        .bind(chat_id)
        .execute(pool)
        .await;
}

pub async fn remove_wallet_from_db(pool: &SqlitePool, wallet: &str, chat_id: i64) {
    let _ = sqlx::query("DELETE FROM tracked_wallets WHERE wallet = ?1 AND chat_id = ?2")
        .bind(wallet)
        .bind(chat_id)
        .execute(pool)
        .await;
}

pub async fn remove_all_user_data(pool: &SqlitePool, state: &SharedState, chat_id: i64) {
    let _ = sqlx::query("DELETE FROM tracked_wallets WHERE chat_id = ?1").bind(chat_id).execute(pool).await;
    state.retain(|_, v| { v.remove(&chat_id); !v.is_empty() });
}
