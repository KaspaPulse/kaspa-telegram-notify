use kaspa_pulse::domain::entities::TrackedWallet;
use kaspa_pulse::domain::errors::AppError;
use kaspa_pulse::infrastructure::database::postgres_adapter::PostgresRepository;
use kaspa_pulse::infrastructure::telegram_delivery_queue::{
    mark_failed, mark_sent, parse_max_delivery_attempts,
};
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use std::sync::Arc;
use std::time::Duration;

fn unavailable_pool() -> PgPool {
    PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_millis(250))
        .connect_lazy("postgres://audit_unavailable@127.0.0.1:1/audit?sslmode=disable")
        .expect("unavailable test endpoint URL must parse")
}

async fn test_pool() -> PgPool {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL is required for database tests");

    PgPoolOptions::new()
        .max_connections(25)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&database_url)
        .await
        .expect("test PostgreSQL must be reachable")
}

#[test]
fn wallet_repository_does_not_hide_lookup_or_quota_errors() {
    let source = include_str!("../src/infrastructure/database/wallets_repo.rs");

    assert!(!source.contains(".unwrap_or(false)"));
    assert!(!source.contains("count_user_wallets(wallet.chat_id).await.unwrap_or(0)"));

    // Quota enforcement must remain in the same transaction as the write. A separate
    // preflight lookup/count would reintroduce a check-then-insert race under concurrency.
    assert!(source.contains("let mut transaction = self"));
    assert!(source.contains("pg_advisory_xact_lock($1)"));
    assert!(source.contains("let already_exists = sqlx::query_scalar"));
    assert!(source.contains("let current_count = sqlx::query_scalar"));
    assert!(source.contains("fetch_one(&mut *transaction)"));
    assert!(source.contains("execute(&mut *transaction)"));
    assert!(source.contains("transaction\n            .commit()"));
}

#[tokio::test]
async fn concurrent_wallet_additions_cannot_exceed_per_user_limit() {
    let pool = test_pool().await;
    let repository = Arc::new(PostgresRepository::new(pool.clone()));
    let chat_id = -9_024_000_001_i64;
    let limit = kaspa_pulse::utils::max_wallets_per_user();

    assert!(limit > 0);
    assert!(
        limit <= 100,
        "concurrency regression test expects a bounded MAX_WALLETS_PER_USER"
    );

    sqlx::query("DELETE FROM user_wallets WHERE chat_id = $1")
        .bind(chat_id)
        .execute(&pool)
        .await
        .expect("test rows must be reset");

    let mut tasks = Vec::new();
    for index in 0..(limit + 8) {
        let repository = Arc::clone(&repository);
        tasks.push(tokio::spawn(async move {
            repository
                .add_tracked_wallet(TrackedWallet {
                    address: format!("kaspa:concurrency-regression-{index}"),
                    chat_id,
                })
                .await
        }));
    }

    let mut successful_adds = 0_i64;
    for task in tasks {
        if task.await.expect("wallet add task must not panic").is_ok() {
            successful_adds += 1;
        }
    }

    let persisted = repository
        .count_user_wallets(chat_id)
        .await
        .expect("wallet count must remain readable");

    assert_eq!(successful_adds, limit);
    assert_eq!(persisted, limit);

    sqlx::query("DELETE FROM user_wallets WHERE chat_id = $1")
        .bind(chat_id)
        .execute(&pool)
        .await
        .expect("test rows must be cleaned up");
}

#[test]
fn empty_wallet_snapshot_prunes_stale_seen_utxos() {
    let source = include_str!("../src/infrastructure/database/wallets_repo.rs");

    assert!(source.contains("if current_outpoints.is_empty()"));
    assert!(source.contains("DELETE FROM wallet_seen_utxos WHERE wallet = $1"));
    assert!(!source.contains("if current_outpoints.is_empty() {\n            return Ok(())"));
}

#[test]
fn transactional_outbox_failure_is_not_treated_as_permission_to_send() {
    let wallet_source = include_str!("../src/wallet/wallet_use_cases.rs");
    let monitor_source = include_str!("../src/presentation/telegram/workers/utxo_monitor.rs");

    assert!(!wallet_source.contains("try_claim_alert_key("));
    assert!(monitor_source.contains("commit_alert_outbox"));
    assert!(monitor_source.contains("alert_outbox_commit_failed"));
    assert!(monitor_source.contains("retry_next_scan"));
    assert!(!monitor_source.contains("BOT OUT FALLBACK"));
    assert!(!monitor_source.contains("Falling back to direct send"));
}

