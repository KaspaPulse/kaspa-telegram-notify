use std::collections::HashSet;

use crate::domain::entities::{TrackedWallet, UserDataDeletionSummary, WalletRemovalOutcome};
use crate::domain::errors::AppError;

use super::postgres_adapter::PostgresRepository;

// The two-int advisory namespace is separate from the one-BIGINT per-chat locks.
// Lock actual hash keys in order: even a hash collision cannot invert lock order
// when two chats forget overlapping sets of wallets concurrently.
async fn lock_wallet_mutations(
    transaction: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    wallets: &[String],
) -> Result<(), AppError> {
    let keys: Vec<i32> = sqlx::query_scalar(
        "SELECT DISTINCT hashtext(wallet) FROM unnest($1::TEXT[]) AS wallets(wallet) ORDER BY 1",
    )
    .bind(wallets)
    .fetch_all(&mut **transaction)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    for key in keys {
        sqlx::query("SELECT pg_advisory_xact_lock(1263552332, $1)")
            .bind(key)
            .execute(&mut **transaction)
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
    }
    Ok(())
}

impl PostgresRepository {
    // Retained as a narrow read API for database characterization/integration tests. Runtime
    // mutation code performs the equivalent check inside add_tracked_wallet's transaction.
    #[allow(dead_code)]
    pub async fn count_user_wallets(&self, chat_id: i64) -> Result<i64, AppError> {
        let count = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*)
             FROM user_wallets
             WHERE chat_id = $1",
        )
        .bind(chat_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(count)
    }

    // Retained for characterization/integration tests without reusing it in the write path,
    // where a separate preflight query would reintroduce a check-then-insert race.
    #[allow(dead_code)]
    pub async fn user_wallet_exists(&self, address: &str, chat_id: i64) -> Result<bool, AppError> {
        let exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                SELECT 1
                FROM user_wallets
                WHERE wallet = $1
                AND chat_id = $2
             )",
        )
        .bind(address)
        .bind(chat_id)
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(exists)
    }

    pub async fn add_tracked_wallet(&self, wallet: TrackedWallet) -> Result<(), AppError> {
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        // Serialize wallet-list mutations per chat so two concurrent add requests cannot both
        // observe the same count and exceed MAX_WALLETS_PER_USER.
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(wallet.chat_id)
            .execute(&mut *transaction)
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        // A different chat must not subscribe between an orphan check and cleanup.
        lock_wallet_mutations(&mut transaction, std::slice::from_ref(&wallet.address)).await?;

        let already_exists = sqlx::query_scalar::<_, bool>(
            "SELECT EXISTS(
                SELECT 1
                FROM user_wallets
                WHERE wallet = $1
                AND chat_id = $2
             )",
        )
        .bind(&wallet.address)
        .bind(wallet.chat_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        if !already_exists {
            let current_count = sqlx::query_scalar::<_, i64>(
                "SELECT COUNT(*)
                 FROM user_wallets
                 WHERE chat_id = $1",
            )
            .bind(wallet.chat_id)
            .fetch_one(&mut *transaction)
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
            let max_wallets = crate::utils::max_wallets_per_user();

            if current_count >= max_wallets {
                return Err(AppError::Internal(format!(
                    "MAX_WALLETS_PER_USER limit reached. Current limit: {} wallets.",
                    max_wallets
                )));
            }
        }

        sqlx::query!(
            "INSERT INTO user_wallets (wallet, chat_id)
             VALUES ($1, $2)
             ON CONFLICT (wallet, chat_id)
             DO UPDATE SET last_active = CURRENT_TIMESTAMP",
            wallet.address,
            wallet.chat_id
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

    pub async fn remove_tracked_wallet(
        &self,
        address: &str,
        chat_id: i64,
    ) -> Result<WalletRemovalOutcome, AppError> {
        let result = sqlx::query(
            "DELETE FROM user_wallets
             WHERE wallet = $1 AND chat_id = $2",
        )
        .bind(address)
        .bind(chat_id)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(if result.rows_affected() == 1 {
            WalletRemovalOutcome::Removed
        } else {
            WalletRemovalOutcome::NotFound
        })
    }

    pub async fn remove_all_user_wallets(&self, chat_id: i64) -> Result<(), AppError> {
        sqlx::query!(
            "DELETE FROM user_wallets
             WHERE chat_id = $1",
            chat_id
        )
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    pub async fn remove_all_user_data(
        &self,
        chat_id: i64,
        actor_user_id: u64,
    ) -> Result<UserDataDeletionSummary, AppError> {
        let actor_user_id = i64::try_from(actor_user_id).map_err(|_| {
            AppError::Internal("Telegram actor id exceeds BIGINT range".to_string())
        })?;
        let mut transaction = self
            .pool
            .begin()
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        // Serialize user-scoped destructive operations with wallet-list mutations.
        sqlx::query("SELECT pg_advisory_xact_lock($1)")
            .bind(chat_id)
            .execute(&mut *transaction)
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        let wallets: Vec<String> = sqlx::query_scalar(
            "SELECT wallet FROM user_wallets WHERE chat_id = $1 ORDER BY wallet FOR UPDATE",
        )
        .bind(chat_id)
        .fetch_all(&mut *transaction)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        // Hold these through commit, covering both the last-subscriber check and
        // shared-state deletion. Additions by every chat use the same locks.
        lock_wallet_mutations(&mut transaction, &wallets).await?;

        let queue_rows_deleted =
            sqlx::query("DELETE FROM telegram_delivery_queue WHERE chat_id = $1")
                .bind(chat_id)
                .execute(&mut *transaction)
                .await
                .map_err(|e| AppError::DatabaseError(e.to_string()))?
                .rows_affected();

        let event_rows_deleted = sqlx::query("DELETE FROM bot_event_log WHERE chat_id = $1")
            .bind(chat_id)
            .execute(&mut *transaction)
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?
            .rows_affected();

        let chat_history_rows_deleted = sqlx::query("DELETE FROM chat_history WHERE chat_id = $1")
            .bind(chat_id)
            .execute(&mut *transaction)
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?
            .rows_affected();

        // Security audit history is retained, but direct Telegram identity linkage is removed.
        let admin_audit_rows_anonymized = sqlx::query(
            "UPDATE admin_audit_log
             SET admin_actor_user_id = NULL, admin_chat_id = 0
             WHERE admin_chat_id = $1 OR admin_actor_user_id = $2",
        )
        .bind(chat_id)
        .bind(actor_user_id)
        .execute(&mut *transaction)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?
        .rows_affected();

        let wallets_deleted = sqlx::query("DELETE FROM user_wallets WHERE chat_id = $1")
            .bind(chat_id)
            .execute(&mut *transaction)
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?
            .rows_affected();

        let mut orphan_wallet_addresses = Vec::new();
        for wallet in &wallets {
            let still_tracked = sqlx::query_scalar::<_, bool>(
                "SELECT EXISTS(SELECT 1 FROM user_wallets WHERE wallet = $1)",
            )
            .bind(wallet)
            .fetch_one(&mut *transaction)
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

            if still_tracked {
                continue;
            }

            for statement in [
                "DELETE FROM wallet_seen_utxos WHERE wallet = $1",
                "DELETE FROM wallet_alert_dedup WHERE wallet = $1",
                "DELETE FROM pending_rewards WHERE wallet = $1",
                "DELETE FROM mined_blocks WHERE wallet = $1",
            ] {
                sqlx::query(statement)
                    .bind(wallet)
                    .execute(&mut *transaction)
                    .await
                    .map_err(|e| AppError::DatabaseError(e.to_string()))?;
            }
            orphan_wallet_addresses.push(wallet.clone());
        }

        let residual_direct_links = sqlx::query_scalar::<_, i64>(
            "SELECT
                (SELECT COUNT(*) FROM user_wallets WHERE chat_id = $1)
              + (SELECT COUNT(*) FROM telegram_delivery_queue WHERE chat_id = $1)
              + (SELECT COUNT(*) FROM bot_event_log WHERE chat_id = $1)
              + (SELECT COUNT(*) FROM chat_history WHERE chat_id = $1)
              + (SELECT COUNT(*) FROM admin_audit_log
                 WHERE admin_chat_id = $1 OR admin_actor_user_id = $2)",
        )
        .bind(chat_id)
        .bind(actor_user_id)
        .fetch_one(&mut *transaction)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        if residual_direct_links != 0 {
            return Err(AppError::Internal(format!(
                "User data deletion verification failed: {residual_direct_links} direct linkage rows remain"
            )));
        }

        transaction
            .commit()
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(UserDataDeletionSummary {
            wallets_deleted,
            event_rows_deleted,
            chat_history_rows_deleted,
            queue_rows_deleted,
            admin_audit_rows_anonymized,
            orphan_wallets_cleaned: orphan_wallet_addresses.len(),
            orphan_wallet_addresses,
        })
    }

    pub async fn get_all_tracked_wallets(&self) -> Result<Vec<TrackedWallet>, AppError> {
        let rows = sqlx::query!(
            r#"
            SELECT wallet, chat_id as "chat_id!"
            FROM user_wallets
            "#
        )
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        let wallets = rows
            .into_iter()
            .map(|row| TrackedWallet {
                address: row.wallet,
                chat_id: row.chat_id,
            })
            .collect();

        Ok(wallets)
    }

    pub async fn get_tracked_wallets_for_chat(
        &self,
        chat_id: i64,
    ) -> Result<Vec<TrackedWallet>, AppError> {
        let rows: Vec<(String, i64)> = sqlx::query_as(
            "SELECT wallet, chat_id
             FROM user_wallets
             WHERE chat_id = $1
             ORDER BY wallet",
        )
        .bind(chat_id)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(rows
            .into_iter()
            .map(|(address, row_chat_id)| TrackedWallet {
                address,
                chat_id: row_chat_id,
            })
            .collect())
    }

    pub async fn get_seen_utxos(&self, wallet: &str) -> Result<HashSet<String>, AppError> {
        let rows: Vec<(String,)> =
            sqlx::query_as("SELECT outpoint FROM wallet_seen_utxos WHERE wallet = $1")
                .bind(wallet)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(rows.into_iter().map(|row| row.0).collect())
    }

    pub async fn upsert_seen_utxos(
        &self,
        wallet: &str,
        outpoints: &[String],
    ) -> Result<(), AppError> {
        for outpoint in outpoints {
            sqlx::query(
                "INSERT INTO wallet_seen_utxos (wallet, outpoint)
                 VALUES ($1, $2)
                 ON CONFLICT (wallet, outpoint)
                 DO UPDATE SET last_seen_at = CURRENT_TIMESTAMP",
            )
            .bind(wallet)
            .bind(outpoint)
            .execute(&self.pool)
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        }

        Ok(())
    }

    pub async fn prune_seen_utxos(
        &self,
        wallet: &str,
        current_outpoints: &[String],
    ) -> Result<(), AppError> {
        if current_outpoints.is_empty() {
            sqlx::query("DELETE FROM wallet_seen_utxos WHERE wallet = $1")
                .bind(wallet)
                .execute(&self.pool)
                .await
                .map_err(|e| AppError::DatabaseError(e.to_string()))?;

            return Ok(());
        }

        sqlx::query(
            "DELETE FROM wallet_seen_utxos
             WHERE wallet = $1
             AND NOT (outpoint = ANY($2))",
        )
        .bind(wallet)
        .bind(current_outpoints)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    #[allow(dead_code)]
    pub async fn get_subscribers_for_wallet(&self, wallet: &str) -> Result<Vec<i64>, AppError> {
        let rows: Vec<(i64,)> =
            sqlx::query_as("SELECT chat_id FROM user_wallets WHERE wallet = $1 ORDER BY chat_id")
                .bind(wallet)
                .fetch_all(&self.pool)
                .await
                .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(rows.into_iter().map(|row| row.0).collect())
    }
}
