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

    /// Durable strict-mode proof backlog. Use the same wallet lock as the outbox.
    /// A retry will not overwrite a sent event; outbox uses its own stable outpoint ID.
    pub async fn upsert_awaiting_block_proof(
        &self,
        wallet: &str,
        utxo: &UtxoRecord,
        virtual_daa_score: u64,
        confirmations: u64,
        required_confirmations: u64,
    ) -> Result<bool, AppError> {
        let Some(mut tx) =
            super::wallets_repo::begin_tracked_wallet_write(&self.pool, wallet).await?
        else {
            return Ok(false);
        };
        let attempted = sqlx::query_scalar::<_, String>(
            "INSERT INTO pending_rewards
                (wallet,outpoint,txid,amount,reward_daa_score,virtual_daa_score,
                 confirmations,required_confirmations,attempts,status,last_checked_at)
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,1,'awaiting_block_proof',NOW())
             ON CONFLICT (wallet,outpoint) DO UPDATE SET
                 virtual_daa_score=EXCLUDED.virtual_daa_score,
                 confirmations=EXCLUDED.confirmations,
                 attempts=pending_rewards.attempts+1,
                 status='awaiting_block_proof',
                 last_checked_at=NOW()
             WHERE pending_rewards.status <> 'awaiting_block_proof'
                OR pending_rewards.last_checked_at < NOW()-INTERVAL '45 seconds'
             RETURNING outpoint",
        )
        .bind(wallet)
        .bind(&utxo.outpoint)
        .bind(&utxo.transaction_id)
        .bind(
            i64::try_from(utxo.amount)
                .map_err(|_| AppError::DatabaseError("Reward amount overflow".into()))?,
        )
        .bind(
            i64::try_from(utxo.block_daa_score)
                .map_err(|_| AppError::DatabaseError("DAA overflow".into()))?,
        )
        .bind(
            i64::try_from(virtual_daa_score)
                .map_err(|_| AppError::DatabaseError("Virtual DAA overflow".into()))?,
        )
        .bind(
            i64::try_from(confirmations)
                .map_err(|_| AppError::DatabaseError("Confirmations overflow".into()))?,
        )
        .bind(
            i64::try_from(required_confirmations)
                .map_err(|_| AppError::DatabaseError("Required confirmations overflow".into()))?,
        )
        .fetch_optional(&mut *tx)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        tx.commit()
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        Ok(attempted.is_some())
    }

    /// Restore a confirmed reward even if its UTXO was spent before PNN gave proof.
    /// Only the newly introduced strict-proof status is eligible for this recovery.
    pub async fn get_awaiting_block_proof(
        &self,
        wallet: &str,
    ) -> Result<Vec<UtxoRecord>, AppError> {
        let rows: Vec<(String, String, i64, i64)> = sqlx::query_as(
            "SELECT outpoint,txid,amount,reward_daa_score FROM pending_rewards
             WHERE wallet=$1 AND status='awaiting_block_proof'
               AND last_checked_at < NOW()-INTERVAL '45 seconds'
             ORDER BY last_checked_at,outpoint LIMIT 6",
        )
        .bind(wallet)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        rows.into_iter()
            .map(|(outpoint, transaction_id, amount, daa)| {
                Ok(UtxoRecord {
                    address: wallet.to_string(),
                    outpoint,
                    transaction_id,
                    amount: u64::try_from(amount).map_err(|_| {
                        AppError::DatabaseError("Invalid saved reward amount".into())
                    })?,
                    block_daa_score: u64::try_from(daa)
                        .map_err(|_| AppError::DatabaseError("Invalid saved reward DAA".into()))?,
                    script_public_key: String::new(),
                    is_coinbase: true,
                })
            })
            .collect()
    }

    /// Claim a spent proof in oldest-attempt order, without allowing concurrent
    /// workers to replay the same outpoint or hot-loop an unavailable PNN.
    pub async fn claim_awaiting_block_proof(
        &self,
        wallet: &str,
        outpoint: &str,
    ) -> Result<bool, AppError> {
        let claimed = sqlx::query_scalar::<_, String>(
            "UPDATE pending_rewards SET
                last_checked_at=NOW(),attempts=attempts+1
             WHERE wallet=$1 AND outpoint=$2 AND status='awaiting_block_proof'
               AND last_checked_at < NOW()-INTERVAL '45 seconds'
             RETURNING outpoint",
        )
        .bind(wallet)
        .bind(outpoint)
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        Ok(claimed.is_some())
    }

    /// Apply a confirmed-VSPC delta and its checkpoint atomically.
    /// Metadata is updated only for existing pending rewards, never for unrelated wallets.
    pub async fn apply_vspc_proof_delta(
        &self,
        matches: &[(String, String)],
        removed: &[String],
        new_cursor: &str,
        rewind: Option<&str>,
    ) -> Result<(), AppError> {
        let mut tx = self
            .pool
            .begin()
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        if !removed.is_empty() {
            sqlx::query(
                "UPDATE pending_rewards
                 SET metadata=metadata-'accepting_block_hash'
                 WHERE status IN ('pending','awaiting_block_proof')
                 AND metadata->>'accepting_block_hash'=ANY($1)",
            )
            .bind(removed)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        }
        if !matches.is_empty() {
            let txids: Vec<&str> = matches.iter().map(|(txid, _)| txid.as_str()).collect();
            let hashes: Vec<&str> = matches.iter().map(|(_, hash)| hash.as_str()).collect();
            sqlx::query(
                "UPDATE pending_rewards AS p
                 SET metadata=jsonb_set(p.metadata,'{accepting_block_hash}',
                      to_jsonb(v.accepting_hash),true)
                 FROM UNNEST($1::text[],$2::text[]) AS v(txid,accepting_hash)
                 WHERE p.txid=v.txid AND p.status IN ('pending','awaiting_block_proof')",
            )
            .bind(txids)
            .bind(hashes)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        }
        sqlx::query(
            "INSERT INTO system_settings (key_name,value_data)
             VALUES ('KASPA_PNN_VSPC_CURSOR',$1)
             ON CONFLICT (key_name) DO UPDATE SET
                value_data=EXCLUDED.value_data,updated_at=CURRENT_TIMESTAMP",
        )
        .bind(new_cursor)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        if let Some(rewind_hash) = rewind {
            sqlx::query(
                "INSERT INTO system_settings (key_name,value_data)
                 VALUES ('KASPA_PNN_VSPC_REWIND_CURSOR',$1)
                 ON CONFLICT (key_name) DO UPDATE SET
                    value_data=EXCLUDED.value_data,updated_at=CURRENT_TIMESTAMP",
            )
            .bind(rewind_hash)
            .execute(&mut *tx)
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        }
        tx.commit()
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        Ok(())
    }

    pub async fn get_pending_accepting_hash(
        &self,
        wallet: &str,
        txid: &str,
    ) -> Result<Option<String>, AppError> {
        sqlx::query_scalar::<_, String>(
            "SELECT COALESCE(metadata->>'accepting_block_hash','') FROM pending_rewards
             WHERE wallet=$1 AND txid=$2 AND status='awaiting_block_proof'
             LIMIT 1",
        )
        .bind(wallet)
        .bind(txid)
        .fetch_optional(&self.pool)
        .await
        .map(|value| value.filter(|hash| !hash.is_empty()))
        .map_err(|e| AppError::DatabaseError(e.to_string()))
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
             WHERE status <> 'awaiting_block_proof' AND last_checked_at < NOW() - ($1::text || ' days')::interval",
        )
        .bind(days.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(result.rows_affected())
    }
}
