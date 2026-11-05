pub mod events_repo;
pub mod kas_price_repo;
pub mod mined_blocks_repo;
pub mod pending_rewards_repo;
pub mod postgres_adapter;
pub mod settings_repo;
pub mod wallets_repo;

use sqlx::postgres::PgPoolOptions;
use tokio_util::sync::CancellationToken;

/// Pool policy for runtime connections. During normal operation connections are
/// recycled normally; after global shutdown begins, released connections are
/// closed instead of being pinged/recycled.
pub fn runtime_pool_options(max_connections: u32, shutdown: CancellationToken) -> PgPoolOptions {
    PgPoolOptions::new()
        .max_connections(max_connections)
        .after_connect(|connection, _metadata| {
            Box::pin(async move {
                // PostgreSQL normally does not poll the client socket while a query is
                // blocked waiting for a lock. Detect a disconnected runtime promptly so
                // an aborted task cannot leave its server backend waiting indefinitely.
                sqlx::query("SET client_connection_check_interval = '1s'")
                    .execute(connection)
                    .await?;
                Ok(())
            })
        })
        .after_release(move |_connection, _metadata| {
            let shutdown = shutdown.clone();
            Box::pin(async move { Ok(!shutdown.is_cancelled()) })
        })
}
