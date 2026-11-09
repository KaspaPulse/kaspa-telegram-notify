use chrono::Utc;
use sqlx::PgPool;
use std::time::Duration;
use teloxide::errors::AsResponseParameters;
use teloxide::prelude::*;
use teloxide::types::{ChatId, LinkPreviewOptions, ParseMode};
use tokio::time::MissedTickBehavior;
use tokio_util::sync::CancellationToken;
use tracing::{error, info};

pub fn start_telegram_delivery_worker(bot: Bot, pool: PgPool, token: CancellationToken) {
    crate::infrastructure::resilience::runtime::spawn_resilient(
        "telegram_delivery_worker",
        async move {
            info!("📬 [WORKER] Telegram delivery worker started.");

            let mut delivery_timer = tokio::time::interval(Duration::from_secs(2));
            delivery_timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
            let metrics_interval_seconds = crate::infrastructure::resilience::runtime::env_u64(
                "DELIVERY_QUEUE_METRICS_INTERVAL_SECS",
                30,
            );
            let mut metrics_timer =
                tokio::time::interval(Duration::from_secs(metrics_interval_seconds));
            metrics_timer.set_missed_tick_behavior(MissedTickBehavior::Skip);
            let failed_queue_window_seconds =
                crate::infrastructure::observability::ReadinessPolicy::from_env()
                    .failed_queue_window_seconds;

            loop {
                tokio::select! {
                    _ = token.cancelled() => {
                        info!("[WORKER] Telegram delivery worker shutdown requested.");
                        break;
                    }
                    _ = metrics_timer.tick() => {
                        if crate::infrastructure::telegram_delivery_queue::delivery_queue_enabled() {
                            refresh_queue_metrics(&pool, failed_queue_window_seconds).await;
                        }
                    }
                    _ = delivery_timer.tick() => {
                        if !crate::infrastructure::telegram_delivery_queue::delivery_queue_enabled() {
                            continue;
                        }

                        deliver_pending_batch(&bot, &pool).await;
                    }
                }
            }
        },
    );
}

async fn refresh_queue_metrics(pool: &PgPool, failed_queue_window_seconds: u64) {
    match crate::infrastructure::telegram_delivery_queue::queue_stats(
        pool,
        failed_queue_window_seconds,
    )
    .await
    {
        Ok(stats) => {
            crate::infrastructure::observability::set_queue_snapshot(
                stats.pending,
                stats.processing,
                stats.failed,
                stats.failed_recent,
                stats.oldest_active_age_seconds,
            );
            tracing::debug!(
                "[DELIVERY QUEUE] pending={} processing={} sent={} failed={} failed_recent={} suppressed={} oldest_active_age={}s",
                stats.pending,
                stats.processing,
                stats.sent,
                stats.failed,
                stats.failed_recent,
                stats.suppressed,
                stats.oldest_active_age_seconds
            );
        }
        Err(error) => {
            crate::infrastructure::metrics::inc_db_errors();
            tracing::warn!("[DELIVERY QUEUE] Failed to read queue stats: {}", error);
        }
    }
    // Read-only aggregate from PostgreSQL; refreshed alongside queue health,
    // not once per mined block. Persisted proof rows are never modified.
    let retry_sql = crate::infrastructure::database::pending_rewards_repo::OP076_RETRY_INTERVAL_SQL;
    let query = format!(
        r#"SELECT
      count(*)::bigint,
      count(*) FILTER (WHERE first_seen_at < now()-interval '1 day')::bigint,
      coalesce(extract(epoch FROM now()-min(first_seen_at))::bigint,0),
      count(*) FILTER (WHERE coalesce(metadata->>'accepting_block_hash','')='')::bigint,
      coalesce(max(attempts),0)::bigint,
      count(*) FILTER (
          WHERE first_seen_at >= TIMESTAMPTZ '2026-11-05 17:30:32+00'
            AND first_seen_at < now()-interval '10 minutes'
      )::bigint,
      count(*) FILTER (
          WHERE first_seen_at >= TIMESTAMPTZ '2026-11-05 17:30:32+00'
      )::bigint,
      count(*) FILTER (
          WHERE first_seen_at < TIMESTAMPTZ '2026-11-05 17:30:32+00'
      )::bigint,
      coalesce(max(attempts) FILTER (
          WHERE first_seen_at >= TIMESTAMPTZ '2026-11-05 17:30:32+00'
      ),0)::bigint,
      count(*) FILTER (
          WHERE first_seen_at >= TIMESTAMPTZ '2026-11-05 17:30:32+00'
            AND last_checked_at < NOW()-{retry_sql}
      )::bigint
      FROM pending_rewards WHERE status='awaiting_block_proof'"#
    );
    match sqlx::query_as::<_, (i64, i64, i64, i64, i64, i64, i64, i64, i64, i64)>(
        sqlx::AssertSqlSafe(query),
    )
    .fetch_one(pool)
    .await
    {
        Ok((
            count,
            old_day,
            max_age,
            missing,
            max_retries,
            newer_over_10m,
            newer_waiting,
            historic_excluded,
            newer_max_attempts,
            newer_due,
        )) => {
            crate::infrastructure::metrics::set_proof_backlog(
                crate::infrastructure::metrics::ProofBacklogSnapshot {
                    count: count.max(0) as u64,
                    old_day: old_day.max(0) as u64,
                    max_age: max_age.max(0) as u64,
                    missing_acceptance: missing.max(0) as u64,
                    max_retries: max_retries.max(0) as u64,
                    newer_over_10m: newer_over_10m.max(0) as u64,
                    newer_waiting: newer_waiting.max(0) as u64,
                    historic_excluded: historic_excluded.max(0) as u64,
                    newer_max_attempts: newer_max_attempts.max(0) as u64,
                    newer_due_for_retry: newer_due.max(0) as u64,
                },
            );
        }
        Err(error) => {
            crate::infrastructure::metrics::inc_db_errors();
            tracing::warn!("[PROOF METRICS] Database snapshot unavailable: {}", error);
        }
    }
}

