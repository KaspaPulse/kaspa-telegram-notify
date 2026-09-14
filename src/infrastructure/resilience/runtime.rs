use crate::domain::errors::AppError;
use std::future::Future;
use std::sync::OnceLock;
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

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TaskSnapshot {
    pub id: u64,
    pub name: &'static str,
    pub stage: &'static str,
    pub elapsed_ms: u128,
}

struct TaskRecord {
    id: u64,
    name: &'static str,
    stage: std::sync::Arc<std::sync::Mutex<&'static str>>,
    started: std::time::Instant,
    abort: tokio::task::AbortHandle,
    monitor: JoinHandle<()>,
}

impl TaskRecord {
    fn snapshot(&self) -> TaskSnapshot {
        TaskSnapshot {
            id: self.id,
            name: self.name,
            stage: *self.stage.lock().unwrap_or_else(|e| e.into_inner()),
            elapsed_ms: self.started.elapsed().as_millis(),
        }
    }
}

#[derive(Default)]
struct TaskRegistry {
    closed: bool,
    next_id: u64,
    tasks: Vec<TaskRecord>,
}

// A drain future can be cancelled by its caller. Keep every remaining monitor
// recoverable until it is joined; dropping a JoinHandle alone would detach it.
struct DrainOwnership<'a> {
    registry: &'a std::sync::Mutex<TaskRegistry>,
    tasks: Vec<TaskRecord>,
}

impl Drop for DrainOwnership<'_> {
    fn drop(&mut self) {
        if !self.tasks.is_empty() {
            self.registry
                .lock()
                .unwrap_or_else(|e| e.into_inner())
                .tasks
                .append(&mut self.tasks);
        }
    }
}

#[derive(Debug, Default)]
pub struct ShutdownReport {
    pub cancelled: Vec<TaskSnapshot>,
}

#[derive(Debug, thiserror::Error)]
#[error("tasks did not join after cancellation; database must remain open: {remaining:?}")]
pub struct ShutdownError {
    pub remaining: Vec<TaskSnapshot>,
}

/// Owns both workers and their join monitors. A dropped result receiver cannot
/// detach DB work from shutdown. Each process (or test lifecycle) has one owner.
#[derive(Default)]
pub struct TaskSupervisor {
    registry: std::sync::Mutex<TaskRegistry>,
    shutdown_lock: tokio::sync::Mutex<()>,
    cancellation: tokio_util::sync::CancellationToken,
}

pub struct TrackedTask<T> {
    result: tokio::sync::oneshot::Receiver<Result<T, tokio::task::JoinError>>,
}

impl<T> TrackedTask<T> {
    pub async fn join(self) -> Result<T, tokio::task::JoinError> {
        self.result
            .await
            .expect("task supervisor owns the join monitor")
    }
}

tokio::task_local! {
    static TASK_STAGE: std::sync::Arc<std::sync::Mutex<&'static str>>;
}

pub fn task_stage(stage: &'static str) {
    let _ = TASK_STAGE.try_with(|current| {
        *current.lock().unwrap_or_else(|e| e.into_inner()) = stage;
    });
}

impl TaskSupervisor {
    pub fn cancellation_token(&self) -> tokio_util::sync::CancellationToken {
        self.cancellation.clone()
    }

    pub fn begin_shutdown(&self) {
        self.registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .closed = true;
        self.cancellation.cancel();
    }

    pub fn active_tasks(&self) -> Vec<TaskSnapshot> {
        self.registry
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .tasks
            .iter()
            .filter(|task| !task.monitor.is_finished())
            .map(TaskRecord::snapshot)
            .collect()
    }

