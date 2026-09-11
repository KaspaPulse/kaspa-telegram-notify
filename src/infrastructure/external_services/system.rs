use crate::domain::errors::AppError;
use crate::domain::models::{BotEventType, EventSeverity};
use kaspa_rpc_core::api::rpc::RpcApi;
use std::sync::atomic::Ordering;
use teloxide::prelude::*;
use teloxide::types::ChatId;
use tokio::time::Duration;
use tokio_util::sync::CancellationToken;

use crate::domain::models::AppContext;

fn apply_price_refresh_result(
    result: Result<(), String>,
    consecutive_failures: &mut u32,
    circuit_open_until: &mut Option<chrono::DateTime<chrono::Utc>>,
    now: chrono::DateTime<chrono::Utc>,
    cooldown_secs: u64,
) {
    match result {
        Ok(()) => {
            if *consecutive_failures > 0 {
                tracing::info!(
                    "[PRICE MONITOR] External price API recovered after {} failures.",
                    *consecutive_failures
                );
            }
            *consecutive_failures = 0;
            *circuit_open_until = None;
        }
        Err(error) => {
            *consecutive_failures = consecutive_failures.saturating_add(1);
            tracing::error!(
                "[PRICE MONITOR] Failed to refresh price cache. failures={} error={}",
                *consecutive_failures,
                error
            );

            if *consecutive_failures >= 3 {
                *circuit_open_until = Some(now + chrono::Duration::seconds(cooldown_secs as i64));
                tracing::error!(
                    "[PRICE MONITOR] Circuit opened for {} seconds. Bot will keep using cached price.",
                    cooldown_secs
                );
            }
        }
    }
}