async fn deliver_pending_batch(bot: &Bot, pool: &PgPool) {
    let batch =
        match crate::infrastructure::telegram_delivery_queue::fetch_pending_batch(pool, 25).await {
            Ok(batch) => batch,
            Err(error) => {
                crate::infrastructure::metrics::inc_db_errors();
                error!(
                    "[DELIVERY QUEUE] Failed to fetch pending messages: {}",
                    error
                );
                return;
            }
        };

    for item in batch {
        let claim =
            match crate::infrastructure::telegram_delivery_queue::acquire_delivery_claim_guard(
                pool,
                item.id,
                &item.claim_token,
            )
            .await
            {
                Ok(Some(claim)) => claim,
                Ok(None) => {
                    info!(
                        "[DELIVERY QUEUE] Skipping stale, revoked, or unsubscribed delivery claim id={} chat={}",
                        item.id, item.chat_id
                    );
                    continue;
                }
                Err(error) => {
                    crate::infrastructure::metrics::inc_db_errors();
                    error!(
                        "[DELIVERY QUEUE] Refusing to send id {} because durable claim authorization failed: {}",
                        item.id, error
                    );
                    continue;
                }
            };

        crate::utils::log_multiline(
            &format!("📤 [BOT QUEUE OUT] Chat: {}", item.chat_id),
            &item.message_html,
            true,
        );

        let send_result = bot
            .send_message(ChatId(item.chat_id), &item.message_html)
            .parse_mode(ParseMode::Html)
            .link_preview_options(LinkPreviewOptions {
                url: None,
                is_disabled: true,
                show_above_text: false,
                prefer_small_media: false,
                prefer_large_media: false,
            })
            .await;

        match send_result {
            Ok(_) => {
                let delivered_at = Utc::now();
                let latency_ms = delivered_at
                    .signed_duration_since(item.created_at)
                    .num_milliseconds()
                    .max(0) as u64;

                if let Err(error) = claim.mark_sent().await {
                    crate::infrastructure::metrics::inc_db_errors();
                    error!(
                        "[DELIVERY QUEUE] Telegram accepted id {} but durable sent acknowledgement failed: {}",
                        item.id, error
                    );
                    continue;
                }

                crate::infrastructure::metrics::inc_alerts_delivered();
                crate::infrastructure::observability::observe_delivery_latency(latency_ms);
                crate::infrastructure::observability::mark_telegram_delivery(
                    delivered_at.timestamp().max(0) as u64,
                );

                info!(
                    "✅ [QUEUED ALERT DELIVERED] id={} | chat={} | wallet={} | txid={} | block={} | amount_kas={} | daa_score={}",
                    item.id,
                    item.chat_id,
                    item.wallet_masked.as_deref().unwrap_or("unknown"),
                    item.txid_masked.as_deref().unwrap_or("unknown"),
                    item.block_hash_masked.as_deref().unwrap_or("unknown"),
                    item.amount_kas
                        .map(|value| format!("{:.8}", value))
                        .unwrap_or_else(|| "unknown".to_string()),
                    item.daa_score
                        .map(|value| value.to_string())
                        .unwrap_or_else(|| "unknown".to_string())
                );
            }
            Err(error) => {
                crate::infrastructure::metrics::inc_telegram_send_failures();

                let (error_text, retry_after) = normalize_delivery_error(&error);

                if let Some(retry_after) = retry_after {
                    tracing::warn!(
                        "[TELEGRAM RATE LIMIT] retry_after={}s for queued alert id={}",
                        retry_after,
                        item.id
                    );
                }

                if let Err(database_error) = claim.mark_failed(&error_text).await {
                    crate::infrastructure::metrics::inc_db_errors();
                    error!(
                        "[DELIVERY QUEUE] Failed to mark failed id {}: {}",
                        item.id, database_error
                    );
                }

                error!(
                    "[TELEGRAM ERROR] Queued alert send failed. id={} chat={} error={}",
                    item.id, item.chat_id, error_text
                );
            }
        }
    }
}

fn normalize_delivery_error(error: &teloxide::RequestError) -> (String, Option<i64>) {
    let retry_after = error
        .retry_after()
        .map(|seconds| i64::from(seconds.seconds()));

    let error_text = match retry_after {
        Some(seconds) => format!("retry_after {seconds}; {error}"),
        None => error.to_string(),
    };

    (error_text, retry_after)
}

#[cfg(test)]
mod tests {
    use super::*;
    use teloxide::types::Seconds;

    #[test]
    fn structured_retry_after_is_normalized_for_queue_scheduling() {
        let error = teloxide::RequestError::RetryAfter(Seconds::from_seconds(17));
        let (error_text, retry_after) = normalize_delivery_error(&error);

        assert_eq!(retry_after, Some(17));
        assert!(error_text.starts_with("retry_after 17;"));
        assert_eq!(
            crate::infrastructure::telegram_delivery_queue::retry_after_seconds(&error_text),
            Some(17)
        );
    }
}
