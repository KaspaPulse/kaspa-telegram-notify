// [INJECTED BY PHASE 4 SECURITY SCRIPT]
// Enterprise standard utilities for self-healing connections and graceful shutdowns.

use tokio::time::{sleep, Duration};
use tokio::signal;

/// Wraps any async network call with an Exponential Backoff retry mechanism.
/// Perfect for wRPC Kaspa Node connections that might drop momentarily.
pub async fn with_retries<F, Fut, T, E>(mut action: F, max_retries: u32) -> Result<T, E>
where
    F: FnMut() -> Fut,
    Fut: std::future::Future<Output = Result<T, E>>,
    E: std::fmt::Debug,
{
    let mut retries = 0;
    let mut delay = Duration::from_secs(2);

    loop {
        match action().await {
            Ok(result) => return Ok(result),
            Err(e) => {
                if retries >= max_retries {
                    tracing::error!("💥 CRITICAL: Action failed after {} retries. Last error: {:?}", max_retries, e);
                    return Err(e);
                }
                tracing::warn!("⚠️ Connection issue detected: {:?}. Retrying in {:?}...", e, delay);
                sleep(delay).await;
                retries += 1;
                delay *= 2; // Exponential backoff (2s, 4s, 8s, 16s...)
            }
        }
    }
}

/// Listens for OS shutdown signals (Ctrl+C, SIGTERM) to safely close DB connections.
pub async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c().await.expect("Failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("Failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => tracing::warn!("🛑 [SHUTDOWN] Ctrl-C received. Flushing state and exiting safely..."),
        _ = terminate => tracing::warn!("🛑 [SHUTDOWN] SIGTERM received. Flushing state and exiting safely..."),
    }
}
