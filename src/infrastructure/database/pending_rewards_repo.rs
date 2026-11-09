use crate::domain::errors::AppError;
use crate::domain::models::UtxoRecord;

use super::postgres_adapter::PostgresRepository;

// OP076: stable SQL expression shared by the queue reader and atomic claim.
// The first three attempts remain as responsive as OP075 (45s).
// Subsequent retries are reason-aware: with verified VSPC acceptance,
// preserve up to 30-min attempts for missing canonical mined-block payload;
// without acceptance, cap at 15min. Neither branch ever excludes a row.
pub(crate) const OP076_RETRY_INTERVAL_SQL: &str = r#"make_interval(secs => CASE
    WHEN attempts <= 3 THEN 45
    WHEN (metadata->>'accepting_block_hash') ~ '^[a-fA-F0-9]{64}$' THEN
        CASE WHEN attempts <= 10 THEN 120 WHEN attempts <= 40 THEN 300
             WHEN attempts <= 120 THEN 900 ELSE 1800 END
    ELSE CASE WHEN attempts <= 10 THEN 120 WHEN attempts <= 40 THEN 300
              ELSE 900 END
END)"#;

/// Mirror the constant SQL delay table for exhaustive unit boundary checks.
#[cfg(test)]
fn op076_retry_secs(attempts: i64, has_accepted_hash: bool) -> u64 {
    if attempts <= 3 {
        45
    } else if has_accepted_hash {
        if attempts <= 10 {
            120
        } else if attempts <= 40 {
            300
        } else if attempts <= 120 {
            900
        } else {
            1800
        }
    } else if attempts <= 10 {
        120
    } else if attempts <= 40 {
        300
    } else {
        900
    }
}

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

    /// Snapshot strict-proof outpoints before the live scan: a persisted reward,
    /// even if its UTXO is still unspent, is historical work and must never
    /// steal the reserved fresh-reward slot by being re-upserted every 45s.
    pub async fn get_awaiting_proof_outpoints(
        &self,
        wallet: &str,
    ) -> Result<std::collections::HashSet<String>, AppError> {
        let rows: Vec<String> = sqlx::query_scalar(
            "SELECT outpoint FROM pending_rewards
             WHERE wallet=$1 AND status='awaiting_block_proof'",
        )
        .bind(wallet)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        Ok(rows.into_iter().collect())
    }

    /// Restore a confirmed reward even if its UTXO was spent before PNN gave proof.
    /// Only the newly introduced strict-proof status is eligible for this recovery.
    // OP064 fixed owner boundary: NEVER resurrect or notify a proof that
    // existed before the audited OP063 production cutover at 17:30:32 UTC.
    // This is a fixed event timestamp, never a moving date filter.
    // Historical records remain durable, counted and untouched. Only fresh
    // proof rows introduced after this boundary can enter the retry lane.
    pub async fn get_awaiting_block_proof(
        &self,
        wallet: &str,
    ) -> Result<Vec<UtxoRecord>, AppError> {
        let query = format!(
            "SELECT outpoint,txid,amount,reward_daa_score FROM pending_rewards
             WHERE wallet=$1 AND status='awaiting_block_proof'
               AND last_checked_at < NOW()-{OP076_RETRY_INTERVAL_SQL}
               AND first_seen_at >= TIMESTAMPTZ '2026-11-05 17:30:32+00'
             ORDER BY last_checked_at,outpoint LIMIT 6"
        );
        // SqlSafeStr audit: SQL is formatted from a compile-time constant
        // OP076_RETRY_INTERVAL_SQL only; the wallet input remains a bind.
        let rows: Vec<(String, String, i64, i64)> = sqlx::query_as(sqlx::AssertSqlSafe(query))
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
        // Repeat EXACTLY the same due interval and fixed cutover as the
        // read-only candidate query. This row claim is the final atomic gate
        // against concurrent wallet scans and historical backfill.
        let query = format!(
            "UPDATE pending_rewards SET
                last_checked_at=NOW(),attempts=attempts+1
             WHERE wallet=$1 AND outpoint=$2 AND status='awaiting_block_proof'
               AND first_seen_at >= TIMESTAMPTZ '2026-11-05 17:30:32+00'
               AND last_checked_at < NOW()-{OP076_RETRY_INTERVAL_SQL}
             RETURNING outpoint"
        );
        // SqlSafeStr audit: only static CASE expression is interpolated;
        // wallet + outpoint stay separately bound by SQLx.
        let claimed = sqlx::query_scalar::<_, String>(sqlx::AssertSqlSafe(query))
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
                 SET metadata=metadata-'accepting_block_hash',
                     attempts=CASE
                       WHEN status='awaiting_block_proof'
                         AND first_seen_at>=TIMESTAMPTZ '2026-11-05 17:30:32+00'
                       THEN 0 ELSE attempts END,
                     last_checked_at=CASE
                       WHEN status='awaiting_block_proof'
                         AND first_seen_at>=TIMESTAMPTZ '2026-11-05 17:30:32+00'
                       THEN NOW()-INTERVAL '60 seconds'
                       ELSE last_checked_at END
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
                      to_jsonb(v.accepting_hash),true),
                     attempts=CASE
                       WHEN p.status='awaiting_block_proof'
                         AND p.first_seen_at>=TIMESTAMPTZ '2026-11-05 17:30:32+00'
                         AND lower(p.metadata->>'accepting_block_hash')
                             IS DISTINCT FROM lower(v.accepting_hash)
                       THEN 0 ELSE p.attempts END,
                     last_checked_at=CASE
                       WHEN p.status='awaiting_block_proof'
                         AND p.first_seen_at>=TIMESTAMPTZ '2026-11-05 17:30:32+00'
                         AND lower(p.metadata->>'accepting_block_hash')
                             IS DISTINCT FROM lower(v.accepting_hash)
                       THEN NOW()-INTERVAL '60 seconds'
                       ELSE p.last_checked_at END
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

// OP076 fail-closed retry policy: bounded, no terminal suppression.
#[cfg(test)]
mod op076_proof_retry_policy_tests {
    use super::*;
    #[test]
    fn all_retries_remain_possible_and_bounds_are_monotonic() {
        for accepted in [false, true] {
            let mut previous = 0;
            for attempts in 0..=5000 {
                let delay = op076_retry_secs(attempts, accepted);
                assert!((45..=1800).contains(&delay));
                assert!(delay >= previous, "backoff must be monotonic");
                previous = delay;
            }
        }
        assert_eq!(op076_retry_secs(684, true), 1800);
        assert_eq!(op076_retry_secs(610, true), 1800);
        assert_eq!(op076_retry_secs(124, false), 900);
        assert_eq!(op076_retry_secs(1, true), 45);
    }
    #[test]
    fn first_three_attempts_and_reason_specific_boundaries() {
        for attempted in [0, 1, 2, 3] {
            assert_eq!(op076_retry_secs(attempted, true), 45);
            assert_eq!(op076_retry_secs(attempted, false), 45);
        }
        assert_eq!(op076_retry_secs(4, true), 120);
        assert_eq!(op076_retry_secs(10, true), 120);
        assert_eq!(op076_retry_secs(11, true), 300);
        assert_eq!(op076_retry_secs(40, true), 300);
        assert_eq!(op076_retry_secs(41, true), 900);
        assert_eq!(op076_retry_secs(120, true), 900);
        assert_eq!(op076_retry_secs(121, true), 1800);
        assert_eq!(op076_retry_secs(41, false), 900);
        assert_eq!(op076_retry_secs(10000, false), 900);
    }
}