    pub fn spawn<T, F>(&self, name: &'static str, future: F) -> Option<TrackedTask<T>>
    where
        T: Send + 'static,
        F: Future<Output = T> + Send + 'static,
    {
        let mut registry = self.registry.lock().unwrap_or_else(|e| e.into_inner());
        if registry.closed {
            drop(registry);
            tracing::debug!(
                task = name,
                "[TASK REJECTED] Shutdown has stopped new work."
            );
            return None;
        }
        registry.tasks.retain(|task| !task.monitor.is_finished());
        registry.next_id += 1;
        let id = registry.next_id;
        let stage = std::sync::Arc::new(std::sync::Mutex::new("running"));
        let worker = tokio::spawn(TASK_STAGE.scope(stage.clone(), async move {
            tracing::info!(task_id = id, "[TASK START] {}", name);
            let result = future.await;
            tracing::info!(task_id = id, "[TASK STOP] {} finished normally", name);
            result
        }));
        let abort = worker.abort_handle();
        let (sender, result) = tokio::sync::oneshot::channel();
        let monitor = tokio::spawn(async move {
            let outcome = worker.await;
            match &outcome {
                Ok(_) => tracing::info!(task_id = id, "[TASK MONITOR] {} joined cleanly", name),
                Err(error) if error.is_cancelled() => tracing::warn!(
                    task_id = id,
                    "[TASK CANCELLED] {} joined after cancellation",
                    name
                ),
                Err(error) => {
                    tracing::error!(task_id = id, "[TASK PANIC] {} join failed: {}", name, error)
                }
            }
            let _ = sender.send(outcome);
        });
        registry.tasks.push(TaskRecord {
            id,
            name,
            stage,
            started: std::time::Instant::now(),
            abort,
            monitor,
        });
        Some(TrackedTask { result })
    }

    pub async fn shutdown(&self, duration: Duration) -> Result<ShutdownReport, ShutdownError> {
        let _shutdown = self.shutdown_lock.lock().await;
        self.begin_shutdown();
        let mut owned = DrainOwnership {
            registry: &self.registry,
            tasks: std::mem::take(
                &mut self
                    .registry
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .tasks,
            ),
        };
        let mut report = ShutdownReport::default();
        if timeout(duration, join_workers(&mut owned.tasks))
            .await
            .is_err()
        {
            report.cancelled = owned
                .tasks
                .iter()
                .filter(|t| !t.monitor.is_finished())
                .map(TaskRecord::snapshot)
                .collect();
            tracing::warn!(remaining = ?report.cancelled,
                "[SHUTDOWN DEADLINE] Cancelling owned tasks; database remains open until joins complete.");
            for task in &owned.tasks {
                task.abort.abort();
            }
            if timeout(duration, join_workers(&mut owned.tasks))
                .await
                .is_err()
            {
                let remaining = owned
                    .tasks
                    .iter()
                    .filter(|t| !t.monitor.is_finished())
                    .map(TaskRecord::snapshot)
                    .collect::<Vec<_>>();
                tracing::error!(remaining = ?remaining,
                    "[SHUTDOWN BLOCKED] Tasks still alive after cancellation; database close refused.");
                return Err(ShutdownError { remaining });
            }
        }
        Ok(report)
    }
}

async fn join_workers(tasks: &mut Vec<TaskRecord>) {
    // Borrow the handle across await: a timeout must never drop ownership.
    while let Some(task) = tasks.last_mut() {
        if let Err(error) = (&mut task.monitor).await {
            tracing::error!(task = task.name, "[TASK MONITOR ERROR] {}", error);
        }
        tasks.pop();
    }
}

pub fn task_supervisor() -> &'static TaskSupervisor {
    static SUPERVISOR: OnceLock<TaskSupervisor> = OnceLock::new();
    SUPERVISOR.get_or_init(TaskSupervisor::default)
}

pub async fn drain_tracked_tasks(duration: Duration) -> Result<ShutdownReport, ShutdownError> {
    task_supervisor().shutdown(duration).await
}

pub fn spawn_tracked<T, F>(name: &'static str, future: F) -> Option<TrackedTask<T>>
where
    T: Send + 'static,
    F: Future<Output = T> + Send + 'static,
{
    task_supervisor().spawn(name, future)
}

/// Result collection for wallet/reward fan-out. Dropping the parent cancels
/// only the receivers; the supervisor retains the actual DB-using workers.
pub struct OwnedTaskSet<T> {
    name: &'static str,
    results: tokio::task::JoinSet<Result<T, tokio::task::JoinError>>,
}

impl<T: Send + 'static> OwnedTaskSet<T> {
    pub fn new(name: &'static str) -> Self {
        Self {
            name,
            results: tokio::task::JoinSet::new(),
        }
    }

    pub fn spawn<F>(&mut self, future: F)
    where
        F: Future<Output = T> + Send + 'static,
    {
        if let Some(task) = spawn_tracked(self.name, future) {
            self.results.spawn(task.join());
        }
    }

    pub async fn join_next(&mut self) -> Option<Result<T, tokio::task::JoinError>> {
        self.results
            .join_next()
            .await
            .map(|result| result.and_then(|inner| inner))
    }
}

pub fn spawn_resilient<F>(name: &'static str, future: F)
where
    F: Future<Output = ()> + Send + 'static,
{
    let _ = spawn_tracked(name, future);
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

#[cfg(test)]
mod ownership_gap_regression_tests {
    use super::*;

    #[tokio::test(start_paused = true)]
    async fn interrupted_drain_retains_ownership_for_retry() {
        let owner = TaskSupervisor::default();
        let (release, wait) = tokio::sync::oneshot::channel::<()>();
        let task = owner
            .spawn("audit_owned_waiter", async move {
                let _ = wait.await;
            })
            .unwrap();
        let interrupted = timeout(
            Duration::from_millis(1),
            owner.shutdown(Duration::from_secs(1)),
        )
        .await;
        let after_cancel = owner.active_tasks();
        let retry = timeout(
            Duration::from_millis(1),
            owner.shutdown(Duration::from_secs(1)),
        )
        .await;
        // Release the synthetic in-memory waiter before asserting, even on the red source.
        release.send(()).unwrap();
        task.join().await.unwrap();
        owner.shutdown(Duration::from_secs(1)).await.unwrap();
        assert!(interrupted.is_err());
        assert_eq!(
            after_cancel.len(),
            1,
            "Dropping shutdown detached a still-live owned task"
        );
        assert!(
            retry.is_err(),
            "Retry falsely succeeded while the original worker was alive"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn normal_drain_remains_idempotent_and_rejects_new_work() {
        let owner = TaskSupervisor::default();
        let token = owner.cancellation_token();
        let worker = owner
            .spawn("cooperative_waiter", async move {
                token.cancelled().await;
            })
            .unwrap();
        owner.shutdown(Duration::from_secs(1)).await.unwrap();
        worker.join().await.unwrap();
        assert!(owner.active_tasks().is_empty());
        assert!(owner.spawn("must_not_run", async {}).is_none());
        assert!(
            owner
                .shutdown(Duration::from_secs(1))
                .await
                .unwrap()
                .cancelled
                .is_empty()
        );
    }
}
