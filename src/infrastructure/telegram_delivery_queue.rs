use crate::domain::errors::AppError;
use sqlx::{PgPool, Row};
use std::collections::BTreeSet;

const DEFAULT_MAX_DELIVERY_ATTEMPTS: i32 = 5;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct QueuedTelegramMessage {
    pub id: i64,
    pub chat_id: i64,
    pub message_html: String,
    pub wallet_masked: Option<String>,
    pub txid_masked: Option<String>,
    pub block_hash_masked: Option<String>,
    pub amount_kas: Option<f64>,
    pub daa_score: Option<i64>,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, Clone, Default)]
pub struct DeliveryQueueStats {
    pub pending: i64,
    pub processing: i64,
    pub sent: i64,
    pub failed: i64,
    pub failed_recent: i64,
    pub suppressed: i64,
    pub oldest_active_age_seconds: u64,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AlertOutboxOutcome {
    Enqueued { recipients: usize },
    Reconciled { recipients: usize },
    Duplicate,
    Suppressed { recipients: usize },
}

pub struct AlertOutboxRequest<'a> {
    pub wallet: &'a str,
    pub source_outpoint: &'a str,
    pub alert_key: &'a str,
    pub message_html: &'a str,
    pub chat_ids: &'a [i64],
    pub wallet_masked: Option<&'a str>,
    pub txid_masked: Option<&'a str>,
    pub block_hash_masked: Option<&'a str>,
    pub amount_kas: Option<f64>,
    pub daa_score: Option<i64>,
}

fn parse_delivery_queue_enabled(value: Option<&str>) -> bool {
    match value {
        Some(value) => {
            let value = value.trim().to_ascii_lowercase();
            matches!(value.as_str(), "true" | "1" | "yes" | "on" | "enabled")
        }
        None => true,
    }
}

pub fn delivery_queue_enabled() -> bool {
    let value = std::env::var("ENABLE_TELEGRAM_DELIVERY_QUEUE").ok();
    parse_delivery_queue_enabled(value.as_deref())
}

pub fn parse_max_delivery_attempts(value: Option<&str>) -> i32 {
    value
        .and_then(|value| value.parse::<i32>().ok())
        .filter(|value| *value > 0)
        .map(|value| value.clamp(1, 100))
        .unwrap_or(DEFAULT_MAX_DELIVERY_ATTEMPTS)
}

pub fn max_delivery_attempts() -> i32 {
    let value = std::env::var("TELEGRAM_DELIVERY_MAX_ATTEMPTS").ok();
    parse_max_delivery_attempts(value.as_deref())
}

pub fn worker_id() -> String {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "unknown-host".to_string());

    format!("{}:{}", host, std::process::id())
}

#[allow(dead_code)]
pub async fn enqueue_message(
    pool: &PgPool,
    chat_id: i64,
    message_html: &str,
) -> Result<(), AppError> {
    enqueue_alert_message(pool, chat_id, message_html, None, None, None, None, None).await
}