pub fn spawn_price_monitor(ctx: AppContext, token: CancellationToken) -> Result<(), AppError> {
    let client = crate::infrastructure::resilience::runtime::build_http_client()?;

    crate::infrastructure::resilience::runtime::spawn_resilient("price_monitor", async move {
        let mut consecutive_failures: u32 = 0;
        let mut circuit_open_until: Option<chrono::DateTime<chrono::Utc>> = None;

        async fn fetch_number(
            client: &reqwest::Client,
            url: &'static str,
            field: &'static str,
        ) -> Result<f64, String> {
            let response = crate::infrastructure::resilience::runtime::with_timeout_result(
                "kaspa.org price API request",
                crate::infrastructure::resilience::runtime::http_timeout_duration(),
                client.get(url).send(),
            )
            .await?;

            let json = crate::infrastructure::resilience::runtime::with_timeout_result(
                "kaspa.org price API json parse",
                crate::infrastructure::resilience::runtime::http_timeout_duration(),
                response.json::<serde_json::Value>(),
            )
            .await?;

            Ok(json[field].as_f64().unwrap_or(0.0))
        }

        async fn update_price_cache(
            client: &reqwest::Client,
            ctx: &AppContext,
        ) -> Result<(), String> {
            let mut last_error = String::new();

            for attempt in 1..=3 {
                let price = fetch_number(client, "https://api.kaspa.org/info/price", "price").await;
                let marketcap =
                    fetch_number(client, "https://api.kaspa.org/info/marketcap", "marketcap").await;

                match (price, marketcap) {
                    (Ok(p), Ok(m)) if p > 0.0 => {
                        let mut write_guard = ctx.price_cache.write().await;
                        *write_guard = (p, m);
                        return Ok(());
                    }
                    (p_result, m_result) => {
                        last_error = format!(
                            "attempt {} failed, price={:?}, marketcap={:?}",
                            attempt,
                            p_result.err(),
                            m_result.err()
                        );
                        tracing::warn!("[PRICE MONITOR] {}", last_error);
                        tokio::time::sleep(Duration::from_secs(attempt * 2)).await;
                    }
                }
            }

            Err(last_error)
        }

        let cooldown_secs = crate::infrastructure::resilience::runtime::env_u64(
            "PRICE_API_CIRCUIT_COOLDOWN_SECS",
            300,
        );
        let initial_result = update_price_cache(&client, &ctx).await;
        apply_price_refresh_result(
            initial_result,
            &mut consecutive_failures,
            &mut circuit_open_until,
            chrono::Utc::now(),
            cooldown_secs,
        );

        loop {
            tokio::select! {
                _ = token.cancelled() => {
                    tracing::info!("[PRICE MONITOR] Cancellation requested.");
                    break;
                }
                _ = tokio::time::sleep(Duration::from_secs(60)) => {
                    if let Some(open_until) = circuit_open_until {
                        if chrono::Utc::now() < open_until {
                            tracing::warn!("[PRICE MONITOR] Circuit open. Serving last cached price and skipping this cycle.");
                            continue;
                        }

                        tracing::info!("[PRICE MONITOR] Circuit moved to half-open. Trying API again.");
                        circuit_open_until = None;
                    }

                    let refresh_result = update_price_cache(&client, &ctx).await;
                    apply_price_refresh_result(
                        refresh_result,
                        &mut consecutive_failures,
                        &mut circuit_open_until,
                        chrono::Utc::now(),
                        cooldown_secs,
                    );
                }
            }
        }
    });

    Ok(())
}
pub fn spawn_node_monitor(ctx: AppContext, bot: Bot, token: CancellationToken) {
    crate::infrastructure::resilience::runtime::spawn_resilient(
        "system_background_task",
        async move {
            let mut failed_attempts = 0;
            let mut is_disconnected = false;
            let initial_connected = ctx.rpc.connect(None).await.is_ok();
            crate::infrastructure::observability::set_node_connected(initial_connected);

            tokio::time::sleep(Duration::from_secs(10)).await;

            loop {
                tokio::select! {
                    _ = token.cancelled() => { break; }
                    _ = tokio::time::sleep(Duration::from_secs(60)) => {
                        let health_check = tokio::time::timeout(
                            crate::infrastructure::resilience::runtime::rpc_timeout_duration(),
                            ctx.rpc.get_server_info()
                        ).await;

                        if health_check.is_err() {
                            crate::infrastructure::metrics::inc_rpc_timeouts();
                        }

                        if health_check.is_err() || health_check.as_ref().is_ok_and(|inner| inner.is_err()) {
                            crate::infrastructure::observability::set_node_connected(false);
                            failed_attempts += 1;
                            tracing::error!("[NODE ALERT] RPC Connection Lost! Attempt {}...", failed_attempts);
                            if let Err(error) = sqlx::query(
                                "INSERT INTO bot_event_log (event_type, severity, status, error_message, metadata)
                                 VALUES ($1, $2, 'node_unreachable', 'RPC connection lost', $3::jsonb)",
                            )
                            .bind(BotEventType::RpcError.as_str())
                            .bind(EventSeverity::Error.as_str())
                            .bind(format!(r#"{{"attempt":{}}}"#, failed_attempts))
                            .execute(&ctx.pool)
                            .await
                            {
                                crate::infrastructure::metrics::inc_db_errors();
                                tracing::error!(
                                    "[DATABASE ERROR] Failed to record RPC outage event: {}",
                                    error
                                );
                            }

                            if failed_attempts == 1 {
                                is_disconnected = true;
                                // Safe sleep mode
                                ctx.live_sync_enabled.store(false, Ordering::Relaxed);
                                if let Err(e) = bot.send_message(ChatId(ctx.admin_chat_id), "⚠️ <b>WARNING:</b> Primary Node connection dropped!\n⏸️ UTXO Monitoring paused safely.\n🔄 Attempting background recovery...")
                                    .parse_mode(teloxide::types::ParseMode::Html).await { tracing::error!("[TELEGRAM ERROR] Bot API request failed: {}", e); }
                            }

                            if failed_attempts % 10 == 0
                                && let Err(e) = bot.send_message(ChatId(ctx.admin_chat_id), format!("🚨 <b>CRITICAL:</b> Node still unreachable after {} attempts. Continuing to retry quietly...", failed_attempts))
                                    .parse_mode(teloxide::types::ParseMode::Html).await { tracing::error!("[TELEGRAM ERROR] Bot API request failed: {}", e); }

                            let _ = ctx.rpc.connect(None).await;
                        } else {
                            crate::infrastructure::observability::set_node_connected(true);
                            if is_disconnected {
                                tracing::info!("[NODE RECOVERED] RPC Tunnel stabilized.");
                                if let Err(error) = sqlx::query(
                                    "INSERT INTO bot_event_log (event_type, severity, status, metadata)
                                     VALUES ($1, $2, 'recovered', $3::jsonb)",
                                )
                                .bind(BotEventType::RpcRecovered.as_str())
                                .bind(EventSeverity::Info.as_str())
                                .bind(format!(r#"{{"failed_attempts":{}}}"#, failed_attempts))
                                .execute(&ctx.pool)
                                .await
                                {
                                    crate::infrastructure::metrics::inc_db_errors();
                                    tracing::error!(
                                        "[DATABASE ERROR] Failed to record RPC recovery event: {}",
                                        error
                                    );
                                }
                                ctx.live_sync_enabled.store(true, Ordering::Relaxed);
                                if let Err(e) = bot.send_message(ChatId(ctx.admin_chat_id), "✅ <b>RECOVERED:</b> Node connection stabilized.\n▶️ UTXO Monitoring resumed smoothly.")
                                    .parse_mode(teloxide::types::ParseMode::Html).await { tracing::error!("[TELEGRAM ERROR] Bot API request failed: {}", e); }

                                failed_attempts = 0;
                                is_disconnected = false;
                            }
                        }
                    }
                }
            }
        },
    );
}

pub fn spawn_memory_cleaner(ctx: AppContext, token: CancellationToken) {
    crate::infrastructure::resilience::runtime::spawn_resilient(
        "system_background_task",
        async move {
            loop {
                tokio::select! {
                    _ = token.cancelled() => { break; }
                    _ = tokio::time::sleep(Duration::from_secs(3600)) => {
                        ctx.utxo_state.retain(|wallet, _| ctx.state.contains_key(wallet));
                        ctx.rate_limiter.retain_recent();
                        let retention_days: i64 = std::env::var("BOT_EVENT_LOG_RETENTION_DAYS")
                            .ok()
                            .and_then(|v| v.parse::<i64>().ok())
                            .unwrap_or(60)
                            .clamp(1, 365);

                        let purge_result = sqlx::query(
                            "DELETE FROM bot_event_log
                             WHERE created_at < NOW() - ($1::text || ' days')::interval"
                        )
                        .bind(retention_days.to_string())
                        .execute(&ctx.pool)
                        .await;

                        match purge_result {
                            Ok(result) => {
                                tracing::info!(
                                    "[MEMORY CLEANER] Purged in-memory runtime state and {} old bot events.",
                                    result.rows_affected()
                                );
                            }
                            Err(e) => {
                                tracing::error!("[DATABASE ERROR] Failed to purge old bot events: {}", e);
                            }
                        }
                    }
                }
            }
        },
    );
}

#[cfg(test)]
mod price_monitor_state_tests {
    use super::apply_price_refresh_result;
    use chrono::{Duration as ChronoDuration, TimeZone, Utc};

    #[test]
    fn startup_failure_counts_toward_circuit_breaker_and_recovery_resets_state() {
        let now = Utc.with_ymd_and_hms(2026, 9, 11, 20, 0, 0).unwrap();
        let mut failures = 0;
        let mut circuit_open_until = None;

        apply_price_refresh_result(
            Err("startup refresh failed".to_string()),
            &mut failures,
            &mut circuit_open_until,
            now,
            300,
        );
        assert_eq!(failures, 1);
        assert!(circuit_open_until.is_none());

        apply_price_refresh_result(
            Err("second refresh failed".to_string()),
            &mut failures,
            &mut circuit_open_until,
            now + ChronoDuration::minutes(1),
            300,
        );
        assert_eq!(failures, 2);
        assert!(circuit_open_until.is_none());

        let third_failure_at = now + ChronoDuration::minutes(2);
        apply_price_refresh_result(
            Err("third refresh failed".to_string()),
            &mut failures,
            &mut circuit_open_until,
            third_failure_at,
            300,
        );
        assert_eq!(failures, 3);
        assert_eq!(
            circuit_open_until,
            Some(third_failure_at + ChronoDuration::seconds(300))
        );

        apply_price_refresh_result(
            Ok(()),
            &mut failures,
            &mut circuit_open_until,
            now + ChronoDuration::minutes(8),
            300,
        );
        assert_eq!(failures, 0);
        assert!(circuit_open_until.is_none());
    }
}
