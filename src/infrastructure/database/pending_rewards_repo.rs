use crate::domain::errors::AppError;
use crate::domain::models::UtxoRecord;

use super::postgres_adapter::PostgresRepository;

impl PostgresRepository {
    pub async fn ensure_pending_rewards_table(&self) -> Result<(), AppError> {
        sqlx::query(
            r#"
            CREATE TABLE IF NOT EXISTS pending_rewards (
                wallet TEXT NOT NULL,
                outpoint TEXT NOT NULL,
                txid TEXT NOT NULL,
                amount BIGINT NOT NULL,
                reward_daa_score BIGINT NOT NULL,
                virtual_daa_score BIGINT NOT NULL,
                confirmations BIGINT NOT NULL DEFAULT 0,
                required_confirmations BIGINT NOT NULL DEFAULT 10,
                attempts BIGINT NOT NULL DEFAULT 0,
                status TEXT NOT NULL DEFAULT 'pending',
                first_seen_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                last_checked_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
                metadata JSONB NOT NULL DEFAULT '{}'::jsonb,
                PRIMARY KEY (wallet, outpoint)
            )
            "#,
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_pending_rewards_status_checked
             ON pending_rewards (status, last_checked_at)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        sqlx::query(
            "CREATE INDEX IF NOT EXISTS idx_pending_rewards_wallet
             ON pending_rewards (wallet)",
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    pub async fn upsert_pending_reward(
        &self,
        wallet: &str,
        utxo: &UtxoRecord,
        virtual_daa_score: u64,
        confirmations: u64,
        required_confirmations: u64,
    ) -> Result<(), AppError> {
        let Some(mut transaction) =
            super::wallets_repo::begin_tracked_wallet_write(&self.pool, wallet).await?
        else {
            return Ok(());
        };
        sqlx::query(
            r#"
            INSERT INTO pending_rewards (
                wallet,
                outpoint,
                txid,
                amount,
                reward_daa_score,
                virtual_daa_score,
                confirmations,
                required_confirmations,
                attempts,
                status,
                last_checked_at,
                metadata
            )
            VALUES ($1, $2, $3, $4, $5, $6, $7, $8, 1, 'pending', NOW(), '{}'::jsonb)
            ON CONFLICT (wallet, outpoint)
            DO UPDATE SET
                txid = EXCLUDED.txid,
                amount = EXCLUDED.amount,
                reward_daa_score = EXCLUDED.reward_daa_score,
                virtual_daa_score = EXCLUDED.virtual_daa_score,
                confirmations = EXCLUDED.confirmations,
                required_confirmations = EXCLUDED.required_confirmations,
                attempts = pending_rewards.attempts + 1,
                status = 'pending',
                last_checked_at = NOW()
            "#,
        )
        .bind(wallet)
        .bind(&utxo.outpoint)
        .bind(&utxo.transaction_id)
        .bind(utxo.amount as i64)
        .bind(utxo.block_daa_score as i64)
        .bind(virtual_daa_score as i64)
        .bind(confirmations as i64)
        .bind(required_confirmations as i64)
        .execute(&mut *transaction)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        transaction
            .commit()
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    pub async fn delete_pending_reward(
        &self,
        wallet: &str,
        outpoint: &str,
    ) -> Result<(), AppError> {
        sqlx::query(
            "DELETE FROM pending_rewards
             WHERE wallet = $1 AND outpoint = $2",
        )
        .bind(wallet)
        .bind(outpoint)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    pub async fn purge_old_pending_rewards(&self, days: i64) -> Result<u64, AppError> {
        let days = days.clamp(1, 365);

        let result = sqlx::query(
            "DELETE FROM pending_rewards
             WHERE last_checked_at < NOW() - ($1::text || ' days')::interval",
        )
        .bind(days.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(result.rows_affected())
    }
}