#[allow(clippy::too_many_arguments)]
pub async fn enqueue_alert_message(
    pool: &PgPool,
    chat_id: i64,
    message_html: &str,
    wallet_masked: Option<&str>,
    txid_masked: Option<&str>,
    block_hash_masked: Option<&str>,
    amount_kas: Option<f64>,
    daa_score: Option<i64>,
) -> Result<(), AppError> {
    sqlx::query(
        "INSERT INTO telegram_delivery_queue
         (chat_id, message_html, status, wallet_masked, txid_masked, block_hash_masked, amount_kas, daa_score, next_attempt_at)
         VALUES ($1, $2, 'pending', $3, $4, $5, $6, $7, NOW())",
    )
    .bind(chat_id)
    .bind(message_html)
    .bind(wallet_masked)
    .bind(txid_masked)
    .bind(block_hash_masked)
    .bind(amount_kas)
    .bind(daa_score)
    .execute(pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    Ok(())
}

pub async fn commit_alert_outbox(
    pool: &PgPool,
    request: AlertOutboxRequest<'_>,
) -> Result<AlertOutboxOutcome, AppError> {
    if request.wallet.trim().is_empty()
        || request.source_outpoint.trim().is_empty()
        || request.alert_key.trim().is_empty()
    {
        return Err(AppError::Internal(
            "Alert outbox identity fields must not be empty".to_string(),
        ));
    }

    let recipients: BTreeSet<i64> = request.chat_ids.iter().copied().collect();
    if recipients.is_empty() {
        return Err(AppError::Internal(
            "Alert outbox requires at least one recipient".to_string(),
        ));
    }

    let mut transaction = pool
        .begin()
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    let delivery_setting = sqlx::query_scalar::<_, String>(
        "SELECT value_data FROM system_settings WHERE key_name = $1",
    )
    .bind(crate::wallet::alert_delivery_gate::ALERT_DELIVERY_SETTING_KEY)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    let delivery_enabled = delivery_setting
        .as_deref()
        .map(crate::wallet::alert_delivery_gate::parse_enabled_value)
        .unwrap_or(true);

    if delivery_enabled && !delivery_queue_enabled() {
        return Err(AppError::Internal(
            "Telegram delivery queue is required for transactional alert delivery".to_string(),
        ));
    }

    let dedup_inserted = sqlx::query(
        "INSERT INTO wallet_alert_dedup (wallet, alert_key, txid_masked, block_hash_masked)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (wallet, alert_key) DO NOTHING",
    )
    .bind(request.wallet)
    .bind(request.alert_key)
    .bind(request.txid_masked)
    .bind(request.block_hash_masked)
    .execute(&mut *transaction)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?
    .rows_affected()
        == 1;

    sqlx::query(
        "INSERT INTO wallet_seen_utxos (wallet, outpoint, first_seen_at, last_seen_at)
         VALUES ($1, $2, NOW(), NOW())
         ON CONFLICT (wallet, outpoint)
         DO UPDATE SET last_seen_at = NOW()",
    )
    .bind(request.wallet)
    .bind(request.source_outpoint)
    .execute(&mut *transaction)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    if !delivery_enabled {
        transaction
            .commit()
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        return Ok(AlertOutboxOutcome::Suppressed {
            recipients: recipients.len(),
        });
    }

    let mut inserted_rows = 0usize;

    for chat_id in &recipients {
        let result = sqlx::query(
            "INSERT INTO telegram_delivery_queue
             (chat_id, message_html, status, wallet_masked, txid_masked,
              block_hash_masked, amount_kas, daa_score, next_attempt_at, event_key)
             VALUES ($1, $2, 'pending', $3, $4, $5, $6, $7, NOW(), $8)
             ON CONFLICT (chat_id, event_key) WHERE event_key IS NOT NULL
             DO NOTHING",
        )
        .bind(*chat_id)
        .bind(request.message_html)
        .bind(request.wallet_masked)
        .bind(request.txid_masked)
        .bind(request.block_hash_masked)
        .bind(request.amount_kas)
        .bind(request.daa_score)
        .bind(request.source_outpoint)
        .execute(&mut *transaction)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        inserted_rows += result.rows_affected() as usize;
    }

    transaction
        .commit()
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    if dedup_inserted && inserted_rows == recipients.len() {
        Ok(AlertOutboxOutcome::Enqueued {
            recipients: inserted_rows,
        })
    } else if inserted_rows > 0 {
        Ok(AlertOutboxOutcome::Reconciled {
            recipients: inserted_rows,
        })
    } else {
        Ok(AlertOutboxOutcome::Duplicate)
    }
}

pub async fn fetch_pending_batch(
    pool: &PgPool,
    limit: i64,
) -> Result<Vec<QueuedTelegramMessage>, AppError> {
    let limit = limit.clamp(1, 100);
    let locked_by = worker_id();

    sqlx::query_as::<_, QueuedTelegramMessage>(
        "WITH picked AS (
            SELECT id
            FROM telegram_delivery_queue
            WHERE
                (
                    status = 'pending'
                    OR (
                        status = 'processing'
                        AND locked_at < NOW() - INTERVAL '120 seconds'
                    )
                )
                AND attempts < $3
                AND next_attempt_at <= NOW()
            ORDER BY created_at ASC
            FOR UPDATE SKIP LOCKED
            LIMIT $1
         )
         UPDATE telegram_delivery_queue q
         SET status = 'processing',
             locked_at = NOW(),
             locked_by = $2,
             updated_at = NOW()
         FROM picked
         WHERE q.id = picked.id
         RETURNING
            q.id,
            q.chat_id,
            q.message_html,
            q.wallet_masked,
            q.txid_masked,
            q.block_hash_masked,
            q.amount_kas,
            q.daa_score,
            q.created_at",
    )
    .bind(limit)
    .bind(locked_by)
    .bind(max_delivery_attempts())
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))
}

