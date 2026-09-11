use crate::domain::errors::AppError;
use std::future::Future;
use std::sync::OnceLock;
use std::sync::atomic::{AtomicUsize, Ordering};
use tokio::sync::Notify;
use tokio::task::JoinHandle;
use tokio::time::{Duration, timeout};

pub fn env_u64(key: &str, default_value: u64) -> u64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<u64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default_value)
}

pub fn rpc_timeout_duration() -> Duration {
    Duration::from_secs(env_u64("RPC_TIMEOUT_SECS", 15))
}

pub fn http_timeout_duration() -> Duration {
    Duration::from_secs(env_u64("HTTP_TIMEOUT_SECS", 10))
}

pub fn application_user_agent() -> String {
    format!("KaspaPulse/{}", env!("CARGO_PKG_VERSION"))
}

pub fn build_http_client() -> Result<reqwest::Client, AppError> {
    build_http_client_with_user_agent(&application_user_agent())
}

fn build_http_client_with_user_agent(user_agent: &str) -> Result<reqwest::Client, AppError> {
    reqwest::Client::builder()
        .timeout(http_timeout_duration())
        .connect_timeout(Duration::from_secs(env_u64("HTTP_CONNECT_TIMEOUT_SECS", 5)))
        .user_agent(user_agent)
        .build()
        .map_err(|error| AppError::ApiError(format!("HTTP client initialization failed: {error}")))
}

pub async fn with_rpc_timeout<T, F>(operation: &'static str, future: F) -> Result<T, AppError>
where
    F: Future<Output = Result<T, AppError>>,
{
    match timeout(rpc_timeout_duration(), future).await {
        Ok(result) => result,
        Err(_) => Err(AppError::NodeConnection(format!(
            "RPC timeout while running {} after {} seconds",
            operation,
            rpc_timeout_duration().as_secs()
        ))),
    }
}

pub async fn with_timeout_result<T, E, F>(
    operation: &'static str,
    duration: Duration,
    future: F,
) -> Result<T, String>
where
    E: std::fmt::Display,
    F: Future<Output = Result<T, E>>,
{
    match timeout(duration, future).await {
        Ok(Ok(value)) => Ok(value),
        Ok(Err(error)) => Err(format!("{} failed: {}", operation, error)),
        Err(_) => Err(format!(
            "{} timed out after {} seconds",
            operation,
            duration.as_secs()
        )),
    }
}

fn active_task_count() -> &'static AtomicUsize {
    static ACTIVE_TASKS: AtomicUsize = AtomicUsize::new(0);
    &ACTIVE_TASKS
}

fn task_completion_notify() -> &'static Notify {
    static TASK_COMPLETION: OnceLock<Notify> = OnceLock::new();
    TASK_COMPLETION.get_or_init(Notify::new)
}

struct ActiveTaskGuard;

impl Drop for ActiveTaskGuard {
    fn drop(&mut self) {
        active_task_count().fetch_sub(1, Ordering::AcqRel);
        task_completion_notify().notify_one();
    }
}

pub async fn drain_tracked_tasks(duration: Duration) -> bool {
    timeout(duration, async {
        loop {
            if active_task_count().load(Ordering::Acquire) == 0 {
                break;
            }

            task_completion_notify().notified().await;
        }
    })
    .await
    .is_ok()
}

pub fn spawn_resilient<F>(task_name: &'static str, future: F) -> JoinHandle<()>
where
    F: Future<Output = ()> + Send + 'static,
{
    active_task_count().fetch_add(1, Ordering::AcqRel);
    let guard = ActiveTaskGuard;

    let worker = tokio::spawn(async move {
        tracing::info!("[TASK START] {}", task_name);
        future.await;
        tracing::info!("[TASK STOP] {} finished normally", task_name);
    });

    tokio::spawn(async move {
        let _guard = guard;

        match worker.await {
            Ok(_) => {
                tracing::info!("[TASK MONITOR] {} joined cleanly", task_name);
            }
            Err(error) if error.is_panic() => {
                tracing::error!(
                    "[TASK PANIC] {} crashed with panic. Global panic hook should record marker if process-level panic occurs.",
                    task_name
                );
            }
            Err(error) if error.is_cancelled() => {
                tracing::warn!("[TASK CANCELLED] {} was cancelled", task_name);
            }
            Err(error) => {
                tracing::error!("[TASK ERROR] {} join error: {}", task_name, error);
            }
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn application_user_agent_tracks_package_version() {
        assert_eq!(
            application_user_agent(),
            format!("KaspaPulse/{}", env!("CARGO_PKG_VERSION"))
        );
    }

    #[test]
    fn invalid_user_agent_returns_error_without_panicking() {
        let result = build_http_client_with_user_agent("invalid\nuser-agent");
        assert!(matches!(result, Err(AppError::ApiError(_))));
    }
}
