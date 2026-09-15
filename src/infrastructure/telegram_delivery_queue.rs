use crate::domain::errors::AppError;
use rand::TryRng;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Postgres, Row, Transaction};
use std::collections::BTreeSet;

const DEFAULT_MAX_DELIVERY_ATTEMPTS: i32 = 5;

#[derive(Debug, Clone, sqlx::FromRow)]
pub struct QueuedTelegramMessage {
    pub id: i64,
    pub claim_token: String,
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
    NoCurrentRecipients,
    Suppressed { recipients: usize },
}

pub struct AlertOutboxRequest<'a> {
    pub wallet: &'a str,
    pub source_outpoint: &'a str,
    pub alert_key: &'a str,
    pub message_html: &'a str,
    pub chat_ids: &'a [i64],
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

const DELIVERY_WALLET_TOKEN_BYTES: usize = 16;
const DELIVERY_WALLET_ID_PREFIX: &str = "v2";

fn delivery_wallet_token(wallet: &str) -> String {
    let digest = Sha256::digest(format!("delivery-wallet-v1:{wallet}").as_bytes());
    digest[..DELIVERY_WALLET_TOKEN_BYTES]
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn delivery_wallet_identity(wallet: &str) -> String {
    format!(
        "{DELIVERY_WALLET_ID_PREFIX}:{}",
        delivery_wallet_token(wallet)
    )
}

fn delivery_wallet_token_from_identity(identity: &str) -> Option<&str> {
    let token = identity.strip_prefix("v2:")?;
    (token.len() == DELIVERY_WALLET_TOKEN_BYTES * 2
        && token.bytes().all(|byte| byte.is_ascii_hexdigit()))
    .then_some(token)
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

    let Some(mut transaction) =
        super::database::wallets_repo::begin_tracked_wallet_write(pool, request.wallet).await?
    else {
        return Ok(AlertOutboxOutcome::NoCurrentRecipients);
    };
    // Intersect the original event recipients with current subscriptions while
    // holding the same wallet lock as subscription mutation and privacy deletion.
    let requested: Vec<i64> = recipients.into_iter().collect();
    let recipients: Vec<i64> = sqlx::query_scalar(
        "SELECT chat_id FROM user_wallets WHERE wallet = $1 AND chat_id = ANY($2) ORDER BY chat_id",
    )
    .bind(request.wallet)
    .bind(&requested)
    .fetch_all(&mut *transaction)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;
    if recipients.is_empty() {
        transaction
            .commit()
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?;
        return Ok(AlertOutboxOutcome::NoCurrentRecipients);
    }

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

    let wallet_identity = delivery_wallet_identity(request.wallet);
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
        .bind(&wallet_identity)
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
    // A new claim must differ even when the same process reclaims an expired lease.
    let mut nonce = [0u8; 16];
    rand::rngs::SysRng
        .try_fill_bytes(&mut nonce)
        .map_err(|error| {
            AppError::Internal(format!(
                "Failed to generate delivery claim identity: {error}"
            ))
        })?;
    let locked_by = format!("{}:{:032x}", worker_id(), u128::from_le_bytes(nonce));

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
            q.locked_by AS claim_token,
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

#[allow(dead_code)]
pub async fn delivery_claim_is_current(
    pool: &PgPool,
    id: i64,
    claim_token: &str,
) -> Result<bool, AppError> {
    sqlx::query_scalar(
        "SELECT EXISTS(
            SELECT 1
            FROM telegram_delivery_queue
            WHERE id = $1
              AND status = 'processing'
              AND locked_by = $2
        )",
    )
    .bind(id)
    .bind(claim_token)
    .fetch_one(pool)
    .await
    .map_err(|error| AppError::DatabaseError(error.to_string()))
}

pub struct DeliveryClaimGuard<'a> {
    transaction: Transaction<'a, Postgres>,
    id: i64,
    claim_token: String,
}

