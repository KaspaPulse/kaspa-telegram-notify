use crate::domain::errors::AppError;
use crate::domain::models::{BotEventRecord, BotEventType, EventSeverity};

use super::postgres_adapter::PostgresRepository;

impl PostgresRepository {
    pub async fn record_bot_event_record(
        &self,
        record: BotEventRecord<'_>,
    ) -> Result<(), AppError> {
        let safe_user_name = record
            .user_name
            .map(crate::utils::sanitize_event_text_for_storage);
        let safe_command = record
            .command
            .map(crate::utils::sanitize_event_text_for_storage);
        let safe_callback_data = record
            .callback_data
            .map(crate::utils::sanitize_event_text_for_storage);
        let safe_wallet_masked = record
            .wallet_masked
            .map(crate::utils::sanitize_event_text_for_storage);
        let safe_txid_masked = record
            .txid_masked
            .map(crate::utils::sanitize_event_text_for_storage);
        let safe_block_hash_masked = record
            .block_hash_masked
            .map(crate::utils::sanitize_event_text_for_storage);
        let safe_error_message = record
            .error_message
            .map(crate::utils::sanitize_event_text_for_storage);
        sqlx::query(
            r#"
            INSERT INTO bot_event_log (
                event_type,
                severity,
                chat_id,
                user_name,
                command,
                callback_data,
                wallet_masked,
                txid_masked,
                block_hash_masked,
                status,
                error_message,
                duration_ms,
                metadata
            )
            VALUES (
                $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11, $12, COALESCE($13::jsonb, '{}'::jsonb)
            )
            "#,
        )
        .bind(record.event_type.as_str())
        .bind(record.severity.as_str())
        .bind(record.chat_id)
        .bind(safe_user_name.as_deref())
        .bind(safe_command.as_deref())
        .bind(safe_callback_data.as_deref())
        .bind(safe_wallet_masked.as_deref())
        .bind(safe_txid_masked.as_deref())
        .bind(safe_block_hash_masked.as_deref())
        .bind(record.status)
        .bind(safe_error_message.as_deref())
        .bind(record.duration_ms)
        .bind(record.metadata_json)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(())
    }
    #[allow(clippy::too_many_arguments)]
    pub async fn record_bot_event_typed(
        &self,
        event_type: BotEventType,
        severity: EventSeverity,
        chat_id: Option<i64>,
        user_name: Option<&str>,
        command: Option<&str>,
        callback_data: Option<&str>,
        wallet_masked: Option<&str>,
        txid_masked: Option<&str>,
        block_hash_masked: Option<&str>,
        status: Option<&str>,
        error_message: Option<&str>,
        duration_ms: Option<i64>,
        metadata_json: &str,
    ) -> Result<(), AppError> {
        let mut record = BotEventRecord::new(event_type, severity);
        record.chat_id = chat_id;
        record.user_name = user_name;
        record.command = command;
        record.callback_data = callback_data;
        record.wallet_masked = wallet_masked;
        record.txid_masked = txid_masked;
        record.block_hash_masked = block_hash_masked;
        record.status = status;
        record.error_message = error_message;
        record.duration_ms = duration_ms;
        record.metadata_json = metadata_json;

        self.record_bot_event_record(record).await
    }

    pub async fn purge_old_bot_events(&self, days: i64) -> Result<u64, AppError> {
        let days = days.clamp(1, 365);
        let result = sqlx::query(
            "DELETE FROM bot_event_log
             WHERE created_at < NOW() - ($1::text || ' days')::interval",
        )
        .bind(days.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(result.rows_affected())
    }

    pub async fn purge_old_wallet_alert_dedup(&self, days: i64) -> Result<u64, AppError> {
        let days = days.clamp(1, 365);
        let result = sqlx::query(
            "DELETE FROM wallet_alert_dedup
             WHERE created_at < NOW() - ($1::text || ' days')::interval",
        )
        .bind(days.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(result.rows_affected())
    }

    pub async fn purge_old_seen_utxos(&self, days: i64) -> Result<u64, AppError> {
        let days = days.clamp(1, 365);
        let result = sqlx::query(
            "DELETE FROM wallet_seen_utxos
             WHERE last_seen_at < NOW() - ($1::text || ' days')::interval",
        )
        .bind(days.to_string())
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(result.rows_affected())
    }

    #[allow(dead_code)]
    pub async fn get_delivery_summary_24h(&self) -> Result<(i64, i64, i64), AppError> {
        let detected: i64 = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM bot_event_log
             WHERE event_type = 'ALERT_DETECTED'
             AND created_at >= NOW() - INTERVAL '24 hours'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        let delivered: i64 = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM bot_event_log
             WHERE event_type = 'ALERT_DELIVERED'
             AND created_at >= NOW() - INTERVAL '24 hours'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        let failed: i64 = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM bot_event_log
             WHERE event_type = 'ALERT_DELIVERY_FAILED'
             AND created_at >= NOW() - INTERVAL '24 hours'",
        )
        .fetch_one(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok((detected, delivered, failed))
    }
}