pub async fn mark_sent(pool: &PgPool, id: i64) -> Result<(), AppError> {
    let result = sqlx::query(
        "UPDATE telegram_delivery_queue
         SET status = 'sent',
             attempts = attempts + 1,
             locked_at = NULL,
             locked_by = NULL,
             updated_at = NOW()
         WHERE id = $1",
    )
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    if result.rows_affected() != 1 {
        return Err(AppError::NotFound(format!(
            "Telegram delivery queue message {id}"
        )));
    }

    Ok(())
}

pub fn retry_after_seconds(error: &str) -> Option<i64> {
    let lower = error.to_ascii_lowercase();

    if !lower.contains("retry_after") && !lower.contains("too many requests") {
        return None;
    }

    lower
        .split(|c: char| !c.is_ascii_digit())
        .filter(|part| !part.is_empty())
        .filter_map(|part| part.parse::<i64>().ok())
        .find(|value| *value > 0 && *value <= 3600)
}

pub fn retry_delay_seconds(attempts_before_increment: i32, error: &str) -> i64 {
    if let Some(retry_after) = retry_after_seconds(error) {
        return retry_after.clamp(1, 3600);
    }

    match attempts_before_increment {
        0 => 5,
        1 => 15,
        2 => 60,
        3 => 300,
        _ => 900,
    }
}

pub async fn mark_failed(pool: &PgPool, id: i64, error: &str) -> Result<(), AppError> {
    let safe_error = crate::utils::sanitize_event_text_for_storage(error);

    let attempts: i32 =
        sqlx::query_scalar("SELECT attempts FROM telegram_delivery_queue WHERE id = $1")
            .bind(id)
            .fetch_optional(pool)
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?
            .ok_or_else(|| AppError::NotFound(format!("Telegram delivery queue message {id}")))?;

    let delay = retry_delay_seconds(attempts, error);
    let max_attempts = max_delivery_attempts();

    let result = sqlx::query(
        "UPDATE telegram_delivery_queue
         SET status = CASE WHEN attempts + 1 >= $4 THEN 'failed' ELSE 'pending' END,
             attempts = attempts + 1,
             last_error = $2,
             locked_at = NULL,
             locked_by = NULL,
             next_attempt_at = NOW() + ($3::TEXT || ' seconds')::INTERVAL,
             updated_at = NOW()
         WHERE id = $1",
    )
    .bind(id)
    .bind(safe_error)
    .bind(delay)
    .bind(max_attempts)
    .execute(pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    if result.rows_affected() != 1 {
        return Err(AppError::NotFound(format!(
            "Telegram delivery queue message {id}"
        )));
    }

    Ok(())
}

#[allow(dead_code)]
pub async fn pending_count(pool: &PgPool) -> Result<i64, AppError> {
    sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM telegram_delivery_queue WHERE status = 'pending'",
    )
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))
}

