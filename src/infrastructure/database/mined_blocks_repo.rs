use crate::domain::entities::MinedBlock;
use crate::domain::errors::AppError;

use super::postgres_adapter::PostgresRepository;

impl PostgresRepository {
    pub async fn record_mined_block(&self, block: MinedBlock) -> Result<(), AppError> {
        let Some(mut transaction) =
            super::wallets_repo::begin_tracked_wallet_write(&self.pool, &block.wallet_address)
                .await?
        else {
            return Ok(());
        };
        sqlx::query!(
            "INSERT INTO mined_blocks (wallet, outpoint, amount, daa_score)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (outpoint) DO NOTHING",
            block.wallet_address,
            block.outpoint,
            block.amount,
            block.daa_score as i64
        )
        .execute(&mut *transaction)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        transaction
            .commit()
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    pub async fn get_lifetime_stats(&self, address: &str) -> Result<(i64, i64), AppError> {
        let row = sqlx::query!(
            r#"
            SELECT
                COUNT(*) as "count!",
                (COALESCE(SUM(amount), 0))::BIGINT as "sum!"
            FROM mined_blocks
            WHERE wallet = $1
            "#,
            address
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok((row.count, row.sum))
    }

    pub async fn get_all_daily_blocks(
        &self,
        address: &str,
    ) -> Result<Vec<(String, i64, i64)>, AppError> {
        let rows: Vec<(String, i64, i64)> = sqlx::query_as(
            "SELECT
                TO_CHAR(timestamp, 'YYYY-MM-DD') as day,
                COUNT(*)::BIGINT as count,
                COALESCE(SUM(amount), 0)::BIGINT as total_sompi
             FROM mined_blocks
             WHERE wallet = $1
             GROUP BY day
             ORDER BY day DESC
            ",
        )
        .bind(address)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(rows)
    }

    pub async fn get_blocks_sum_sompi_1h(&self, address: &str) -> Result<i64, AppError> {
        let total_sompi: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(amount), 0)::BIGINT
             FROM mined_blocks
             WHERE wallet = $1
               AND timestamp >= CURRENT_TIMESTAMP - INTERVAL '1 hour'",
        )
        .bind(address)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(total_sompi)
    }

    pub async fn get_blocks_sum_sompi_24h(&self, address: &str) -> Result<i64, AppError> {
        let total_sompi: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(amount), 0)::BIGINT
             FROM mined_blocks
             WHERE wallet = $1
               AND timestamp >= CURRENT_TIMESTAMP - INTERVAL '24 hours'",
        )
        .bind(address)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(total_sompi)
    }

    pub async fn get_blocks_sum_sompi_7d(&self, address: &str) -> Result<i64, AppError> {
        let total_sompi: i64 = sqlx::query_scalar(
            "SELECT COALESCE(SUM(amount), 0)::BIGINT
             FROM mined_blocks
             WHERE wallet = $1
               AND timestamp >= CURRENT_TIMESTAMP - INTERVAL '7 days'",
        )
        .bind(address)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(total_sompi)
    }

    pub async fn get_blocks_count_1h(&self, address: &str) -> Result<i64, AppError> {
        let count = sqlx::query_scalar!(
            "SELECT COUNT(*)
             FROM mined_blocks
             WHERE wallet = $1
             AND timestamp >= CURRENT_TIMESTAMP - INTERVAL '1 hour'",
            address
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?
        .unwrap_or(0);

        Ok(count)
    }

    pub async fn get_blocks_count_24h(&self, address: &str) -> Result<i64, AppError> {
        let count = sqlx::query_scalar!(
            "SELECT COUNT(*)
             FROM mined_blocks
             WHERE wallet = $1
             AND timestamp >= CURRENT_TIMESTAMP - INTERVAL '24 hours'",
            address
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?
        .unwrap_or(0);

        Ok(count)
    }

    pub async fn get_blocks_count_7d(&self, address: &str) -> Result<i64, AppError> {
        let count = sqlx::query_scalar!(
            "SELECT COUNT(*)
             FROM mined_blocks
             WHERE wallet = $1
             AND timestamp >= CURRENT_TIMESTAMP - INTERVAL '7 days'",
            address
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?
        .unwrap_or(0);

        Ok(count)
    }

    // Retained for database characterization tests while production alert claims
    // are committed through the transactional outbox.
    #[allow(dead_code)]
    pub async fn try_claim_alert_key(
        &self,
        wallet: &str,
        alert_key: &str,
        txid_masked: Option<&str>,
        block_hash_masked: Option<&str>,
    ) -> Result<bool, AppError> {
        let result = sqlx::query(
            "INSERT INTO wallet_alert_dedup (wallet, alert_key, txid_masked, block_hash_masked)
             VALUES ($1, $2, $3, $4)
             ON CONFLICT (wallet, alert_key) DO NOTHING",
        )
        .bind(wallet)
        .bind(alert_key)
        .bind(txid_masked)
        .bind(block_hash_masked)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(result.rows_affected() > 0)
    }
}
