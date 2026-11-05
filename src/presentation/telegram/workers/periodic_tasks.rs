use std::sync::Arc;
use std::time::Duration;
use tokio_util::sync::CancellationToken;
use tracing::info;

use crate::application::background_jobs::SystemTasksUseCase;

pub fn start_system_monitors(sys_tasks: Arc<SystemTasksUseCase>, token: CancellationToken) {
    let sys_gc = sys_tasks.clone();
    let sys_price = sys_tasks.clone();
    let price_token = token.clone();

    crate::infrastructure::resilience::runtime::spawn_resilient(
        "periodic_system_task",
        async move {
            loop {
                tokio::select! {
                    _ = token.cancelled() => {
                        info!("[MEMORY CLEANER] Scheduled cleanup worker shutdown requested.");
                        break;
                    }
                    _ = tokio::time::sleep(Duration::from_secs(3600)) => {
                        info!("[MEMORY CLEANER] Running scheduled cleanup.");
                        sys_gc.execute_memory_cleanup().await;
                    }
                }
            }
        },
    );

    crate::infrastructure::resilience::runtime::spawn_resilient(
        "periodic_kas_price_sync",
        run_periodic_price_sync_worker(sys_price, price_token),
    );
}

async fn execute_price_sync_until_cancel(
    sys_price: &SystemTasksUseCase,
    price_token: &CancellationToken,
) -> bool {
    tokio::select! {
        biased;
        _ = price_token.cancelled() => false,
        _ = sys_price.execute_kas_price_sync() => true,
    }
}

async fn run_periodic_price_sync_worker(
    sys_price: Arc<SystemTasksUseCase>,
    price_token: CancellationToken,
) {
    if !execute_price_sync_until_cancel(&sys_price, &price_token).await {
        info!("[KAS PRICE] Initial price sync skipped or cancelled during shutdown.");
        return;
    }

    loop {
        tokio::select! {
            biased;
            _ = price_token.cancelled() => {
                info!("[KAS PRICE] Scheduled price sync worker shutdown requested.");
                break;
            }
            _ = tokio::time::sleep(Duration::from_secs(env_u64(
                "KAS_PRICE_REFRESH_INTERVAL_SECS",
                3600,
            ))) => {
                info!("[KAS PRICE] Running scheduled stored KAS/USD price sync.");
                if !execute_price_sync_until_cancel(&sys_price, &price_token).await {
                    info!("[KAS PRICE] In-flight price sync cancelled during shutdown.");
                    break;
                }
            }
        }
    }
}

fn env_u64(key: &str, default_value: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .unwrap_or(default_value)
}

#[cfg(test)]
mod cancellation_tests {
    use super::*;
    use crate::domain::errors::AppError;
    use crate::infrastructure::database::postgres_adapter::PostgresRepository;
    use crate::infrastructure::market::coingecko_adapter::MarketProvider;
    use async_trait::async_trait;
    use sqlx::postgres::PgPoolOptions;
    use std::sync::atomic::{AtomicUsize, Ordering};

    struct CountingMarketProvider {
        calls: Arc<AtomicUsize>,
    }

    #[async_trait]
    impl MarketProvider for CountingMarketProvider {
        async fn get_kaspa_market_data(&self) -> Result<(f64, f64), AppError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok((0.1, 1_000_000.0))
        }

        async fn get_kaspa_usd_history(
            &self,
            _from_unix: i64,
            _to_unix: i64,
        ) -> Result<Vec<(String, f64)>, AppError> {
            self.calls.fetch_add(1, Ordering::SeqCst);
            Ok(Vec::new())
        }
    }

    struct BlockingMarketProvider {
        started: Arc<tokio::sync::Notify>,
    }

    #[async_trait]
    impl MarketProvider for BlockingMarketProvider {
        async fn get_kaspa_market_data(&self) -> Result<(f64, f64), AppError> {
            self.started.notify_one();
            std::future::pending::<Result<(f64, f64), AppError>>().await
        }

        async fn get_kaspa_usd_history(
            &self,
            _from_unix: i64,
            _to_unix: i64,
        ) -> Result<Vec<(String, f64)>, AppError> {
            std::future::pending::<Result<Vec<(String, f64)>, AppError>>().await
        }
    }

    #[tokio::test]
    async fn pre_cancelled_price_worker_starts_no_provider_or_database_work() {
        let calls = Arc::new(AtomicUsize::new(0));
        let provider = Arc::new(CountingMarketProvider {
            calls: calls.clone(),
        });
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .acquire_timeout(Duration::from_millis(80))
            .connect_lazy("postgres://cancel_test@127.0.0.1:1/cancel_test?sslmode=disable")
            .unwrap();
        let tasks = Arc::new(SystemTasksUseCase::new(
            Arc::new(PostgresRepository::new(pool)),
            provider,
        ));
        let token = CancellationToken::new();
        token.cancel();

        tokio::time::timeout(
            Duration::from_millis(400),
            run_periodic_price_sync_worker(tasks, token),
        )
        .await
        .expect("a pre-cancelled worker must return without starting work");
        assert_eq!(
            calls.load(Ordering::SeqCst),
            0,
            "pre-cancelled worker started external provider work"
        );
    }

    #[tokio::test]
    async fn cancellation_interrupts_inflight_initial_price_provider_work() {
        let started = Arc::new(tokio::sync::Notify::new());
        let provider = Arc::new(BlockingMarketProvider {
            started: started.clone(),
        });
        let pool = PgPoolOptions::new()
            .max_connections(1)
            .connect_lazy("postgres://cancel_test@127.0.0.1:1/cancel_test?sslmode=disable")
            .unwrap();
        let tasks = Arc::new(SystemTasksUseCase::new(
            Arc::new(PostgresRepository::new(pool)),
            provider,
        ));
        let token = CancellationToken::new();
        let worker_token = token.clone();
        let worker = tokio::spawn(run_periodic_price_sync_worker(tasks, worker_token));

        tokio::time::timeout(Duration::from_millis(200), started.notified())
            .await
            .expect("synthetic provider must enter its blocking request");
        token.cancel();
        tokio::time::timeout(Duration::from_millis(200), worker)
            .await
            .expect("shutdown must cancel the in-flight provider future")
            .unwrap();
    }
}
