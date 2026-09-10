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
                crate::infrastructure::metrics::inc_alerts_delivered();
                let delivered_at = Utc::now();
                let latency_ms = delivered_at
                    .signed_duration_since(item.created_at)
                    .num_milliseconds()
                    .max(0) as u64;
                crate::infrastructure::observability::observe_delivery_latency(latency_ms);
                crate::infrastructure::observability::mark_telegram_delivery(
                    delivered_at.timestamp().max(0) as u64,
                );

                if let Err(error) =
                    crate::infrastructure::telegram_delivery_queue::mark_sent(pool, item.id).await
                {
                    crate::infrastructure::metrics::inc_db_errors();
                    error!(
                        "[DELIVERY QUEUE] Failed to mark sent id {}: {}",
                        item.id, error
                    );
                }

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

                if let Err(database_error) =
                    crate::infrastructure::telegram_delivery_queue::mark_failed(
                        pool,
                        item.id,
                        &error_text,
                    )
                    .await
                {
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
