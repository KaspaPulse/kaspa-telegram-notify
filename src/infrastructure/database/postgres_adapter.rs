use crate::domain::entities::{MinedBlock, TrackedWallet};
use crate::domain::errors::AppError;
use sqlx::postgres::PgPool;

pub struct PostgresRepository {
    pool: PgPool,
}

impl PostgresRepository {
    pub fn new(pool: PgPool) -> Self {
        Self { pool }
    }
}

// Implementing the Port from the Domain Layer with REAL SQLx queries
impl PostgresRepository {
    pub async fn add_tracked_wallet(&self, wallet: TrackedWallet) -> Result<(), AppError> {
        sqlx::query!(
            "INSERT INTO user_wallets (wallet, chat_id) VALUES ($1, $2) ON CONFLICT (wallet, chat_id) DO UPDATE SET last_active = CURRENT_TIMESTAMP",
            wallet.address, wallet.chat_id
        ).execute(&self.pool).await.map_err(|e| {
            if let sqlx::Error::RowNotFound = e {
                crate::domain::errors::AppError::NotFound("Database record not found".to_string())
            } else {
                crate::domain::errors::AppError::DatabaseError(e)
            }
        })?;
        Ok(())
    }

    pub async fn remove_tracked_wallet(&self, address: &str, chat_id: i64) -> Result<(), AppError> {
        sqlx::query!(
            "DELETE FROM user_wallets WHERE wallet = $1 AND chat_id = $2",
            address,
            chat_id
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            if let sqlx::Error::RowNotFound = e {
                crate::domain::errors::AppError::NotFound("Database record not found".to_string())
            } else {
                crate::domain::errors::AppError::DatabaseError(e)
            }
        })?;
        Ok(())
    }

    pub async fn get_all_tracked_wallets(&self) -> Result<Vec<TrackedWallet>, AppError> {
        let rows = sqlx::query!("SELECT wallet, chat_id FROM user_wallets")
            .fetch_all(&self.pool)
            .await
            .map_err(|e| {
                if let sqlx::Error::RowNotFound = e {
                    crate::domain::errors::AppError::NotFound(
                        "Database record not found".to_string(),
                    )
                } else {
                    crate::domain::errors::AppError::DatabaseError(e)
                }
            })?;
        let wallets = rows
            .into_iter()
            .map(|r| TrackedWallet {
                address: r.wallet,
                chat_id: r.chat_id,
            })
            .collect();
        Ok(wallets)
    }

    pub async fn record_mined_block(&self, block: MinedBlock) -> Result<(), AppError> {
        sqlx::query!(
            "INSERT INTO mined_blocks (wallet, outpoint, amount, daa_score) VALUES ($1, $2, $3, $4) ON CONFLICT (outpoint) DO NOTHING",
            block.wallet_address, block.outpoint, block.amount, block.daa_score as i64
        ).execute(&self.pool).await.map_err(|e| {
            if let sqlx::Error::RowNotFound = e {
                crate::domain::errors::AppError::NotFound("Database record not found".to_string())
            } else {
                crate::domain::errors::AppError::DatabaseError(e)
            }
        })?;
        Ok(())
    }

    pub async fn get_lifetime_stats(&self, address: &str) -> Result<(i64, i64), AppError> {
        let res = sqlx::query!(
            r#"SELECT COUNT(*) as "count!", (COALESCE(SUM(amount), 0))::BIGINT as "sum!" FROM mined_blocks WHERE wallet = $1"#,
            address
        ).fetch_one(&self.pool).await.map_err(|e| {
            if let sqlx::Error::RowNotFound = e {
                crate::domain::errors::AppError::NotFound("Database record not found".to_string())
            } else {
                crate::domain::errors::AppError::DatabaseError(e)
            }
        })?;
        Ok((res.count, res.sum))
    }

    pub async fn get_sync_checkpoint(&self, address: &str) -> Result<u64, AppError> {
        let score = sqlx::query_scalar!(
            "SELECT daa_score FROM mined_blocks WHERE wallet = $1 ORDER BY daa_score DESC LIMIT 1",
            address
        )
        .fetch_optional(&self.pool)
        .await
        .unwrap_or(None)
        .map(|v| v as u64)
        .unwrap_or(0);
        Ok(score)
    }

    pub async fn update_sync_checkpoint(
        &self,
        _address: &str,
        _daa_score: u64,
    ) -> Result<(), AppError> {
        // Handled naturally by highest DAA score in mined_blocks
        Ok(())
    }

    pub async fn get_daily_blocks(&self, address: &str) -> Result<Vec<(String, i64)>, AppError> {
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT TO_CHAR(timestamp, 'YYYY-MM-DD') as day, COUNT(*) as count FROM mined_blocks WHERE wallet = $1 GROUP BY day ORDER BY day DESC LIMIT 7"
        )
        .bind(address)
        .fetch_all(&self.pool)
        .await
        .unwrap_or_default();
        Ok(rows)
    }

    pub async fn get_blocks_count_1h(&self, address: &str) -> Result<i64, AppError> {
        let count = sqlx::query_scalar!("SELECT COUNT(*) FROM mined_blocks WHERE wallet = $1 AND timestamp >= CURRENT_TIMESTAMP - INTERVAL '1 hour'", address)
            .fetch_one(&self.pool).await.unwrap_or(Some(0));
        Ok(count.unwrap_or(0))
    }

    pub async fn get_blocks_count_24h(&self, address: &str) -> Result<i64, AppError> {
        let count = sqlx::query_scalar!("SELECT COUNT(*) FROM mined_blocks WHERE wallet = $1 AND timestamp >= CURRENT_TIMESTAMP - INTERVAL '24 hours'", address)
            .fetch_one(&self.pool).await.unwrap_or(Some(0));
        Ok(count.unwrap_or(0))
    }

    pub async fn get_setting(&self, key: &str, default_val: &str) -> Result<String, AppError> {
        let res: Option<String> =
            sqlx::query_scalar("SELECT value_data FROM system_settings WHERE key_name = $1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await
                .unwrap_or(None);

        match res {
            Some(val) => Ok(val),
            None => {
                let _ = sqlx::query("INSERT INTO system_settings (key_name, value_data) VALUES ($1, $2) ON CONFLICT DO NOTHING")
                    .bind(key).bind(default_val).execute(&self.pool).await;
                Ok(default_val.to_string())
            }
        }
    }

    pub async fn update_setting(&self, key: &str, value: &str) -> Result<(), AppError> {
        sqlx::query("INSERT INTO system_settings (key_name, value_data) VALUES ($1, $2) ON CONFLICT (key_name) DO UPDATE SET value_data = EXCLUDED.value_data, updated_at = CURRENT_TIMESTAMP")
            .bind(key).bind(value).execute(&self.pool).await.map_err(|e| {
            if let sqlx::Error::RowNotFound = e {
                crate::domain::errors::AppError::NotFound("Database record not found".to_string())
            } else {
                crate::domain::errors::AppError::DatabaseError(e)
            }
        })?;
        Ok(())
    }

    pub async fn run_memory_cleaner(&self) -> Result<(), AppError> {
        sqlx::query!(
            "DELETE FROM chat_history WHERE timestamp < CURRENT_TIMESTAMP - INTERVAL '30 days'"
        )
        .execute(&self.pool)
        .await
        .map_err(|e| {
            if let sqlx::Error::RowNotFound = e {
                crate::domain::errors::AppError::NotFound("Database record not found".to_string())
            } else {
                crate::domain::errors::AppError::DatabaseError(e)
            }
        })?;
        Ok(())
    }

    pub async fn add_to_knowledge_base(
        &self,
        title: &str,
        link: &str,
        content: &str,
        source: &str,
    ) -> Result<(), AppError> {
        sqlx::query!(
            "INSERT INTO knowledge_base (title, link, content, source) VALUES ($1, $2, $3, $4) ON CONFLICT (link) DO NOTHING",
            title, link, content, source
        ).execute(&self.pool).await.map_err(|e| {
            if let sqlx::Error::RowNotFound = e {
                crate::domain::errors::AppError::NotFound("Database record not found".to_string())
            } else {
                crate::domain::errors::AppError::DatabaseError(e)
            }
        })?;
        Ok(())
    }

    pub async fn get_unindexed_knowledge(
        &self,
        limit: i64,
    ) -> Result<Vec<(i32, String)>, AppError> {
        let rows = sqlx::query!(
            "SELECT id, content FROM knowledge_base WHERE embedding IS NULL LIMIT $1",
            limit
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| {
            if let sqlx::Error::RowNotFound = e {
                crate::domain::errors::AppError::NotFound("Database record not found".to_string())
            } else {
                crate::domain::errors::AppError::DatabaseError(e)
            }
        })?;
        Ok(rows.into_iter().map(|r| (r.id, r.content)).collect())
    }

    pub async fn update_knowledge_embedding(
        &self,
        id: i32,
        embedding: Vec<f32>,
    ) -> Result<(), AppError> {
        let vec_str = format!("{:?}", embedding);
        sqlx::query("UPDATE knowledge_base SET embedding = $1::vector WHERE id = $2")
            .bind(&vec_str)
            .bind(id)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                if let sqlx::Error::RowNotFound = e {
                    crate::domain::errors::AppError::NotFound(
                        "Database record not found".to_string(),
                    )
                } else {
                    crate::domain::errors::AppError::DatabaseError(e)
                }
            })?;
        Ok(())
    }

    pub async fn remove_all_user_data(&self, chat_id: i64) -> Result<(), AppError> {
        sqlx::query!("DELETE FROM user_wallets WHERE chat_id = $1", chat_id)
            .execute(&self.pool)
            .await
            .map_err(|e| {
                if let sqlx::Error::RowNotFound = e {
                    crate::domain::errors::AppError::NotFound(
                        "Database record not found".to_string(),
                    )
                } else {
                    crate::domain::errors::AppError::DatabaseError(e)
                }
            })?;
        Ok(())
    }

    pub async fn get_knowledge_context(&self, keyword: &str) -> Result<Option<String>, AppError> {
        let search_term = format!("%{}%", keyword);
        let res = sqlx::query_scalar!(
            "SELECT content FROM knowledge_base
             WHERE title ILIKE $1 OR content ILIKE $1
             ORDER BY published_at DESC LIMIT 1",
            search_term
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| {
            if let sqlx::Error::RowNotFound = e {
                crate::domain::errors::AppError::NotFound("Database record not found".to_string())
            } else {
                crate::domain::errors::AppError::DatabaseError(e)
            }
        })?;
        Ok(res)
    }
}
