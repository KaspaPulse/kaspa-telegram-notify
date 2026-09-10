use kaspa_pulse::domain::entities::TrackedWallet;
use kaspa_pulse::domain::errors::AppError;
use kaspa_pulse::infrastructure::database::postgres_adapter::PostgresRepository;
use kaspa_pulse::infrastructure::telegram_delivery_queue::{
    enqueue_message, fetch_pending_batch, mark_failed, mark_sent, pending_count, queue_stats,
};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use std::sync::atomic::{AtomicI64, Ordering};
use std::sync::{Arc, OnceLock};
use std::time::Duration;
use tokio::sync::Mutex;

static NEXT_CHAT_ID: AtomicI64 = AtomicI64::new(9_100_000_000);

fn database_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn next_chat_id() -> i64 {
    NEXT_CHAT_ID.fetch_add(1, Ordering::Relaxed)
}

fn wallet_address(label: &str, chat_id: i64) -> String {
    format!("kaspa:stage3b-{label}-{chat_id}")
}

async fn test_pool() -> PgPool {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL is required for Stage 3B tests");

    PgPoolOptions::new()
        .max_connections(20)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&database_url)
        .await
        .expect("Stage 3B PostgreSQL must be reachable")
}

async fn reset_characterization_tables(pool: &PgPool) {
    sqlx::query(
        "TRUNCATE TABLE
            telegram_delivery_queue,
            wallet_alert_dedup,
            user_wallets",
    )
    .execute(pool)
    .await
    .expect("failed to reset Stage 3B characterization tables");
}

fn unavailable_pool() -> PgPool {
    PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_millis(250))
        .connect_lazy("postgres://stage3b@127.0.0.1:1/stage3b?sslmode=disable")
        .expect("invalid-endpoint pool URL should parse")
}

#[tokio::test]
async fn wallet_add_is_idempotent_for_the_same_chat_and_wallet() {
    let _guard = database_test_lock().lock().await;
    let pool = test_pool().await;
    reset_characterization_tables(&pool).await;

    let chat_id = next_chat_id();
    let address = wallet_address("idempotent", chat_id);
    let repository = PostgresRepository::new(pool.clone());
    let wallet = TrackedWallet {
        address: address.clone(),
        chat_id,
    };

    repository
        .add_tracked_wallet(wallet.clone())
        .await
        .expect("first wallet insert should succeed");
    repository
        .add_tracked_wallet(wallet)
        .await
        .expect("repeated wallet insert should remain idempotent");

    let count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM user_wallets WHERE wallet = $1 AND chat_id = $2",
    )
    .bind(&address)
    .bind(chat_id)
    .fetch_one(&pool)
    .await
    .expect("wallet count query should succeed");

    assert_eq!(count, 1);
}

#[tokio::test]
async fn the_same_wallet_can_be_tracked_by_distinct_chats() {
    let _guard = database_test_lock().lock().await;
    let pool = test_pool().await;
    reset_characterization_tables(&pool).await;

    let first_chat_id = next_chat_id();
    let second_chat_id = next_chat_id();
    let address = wallet_address("shared", first_chat_id);
    let repository = PostgresRepository::new(pool.clone());

    repository
        .add_tracked_wallet(TrackedWallet {
            address: address.clone(),
            chat_id: first_chat_id,
        })
        .await
        .expect("first chat should track the wallet");

    repository
        .add_tracked_wallet(TrackedWallet {
            address: address.clone(),
            chat_id: second_chat_id,
        })
        .await
        .expect("second chat should independently track the wallet");

    let count = sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM user_wallets WHERE wallet = $1")
        .bind(&address)
        .fetch_one(&pool)
        .await
        .expect("wallet subscriber count should succeed");

    assert_eq!(count, 2);
}

#[tokio::test]
async fn wallet_remove_deletes_only_the_exact_chat_wallet_pair() {
    let _guard = database_test_lock().lock().await;
    let pool = test_pool().await;
    reset_characterization_tables(&pool).await;

    let first_chat_id = next_chat_id();
    let second_chat_id = next_chat_id();
    let shared_address = wallet_address("remove-shared", first_chat_id);
    let other_address = wallet_address("remove-other", first_chat_id);
    let repository = PostgresRepository::new(pool.clone());

    for wallet in [
        TrackedWallet {
            address: shared_address.clone(),
            chat_id: first_chat_id,
        },
        TrackedWallet {
            address: shared_address.clone(),
            chat_id: second_chat_id,
        },
        TrackedWallet {
            address: other_address.clone(),
            chat_id: first_chat_id,
        },
    ] {
        repository
            .add_tracked_wallet(wallet)
            .await
            .expect("test wallet insert should succeed");
    }

    repository
        .remove_tracked_wallet(&shared_address, first_chat_id)
        .await
        .expect("exact wallet removal should succeed");

    assert!(
        !repository
            .user_wallet_exists(&shared_address, first_chat_id)
            .await
            .expect("removed pair lookup should succeed")
    );
    assert!(
        repository
            .user_wallet_exists(&shared_address, second_chat_id)
            .await
            .expect("other chat lookup should succeed")
    );
    assert!(
        repository
            .user_wallet_exists(&other_address, first_chat_id)
            .await
            .expect("other wallet lookup should succeed")
    );

    repository
        .remove_tracked_wallet(&shared_address, first_chat_id)
        .await
        .expect("removing an already absent pair should remain idempotent");
}

