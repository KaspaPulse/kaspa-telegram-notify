use crate::domain::errors::AppError;
use sqlx::PgPool;

pub const ALERT_DELIVERY_SETTING_KEY: &str = "ENABLE_ALERT_DELIVERY";

pub fn parse_enabled_value(value: &str) -> bool {
    matches!(
        value.trim().to_ascii_lowercase().as_str(),
        "true" | "1" | "yes" | "on" | "enabled"
    )
}

pub async fn is_alert_delivery_enabled(pool: &PgPool) -> Result<bool, AppError> {
    let value = sqlx::query_scalar::<_, String>(
        "SELECT value_data FROM system_settings WHERE key_name = $1",
    )
    .bind(ALERT_DELIVERY_SETTING_KEY)
    .fetch_optional(pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    Ok(value.as_deref().map(parse_enabled_value).unwrap_or(true))
}

pub async fn set_alert_delivery_enabled(pool: &PgPool, enabled: bool) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO system_settings (key_name, value_data)
         VALUES ($1, $2)
         ON CONFLICT (key_name)
         DO UPDATE SET
            value_data = EXCLUDED.value_data,
            updated_at = CURRENT_TIMESTAMP",
    )
    .bind(ALERT_DELIVERY_SETTING_KEY)
    .bind(if enabled { "true" } else { "false" })
    .execute(pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    Ok(())
}

async fn suppressed_count_since_last_change(pool: &PgPool) -> i64 {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*)
         FROM bot_event_log
         WHERE event_type = 'ALERT_DELIVERY_SUPPRESSED'
         AND created_at >= COALESCE(
             (SELECT updated_at FROM system_settings WHERE key_name = $1),
             NOW() - INTERVAL '30 days'
         )",
    )
    .bind(ALERT_DELIVERY_SETTING_KEY)
    .fetch_one(pool)
    .await
    .unwrap_or(0)
}

pub async fn alert_delivery_status_text(pool: &PgPool) -> String {
    let enabled = match is_alert_delivery_enabled(pool).await {
        Ok(value) => value,
        Err(error) => {
            tracing::error!(
                "[DATABASE ERROR] Failed to read alert delivery setting: {}",
                error
            );

            return "⚠️ <b>Alert Delivery Status</b>
━━━━━━━━━━━━━━━━━━
Status: <code>UNAVAILABLE</code>

The database setting could not be read. Mining alerts are not treated as enabled until the database recovers.".to_string();
        }
    };

    let suppressed_count = suppressed_count_since_last_change(pool).await;

    if enabled {
        "🔔 <b>Alert Delivery Status</b>\n━━━━━━━━━━━━━━━━━━\nStatus: <code>ENABLED</code>\n\nMining alerts are being sent normally.\n\nNote: This does not affect block detection, DAG analysis, or database logging.".to_string()
    } else {
        format!(
            "🔕 <b>Alert Delivery Status</b>\n━━━━━━━━━━━━━━━━━━\nStatus: <code>DISABLED</code>\n\nThe bot is still detecting blocks and recording events, but Telegram mining alerts are muted.\n\nSuppressed alerts since last setting change: <code>{}</code>\n\nNew alerts will not be sent until alert delivery is resumed.",
            suppressed_count
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_enabled_values() {
        assert!(parse_enabled_value("true"));
        assert!(parse_enabled_value("1"));
        assert!(parse_enabled_value("enabled"));
        assert!(!parse_enabled_value("false"));
        assert!(!parse_enabled_value("0"));
    }
}