pub async fn queue_stats(
    pool: &PgPool,
    recent_failure_window_seconds: u64,
) -> Result<DeliveryQueueStats, AppError> {
    let recent_failure_window_seconds = recent_failure_window_seconds.min(i64::MAX as u64) as i64;
    let row = sqlx::query(
        "SELECT
            COUNT(*) FILTER (WHERE status = 'pending')::BIGINT AS pending,
            COUNT(*) FILTER (WHERE status = 'processing')::BIGINT AS processing,
            COUNT(*) FILTER (WHERE status = 'sent')::BIGINT AS sent,
            COUNT(*) FILTER (WHERE status = 'failed')::BIGINT AS failed,
            COUNT(*) FILTER (
                WHERE status = 'failed'
                  AND updated_at >= NOW() - ($1::BIGINT * INTERVAL '1 second')
            )::BIGINT AS failed_recent,
            COUNT(*) FILTER (WHERE status = 'suppressed')::BIGINT AS suppressed,
            COUNT(*) FILTER (
                WHERE status IS NULL
                   OR status NOT IN ('pending', 'processing', 'sent', 'failed', 'suppressed')
            )::BIGINT AS unexpected_status_count,
            MIN(COALESCE(status, '<NULL>')) FILTER (
                WHERE status IS NULL
                   OR status NOT IN ('pending', 'processing', 'sent', 'failed', 'suppressed')
            ) AS unexpected_status,
            COALESCE(
                EXTRACT(EPOCH FROM (
                    NOW() - MIN(created_at) FILTER (
                        WHERE status IN ('pending', 'processing')
                    )
                )),
                0
            )::BIGINT AS oldest_active_age_seconds
         FROM telegram_delivery_queue",
    )
    .bind(recent_failure_window_seconds)
    .fetch_one(pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    let unexpected_status_count = row
        .try_get::<i64, _>("unexpected_status_count")
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    if unexpected_status_count != 0 {
        let unexpected_status = row
            .try_get::<Option<String>, _>("unexpected_status")
            .map_err(|e| AppError::DatabaseError(e.to_string()))?
            .unwrap_or_else(|| "<unknown>".to_string());
        return Err(AppError::DatabaseError(format!(
            "Unexpected Telegram delivery queue status: {unexpected_status}"
        )));
    }

    let oldest_active_age_seconds = row
        .try_get::<i64, _>("oldest_active_age_seconds")
        .map_err(|e| AppError::DatabaseError(e.to_string()))?
        .max(0) as u64;

    Ok(DeliveryQueueStats {
        pending: row
            .try_get::<i64, _>("pending")
            .map_err(|e| AppError::DatabaseError(e.to_string()))?,
        processing: row
            .try_get::<i64, _>("processing")
            .map_err(|e| AppError::DatabaseError(e.to_string()))?,
        sent: row
            .try_get::<i64, _>("sent")
            .map_err(|e| AppError::DatabaseError(e.to_string()))?,
        failed: row
            .try_get::<i64, _>("failed")
            .map_err(|e| AppError::DatabaseError(e.to_string()))?,
        failed_recent: row
            .try_get::<i64, _>("failed_recent")
            .map_err(|e| AppError::DatabaseError(e.to_string()))?,
        suppressed: row
            .try_get::<i64, _>("suppressed")
            .map_err(|e| AppError::DatabaseError(e.to_string()))?,
        oldest_active_age_seconds,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn delivery_queue_is_enabled_by_default() {
        assert!(parse_delivery_queue_enabled(None));
    }

    #[test]
    fn delivery_queue_can_be_disabled_by_env_value() {
        assert!(!parse_delivery_queue_enabled(Some("false")));
        assert!(!parse_delivery_queue_enabled(Some("off")));
    }

    #[test]
    fn delivery_queue_accepts_enabled_env_values() {
        for value in ["true", "1", "yes", "on", "enabled", " YES "] {
            assert!(parse_delivery_queue_enabled(Some(value)), "value={value}");
        }
    }

    #[test]
    fn retry_after_is_extracted_from_error_text() {
        assert_eq!(
            retry_after_seconds("Too Many Requests: retry_after 17"),
            Some(17)
        );
        assert_eq!(retry_after_seconds("normal error"), None);
    }

    #[test]
    fn retry_delay_uses_backoff() {
        assert_eq!(retry_delay_seconds(0, "network error"), 5);
        assert_eq!(retry_delay_seconds(2, "network error"), 60);
    }
}