#[tokio::test]
async fn concurrent_alert_claims_allow_exactly_one_winner() {
    let _guard = database_test_lock().lock().await;
    let pool = test_pool().await;
    reset_characterization_tables(&pool).await;

    let repository = Arc::new(PostgresRepository::new(pool.clone()));
    let wallet = wallet_address("dedup", next_chat_id());
    let alert_key = "stage3b-shared-alert-key".to_string();

    let mut tasks = Vec::new();
    for attempt in 0..16 {
        let repository = Arc::clone(&repository);
        let wallet = wallet.clone();
        let alert_key = alert_key.clone();

        tasks.push(tokio::spawn(async move {
            let txid = format!("tx-{attempt}");

            repository
                .try_claim_alert_key(&wallet, &alert_key, Some(&txid), Some("block-stage3b"))
                .await
        }));
    }

    let mut winners = 0;
    for task in tasks {
        if task
            .await
            .expect("dedup task should not panic")
            .expect("dedup claim should not fail")
        {
            winners += 1;
        }
    }

    assert_eq!(winners, 1);

    let stored =
        sqlx::query_scalar::<_, i64>("SELECT COUNT(*) FROM wallet_alert_dedup WHERE wallet = $1")
            .bind(&wallet)
            .fetch_one(&pool)
            .await
            .expect("dedup count query should succeed");

    assert_eq!(stored, 1);
}

#[tokio::test]
async fn a_fresh_processing_message_is_not_claimed_twice() {
    let _guard = database_test_lock().lock().await;
    let pool = test_pool().await;
    reset_characterization_tables(&pool).await;

    let chat_id = next_chat_id();
    enqueue_message(&pool, chat_id, "<b>stage3b fresh lock</b>")
        .await
        .expect("queue insert should succeed");

    let first_batch = fetch_pending_batch(&pool, 10)
        .await
        .expect("first queue claim should succeed");
    assert_eq!(first_batch.len(), 1);

    let second_batch = fetch_pending_batch(&pool, 10)
        .await
        .expect("second queue claim should succeed");
    assert!(second_batch.is_empty());
}

#[tokio::test]
async fn a_stale_processing_message_is_reclaimed_after_the_worker_crash_window() {
    let _guard = database_test_lock().lock().await;
    let pool = test_pool().await;
    reset_characterization_tables(&pool).await;

    let chat_id = next_chat_id();
    enqueue_message(&pool, chat_id, "<b>stage3b stale lock</b>")
        .await
        .expect("queue insert should succeed");

    let first_batch = fetch_pending_batch(&pool, 10)
        .await
        .expect("first queue claim should succeed");
    assert_eq!(first_batch.len(), 1);
    let message_id = first_batch[0].id;

    sqlx::query(
        "UPDATE telegram_delivery_queue
         SET locked_at = NOW() - INTERVAL '121 seconds'
         WHERE id = $1",
    )
    .bind(message_id)
    .execute(&pool)
    .await
    .expect("stale lock simulation should succeed");

    let reclaimed = fetch_pending_batch(&pool, 10)
        .await
        .expect("stale queue reclaim should succeed");

    assert_eq!(reclaimed.len(), 1);
    assert_eq!(reclaimed[0].id, message_id);
}

#[tokio::test]
async fn mark_sent_finishes_the_processing_row_and_clears_its_lock() {
    let _guard = database_test_lock().lock().await;
    let pool = test_pool().await;
    reset_characterization_tables(&pool).await;

    let chat_id = next_chat_id();
    enqueue_message(&pool, chat_id, "<b>stage3b sent</b>")
        .await
        .expect("queue insert should succeed");

    let batch = fetch_pending_batch(&pool, 10)
        .await
        .expect("queue claim should succeed");
    let message_id = batch[0].id;

    mark_sent(&pool, message_id)
        .await
        .expect("mark_sent should succeed");

    let row = sqlx::query(
        "SELECT status, attempts, locked_at IS NULL AS lock_time_cleared,
                locked_by IS NULL AS worker_cleared
         FROM telegram_delivery_queue
         WHERE id = $1",
    )
    .bind(message_id)
    .fetch_one(&pool)
    .await
    .expect("sent row query should succeed");

    assert_eq!(
        row.try_get::<String, _>("status")
            .expect("status should decode"),
        "sent"
    );
    assert_eq!(
        row.try_get::<i32, _>("attempts")
            .expect("attempts should decode"),
        1
    );
    assert!(
        row.try_get::<bool, _>("lock_time_cleared")
            .expect("lock state should decode")
    );
    assert!(
        row.try_get::<bool, _>("worker_cleared")
            .expect("worker state should decode")
    );
}