impl DeliveryClaimGuard<'_> {
    #[allow(dead_code)]
    pub async fn mark_sent(mut self) -> Result<(), AppError> {
        let result = sqlx::query(
            "UPDATE telegram_delivery_queue
             SET status = 'sent',
                 attempts = attempts + 1,
                 locked_at = NULL,
                 locked_by = NULL,
                 updated_at = NOW()
             WHERE id = $1 AND status = 'processing' AND locked_by = $2",
        )
        .bind(self.id)
        .bind(&self.claim_token)
        .execute(&mut *self.transaction)
        .await
        .map_err(|error| AppError::DatabaseError(error.to_string()))?;

        if result.rows_affected() != 1 {
            return Err(AppError::NotFound(format!(
                "Active delivery claim for queue message {}",
                self.id
            )));
        }

        self.transaction
            .commit()
            .await
            .map_err(|error| AppError::DatabaseError(error.to_string()))
    }

    #[allow(dead_code)]
    pub async fn mark_failed(mut self, error: &str) -> Result<(), AppError> {
        let safe_error = crate::utils::sanitize_event_text_for_storage(error);
        let attempts: i32 = sqlx::query_scalar(
            "SELECT attempts FROM telegram_delivery_queue
             WHERE id = $1 AND status = 'processing' AND locked_by = $2",
        )
        .bind(self.id)
        .bind(&self.claim_token)
        .fetch_optional(&mut *self.transaction)
        .await
        .map_err(|database_error| AppError::DatabaseError(database_error.to_string()))?
        .ok_or_else(|| {
            AppError::NotFound(format!(
                "Active delivery claim for queue message {}",
                self.id
            ))
        })?;

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
             WHERE id = $1 AND status = 'processing' AND locked_by = $5",
        )
        .bind(self.id)
        .bind(safe_error)
        .bind(delay)
        .bind(max_attempts)
        .bind(&self.claim_token)
        .execute(&mut *self.transaction)
        .await
        .map_err(|database_error| AppError::DatabaseError(database_error.to_string()))?;

        if result.rows_affected() != 1 {
            return Err(AppError::NotFound(format!(
                "Active delivery claim for queue message {}",
                self.id
            )));
        }

        self.transaction
            .commit()
            .await
            .map_err(|database_error| AppError::DatabaseError(database_error.to_string()))
    }
}

