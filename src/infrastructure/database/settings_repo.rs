use crate::domain::errors::AppError;

use super::postgres_adapter::PostgresRepository;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PersistedRuntimeSettings {
    pub memory_cleaner_enabled: bool,
    pub live_sync_enabled: bool,
    pub maintenance_mode: bool,
}

fn parse_persisted_bool(key: &str, value: &str) -> Result<bool, AppError> {
    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => Err(AppError::Internal(format!(
            "Persisted setting {key} must be a boolean value"
        ))),
    }
}

impl PostgresRepository {
    pub async fn get_setting(&self, key: &str, default_val: &str) -> Result<String, AppError> {
        let value: Option<String> =
            sqlx::query_scalar("SELECT value_data FROM system_settings WHERE key_name = $1")
                .bind(key)
                .fetch_optional(&self.pool)
                .await
                .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        if let Some(value) = value {
            return Ok(value);
        }

        sqlx::query(
            "INSERT INTO system_settings (key_name, value_data)
             VALUES ($1, $2)
             ON CONFLICT DO NOTHING",
        )
        .bind(key)
        .bind(default_val)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(default_val.to_string())
    }

    pub async fn load_persisted_runtime_settings(
        &self,
    ) -> Result<PersistedRuntimeSettings, AppError> {
        let memory = self.get_setting("ENABLE_MEMORY_CLEANER", "false").await?;
        let live_sync = self.get_setting("ENABLE_LIVE_SYNC", "true").await?;
        let maintenance = self.get_setting("MAINTENANCE_MODE", "false").await?;

        Ok(PersistedRuntimeSettings {
            memory_cleaner_enabled: parse_persisted_bool("ENABLE_MEMORY_CLEANER", &memory)?,
            live_sync_enabled: parse_persisted_bool("ENABLE_LIVE_SYNC", &live_sync)?,
            maintenance_mode: parse_persisted_bool("MAINTENANCE_MODE", &maintenance)?,
        })
    }

    pub async fn update_setting(&self, key: &str, value: &str) -> Result<(), AppError> {
        sqlx::query(
            "INSERT INTO system_settings (key_name, value_data)
             VALUES ($1, $2)
             ON CONFLICT (key_name)
             DO UPDATE SET
                value_data = EXCLUDED.value_data,
                updated_at = CURRENT_TIMESTAMP",
        )
        .bind(key)
        .bind(value)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    pub async fn run_memory_cleaner(&self) -> Result<(), AppError> {
        let deleted_events = self.purge_old_bot_events(60).await?;
        let deleted_alert_dedup = self.purge_old_wallet_alert_dedup(14).await?;
        let deleted_seen_utxos = self.purge_old_seen_utxos(30).await?;
        let deleted_pending_rewards = self.purge_old_pending_rewards(14).await?;

        tracing::info!(
            "[MEMORY CLEANER] Cleanup complete. Events: {}, Alert dedup: {}, Seen UTXOs: {}, Pending rewards: {}",
            deleted_events,
            deleted_alert_dedup,
            deleted_seen_utxos,
            deleted_pending_rewards
        );

        let metadata = format!(
            r#"{{"bot_event_log":{},"wallet_alert_dedup":{},"wallet_seen_utxos":{},"pending_rewards":{},"retention_days":{{"bot_event_log":60,"wallet_alert_dedup":14,"wallet_seen_utxos":30,"pending_rewards":14}}}}"#,
            deleted_events, deleted_alert_dedup, deleted_seen_utxos, deleted_pending_rewards
        );

        self.record_bot_event_typed(
            crate::domain::models::BotEventType::EventLogPurged,
            crate::domain::models::EventSeverity::Info,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some("cleanup_complete"),
            None,
            None,
            &metadata,
        )
        .await?;

        Ok(())
    }
}