#[tokio::test]
async fn repeated_delivery_failures_back_off_then_become_terminal() {
    let _guard = database_test_lock().lock().await;
    let pool = test_pool().await;
    reset_characterization_tables(&pool).await;

    let chat_id = next_chat_id();
    enqueue_message(&pool, chat_id, "<b>stage3b retry</b>")
        .await
        .expect("queue insert should succeed");

    let mut batch = fetch_pending_batch(&pool, 10)
        .await
        .expect("initial queue claim should succeed");
    let message_id = batch.remove(0).id;

    for attempt in 1..=5 {
        mark_failed(
            &pool,
            message_id,
            "Too Many Requests: retry_after 42 for kaspa:stage3b-sensitive-wallet-000001",
        )
        .await
        .expect("mark_failed should succeed");

        let row = sqlx::query(
            "SELECT status, attempts, last_error,
                    EXTRACT(EPOCH FROM (next_attempt_at - NOW()))::BIGINT AS delay_seconds
             FROM telegram_delivery_queue
             WHERE id = $1",
        )
        .bind(message_id)
        .fetch_one(&pool)
        .await
        .expect("failed row query should succeed");

        let status = row
            .try_get::<String, _>("status")
            .expect("status should decode");
        let attempts = row
            .try_get::<i32, _>("attempts")
            .expect("attempts should decode");
        let stored_error = row
            .try_get::<Option<String>, _>("last_error")
            .expect("last_error should decode")
            .unwrap_or_default();

        assert_eq!(attempts, attempt);
        assert!(!stored_error.contains("kaspa:stage3b-sensitive-wallet-000001"));

        if attempt == 1 {
            let delay_seconds = row
                .try_get::<i64, _>("delay_seconds")
                .expect("delay should decode");
            assert!((35..=45).contains(&delay_seconds));
        }

        if attempt < 5 {
            assert_eq!(status, "pending");

            sqlx::query(
                "UPDATE telegram_delivery_queue
                 SET next_attempt_at = NOW()
                 WHERE id = $1",
            )
            .bind(message_id)
            .execute(&pool)
            .await
            .expect("retry clock reset should succeed");

            let reclaimed = fetch_pending_batch(&pool, 10)
                .await
                .expect("retry claim should succeed");
            assert_eq!(reclaimed.len(), 1);
            assert_eq!(reclaimed[0].id, message_id);
        } else {
            assert_eq!(status, "failed");
        }
    }

    assert_eq!(
        pending_count(&pool)
            .await
            .expect("pending count should succeed"),
        0
    );
}

#[tokio::test]
async fn queue_stats_separate_historical_and_recent_terminal_failures() {
    let _guard = database_test_lock().lock().await;
    let pool = test_pool().await;
    reset_characterization_tables(&pool).await;

    sqlx::query(
        "INSERT INTO telegram_delivery_queue
            (chat_id, message_html, status, attempts, created_at, updated_at, next_attempt_at)
         VALUES
            ($1, '<b>historical failure</b>', 'failed', 5, NOW() - INTERVAL '2 days', NOW() - INTERVAL '2 days', NOW()),
            ($2, '<b>recent failure</b>', 'failed', 5, NOW(), NOW(), NOW())",
    )
    .bind(next_chat_id())
    .bind(next_chat_id())
    .execute(&pool)
    .await
    .expect("failed-row fixtures should insert");

    let stats = queue_stats(&pool, 3_600)
        .await
        .expect("queue stats should succeed");

    assert_eq!(stats.failed, 2);
    assert_eq!(stats.failed_recent, 1);
}

#[tokio::test]
async fn repository_operations_return_database_error_during_an_outage() {
    let pool = unavailable_pool();
    let repository = PostgresRepository::new(pool);

    let result = repository.count_user_wallets(next_chat_id()).await;

    assert!(matches!(result, Err(AppError::DatabaseError(_))));
}

#[tokio::test]
async fn enqueue_returns_database_error_when_the_queue_database_is_unavailable() {
    let pool = unavailable_pool();

    let result = enqueue_message(&pool, next_chat_id(), "stage3b outage").await;

    assert!(matches!(result, Err(AppError::DatabaseError(_))));
}