pub async fn acquire_delivery_claim_guard<'a>(
    pool: &'a PgPool,
    id: i64,
    claim_token: &str,
) -> Result<Option<DeliveryClaimGuard<'a>>, AppError> {
    let mut transaction = pool
        .begin()
        .await
        .map_err(|error| AppError::DatabaseError(error.to_string()))?;

    let initial: Option<(i64,)> = sqlx::query_as(
        "SELECT chat_id FROM telegram_delivery_queue
         WHERE id = $1 AND status = 'processing' AND locked_by = $2",
    )
    .bind(id)
    .bind(claim_token)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| AppError::DatabaseError(error.to_string()))?;

    let Some((chat_id,)) = initial else {
        transaction
            .rollback()
            .await
            .map_err(|error| AppError::DatabaseError(error.to_string()))?;
        return Ok(None);
    };

    // Serialize the final authorization-to-send boundary with user subscription
    // mutation and privacy deletion. This lock is scoped to one Telegram chat.
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(chat_id)
        .execute(&mut *transaction)
        .await
        .map_err(|error| AppError::DatabaseError(error.to_string()))?;

    let current: Option<(Option<String>, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
        "SELECT wallet_masked, created_at FROM telegram_delivery_queue
         WHERE id = $1 AND status = 'processing' AND locked_by = $2
         FOR UPDATE",
    )
    .bind(id)
    .bind(claim_token)
    .fetch_optional(&mut *transaction)
    .await
    .map_err(|error| AppError::DatabaseError(error.to_string()))?;

    let Some((wallet_identity, queue_created_at)) = current else {
        transaction
            .rollback()
            .await
            .map_err(|error| AppError::DatabaseError(error.to_string()))?;
        return Ok(None);
    };

    let expected_wallet_token = wallet_identity
        .as_deref()
        .and_then(delivery_wallet_token_from_identity)
        .map(str::to_owned);

    let active_subscription = if let Some(expected_wallet_token) = expected_wallet_token.as_deref()
    {
        let subscriptions: Vec<(String, chrono::DateTime<chrono::Utc>)> = sqlx::query_as(
            "SELECT wallet, created_at FROM user_wallets WHERE chat_id = $1 ORDER BY wallet",
        )
        .bind(chat_id)
        .fetch_all(&mut *transaction)
        .await
        .map_err(|error| AppError::DatabaseError(error.to_string()))?;

        subscriptions.iter().any(|(wallet, created_at)| {
            *created_at <= queue_created_at
                && delivery_wallet_token(wallet) == expected_wallet_token
        })
    } else {
        false
    };

    if !active_subscription {
        let reason = if expected_wallet_token.is_some() {
            "delivery revoked: wallet subscription is absent or newer than this queued event"
        } else {
            "delivery revoked: queue row lacks verifiable wallet identity"
        };
        let safe_reason = crate::utils::sanitize_event_text_for_storage(reason);
        sqlx::query(
            "UPDATE telegram_delivery_queue
             SET status = 'suppressed',
                 last_error = $3,
                 locked_at = NULL,
                 locked_by = NULL,
                 updated_at = NOW()
             WHERE id = $1 AND status = 'processing' AND locked_by = $2",
        )
        .bind(id)
        .bind(claim_token)
        .bind(safe_reason)
        .execute(&mut *transaction)
        .await
        .map_err(|error| AppError::DatabaseError(error.to_string()))?;
        transaction
            .commit()
            .await
            .map_err(|error| AppError::DatabaseError(error.to_string()))?;
        return Ok(None);
    }

    Ok(Some(DeliveryClaimGuard {
        transaction,
        id,
        claim_token: claim_token.to_owned(),
    }))
}

#[allow(dead_code)] // retained for queue characterization/integration tests
pub async fn mark_sent(pool: &PgPool, id: i64, claim_token: &str) -> Result<(), AppError> {
    let result = sqlx::query(
        "UPDATE telegram_delivery_queue
         SET status = 'sent',
             attempts = attempts + 1,
             locked_at = NULL,
             locked_by = NULL,
             updated_at = NOW()
         WHERE id = $1 AND status = 'processing' AND locked_by = $2",
    )
    .bind(id)
    .bind(claim_token)
    .execute(pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    if result.rows_affected() != 1 {
        return Err(AppError::NotFound(format!(
            "Active delivery claim for queue message {id}"
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

#[allow(dead_code)] // retained for queue characterization/integration tests
pub async fn mark_failed(
    pool: &PgPool,
    id: i64,
    claim_token: &str,
    error: &str,
) -> Result<(), AppError> {
    let safe_error = crate::utils::sanitize_event_text_for_storage(error);

    let attempts: i32 =
        sqlx::query_scalar("SELECT attempts FROM telegram_delivery_queue WHERE id = $1 AND status = 'processing' AND locked_by = $2")
            .bind(id)
            .bind(claim_token)
            .fetch_optional(pool)
            .await
            .map_err(|e| AppError::DatabaseError(e.to_string()))?
            .ok_or_else(|| AppError::NotFound(format!("Active delivery claim for queue message {id}")))?;

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
         WHERE id = $1 AND status = 'processing' AND locked_by = $5",
    )
    .bind(id)
    .bind(safe_error)
    .bind(delay)
    .bind(max_attempts)
    .bind(claim_token)
    .execute(pool)
    .await
    .map_err(|e| AppError::DatabaseError(e.to_string()))?;

    if result.rows_affected() != 1 {
        return Err(AppError::NotFound(format!(
            "Active delivery claim for queue message {id}"
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