#[test]
fn delivery_success_metrics_follow_durable_sent_acknowledgement() {
    let source = include_str!("../src/presentation/telegram/workers/telegram_delivery.rs");
    let success = source
        .split("Ok(_) => {")
        .nth(1)
        .expect("delivery success branch must exist");
    let acknowledge = success
        .find("claim.mark_sent().await")
        .expect("durable acknowledgement must exist");
    let metric = success
        .find("metrics::inc_alerts_delivered()")
        .expect("delivery metric must exist");
    let visible_success = success
        .find("QUEUED ALERT DELIVERED")
        .expect("visible success log must exist");

    assert!(
        acknowledge < metric,
        "delivery metric preceded durable acknowledgement"
    );
    assert!(
        acknowledge < visible_success,
        "success log preceded durable acknowledgement"
    );
}

#[test]
fn confirmed_rewards_defer_seen_persistence_to_the_transactional_outbox() {
    let wallet_source = include_str!("../src/wallet/wallet_use_cases.rs");
    let queue_source = include_str!("../src/infrastructure/telegram_delivery_queue.rs");

    assert!(!wallet_source.contains("if reward_is_confirmed || seen_before || is_first_run"));
    assert!(wallet_source.contains("if seen_before || is_first_run"));
    assert!(wallet_source.contains("source_outpoint: utxo.outpoint"));
    assert!(queue_source.contains("INSERT INTO wallet_seen_utxos"));
    assert!(queue_source.contains("execute(&mut *transaction)"));
}

#[test]
fn delivery_queue_decoding_does_not_fall_back_to_default_values() {
    let source = include_str!("../src/infrastructure/telegram_delivery_queue.rs");

    assert!(source.contains("query_as::<_, QueuedTelegramMessage>"));
    assert!(!source.contains("try_get::<i64, _>(\"id\").unwrap_or_default()"));
    assert!(!source.contains("try_get::<String, _>(\"status\").unwrap_or_default()"));
    assert!(source.contains("Unexpected Telegram delivery queue status"));
}

#[test]
fn delivery_max_attempts_has_a_safe_default_and_bounds() {
    assert_eq!(parse_max_delivery_attempts(None), 5);
    assert_eq!(parse_max_delivery_attempts(Some("0")), 5);
    assert_eq!(parse_max_delivery_attempts(Some("7")), 7);
    assert_eq!(parse_max_delivery_attempts(Some("1000")), 100);
    assert_eq!(parse_max_delivery_attempts(Some("not-a-number")), 5);
}

#[tokio::test]
async fn privacy_deletion_fails_closed_when_database_is_unavailable() {
    let pool = unavailable_pool();
    let repo = PostgresRepository::new(pool);

    let full_delete = repo.remove_all_user_data(9_999_001, 9_999_001).await;
    assert!(matches!(full_delete, Err(AppError::DatabaseError(_))));
}

#[tokio::test]
async fn wallet_clear_fails_closed_when_database_is_unavailable() {
    let pool = unavailable_pool();
    let repo = PostgresRepository::new(pool);

    let clear = repo.remove_all_user_wallets(9_999_002).await;
    assert!(matches!(clear, Err(AppError::DatabaseError(_))));
}

#[tokio::test]
async fn mark_sent_returns_not_found_for_a_missing_queue_row() {
    let pool = test_pool().await;
    let missing_id = i64::MAX - 11;

    let result = mark_sent(&pool, missing_id, "missing-claim").await;

    assert!(matches!(result, Err(AppError::NotFound(_))));
}

#[tokio::test]
async fn mark_failed_returns_not_found_for_a_missing_queue_row() {
    let pool = test_pool().await;
    let missing_id = i64::MAX - 12;

    let result = mark_failed(
        &pool,
        missing_id,
        "missing-claim",
        "stage3 batch1 missing row",
    )
    .await;

    assert!(matches!(result, Err(AppError::NotFound(_))));
}
