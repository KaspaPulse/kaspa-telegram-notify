use kaspa_pulse::domain::errors::AppError;
use kaspa_pulse::infrastructure::telegram_delivery_queue::{
    AlertOutboxOutcome, AlertOutboxRequest, acquire_delivery_claim_guard, commit_alert_outbox,
    delivery_claim_is_current, enqueue_message, fetch_pending_batch,
};
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Row};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicI64, Ordering};
use std::time::Duration;
use tokio::sync::{Mutex, MutexGuard};

static NEXT_ID: AtomicI64 = AtomicI64::new(9_300_000_000);

fn database_test_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

fn next_id() -> i64 {
    NEXT_ID.fetch_add(1, Ordering::Relaxed)
}

async fn test_pool() -> PgPool {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL is required for outbox tests");

    PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&database_url)
        .await
        .expect("runtime test PostgreSQL must be reachable")
}

async fn admin_pool() -> PgPool {
    let database_admin_url = std::env::var("DATABASE_ADMIN_URL")
        .expect("DATABASE_ADMIN_URL is required for outbox test setup");

    PgPoolOptions::new()
        .max_connections(2)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&database_admin_url)
        .await
        .expect("administrative test PostgreSQL must be reachable")
}

async fn reset_tables(admin_pool: &PgPool) {
    sqlx::query(
        "DROP TRIGGER IF EXISTS stage3b2_reject_queue_insert_trigger
         ON telegram_delivery_queue",
    )
    .execute(admin_pool)
    .await
    .expect("failed to remove a stale queue failure trigger");

    sqlx::query("DROP FUNCTION IF EXISTS stage3b2_reject_queue_insert()")
        .execute(admin_pool)
        .await
        .expect("failed to remove a stale queue failure function");

    sqlx::query("TRUNCATE TABLE telegram_delivery_queue, wallet_alert_dedup, wallet_seen_utxos, user_wallets")
        .execute(admin_pool)
        .await
        .expect("failed to reset outbox tables");

    sqlx::query(
        "INSERT INTO system_settings (key_name, value_data)
         VALUES ('ENABLE_ALERT_DELIVERY', 'true')
         ON CONFLICT (key_name)
         DO UPDATE SET value_data = EXCLUDED.value_data, updated_at = NOW()",
    )
    .execute(admin_pool)
    .await
    .expect("failed to enable alert delivery");
}

async fn subscribe_fixture(pool: &PgPool, wallet: &str, chat_ids: &[i64]) {
    for chat_id in chat_ids {
        sqlx::query(
            "INSERT INTO user_wallets (wallet, chat_id) VALUES ($1, $2) ON CONFLICT DO NOTHING",
        )
        .bind(wallet)
        .bind(chat_id)
        .execute(pool)
        .await
        .unwrap();
    }
}

fn request<'a>(
    wallet: &'a str,
    outpoint: &'a str,
    alert_key: &'a str,
    message: &'a str,
    chat_ids: &'a [i64],
) -> AlertOutboxRequest<'a> {
    AlertOutboxRequest {
        wallet,
        source_outpoint: outpoint,
        alert_key,
        message_html: message,
        chat_ids,
        txid_masked: Some("stage3b2...txid"),
        block_hash_masked: Some("stage3b2...block"),
        amount_kas: Some(2.59565436),
        daa_score: Some(474_800_104),
    }
}

#[tokio::test]
async fn commits_dedup_seen_and_recipient_queue_rows_atomically() {
    let _guard: MutexGuard<'static, ()> = database_test_lock().lock().await;
    let pool = test_pool().await;
    let privileged_pool = admin_pool().await;
    reset_tables(&privileged_pool).await;

    let id = next_id();
    let wallet = format!("kaspa:stage3b2-wallet-{id}");
    let outpoint = format!("stage3b2-outpoint-{id}:0");
    let alert_key = format!("stage3b2-alert-{id}");
    let chat_ids = [id, id + 1, id];
    subscribe_fixture(&pool, &wallet, &chat_ids).await;

    let outcome = commit_alert_outbox(
        &pool,
        request(&wallet, &outpoint, &alert_key, "<b>stage3b2</b>", &chat_ids),
    )
    .await
    .expect("transactional outbox commit should succeed");

    assert_eq!(outcome, AlertOutboxOutcome::Enqueued { recipients: 2 });

    let dedup_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM wallet_alert_dedup WHERE wallet = $1 AND alert_key = $2",
    )
    .bind(&wallet)
    .bind(&alert_key)
    .fetch_one(&pool)
    .await
    .expect("dedup count should succeed");

    let seen_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM wallet_seen_utxos WHERE wallet = $1 AND outpoint = $2",
    )
    .bind(&wallet)
    .bind(&outpoint)
    .fetch_one(&pool)
    .await
    .expect("seen count should succeed");

    let queue_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM telegram_delivery_queue WHERE event_key = $1",
    )
    .bind(&outpoint)
    .fetch_one(&pool)
    .await
    .expect("queue count should succeed");

    assert_eq!(dedup_count, 1);
    assert_eq!(seen_count, 1);
    assert_eq!(queue_count, 2);
}

#[tokio::test]
async fn repeated_commit_does_not_duplicate_queue_rows() {
    let _guard: MutexGuard<'static, ()> = database_test_lock().lock().await;
    let pool = test_pool().await;
    let privileged_pool = admin_pool().await;
    reset_tables(&privileged_pool).await;

    let id = next_id();
    let wallet = format!("kaspa:stage3b2-wallet-{id}");
    let outpoint = format!("stage3b2-outpoint-{id}:0");
    let alert_key = format!("stage3b2-alert-{id}");
    let chat_ids = [id, id + 1];
    subscribe_fixture(&pool, &wallet, &chat_ids).await;

    let first = commit_alert_outbox(
        &pool,
        request(&wallet, &outpoint, &alert_key, "<b>first</b>", &chat_ids),
    )
    .await
    .expect("first commit should succeed");
    assert_eq!(first, AlertOutboxOutcome::Enqueued { recipients: 2 });

    let second = commit_alert_outbox(
        &pool,
        request(&wallet, &outpoint, &alert_key, "<b>second</b>", &chat_ids),
    )
    .await
    .expect("repeated commit should be idempotent");
    assert_eq!(second, AlertOutboxOutcome::Duplicate);

    let queue_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM telegram_delivery_queue WHERE event_key = $1",
    )
    .bind(&outpoint)
    .fetch_one(&pool)
    .await
    .expect("queue count should succeed");

    assert_eq!(queue_count, 2);
}

#[tokio::test]
async fn queue_insert_failure_rolls_back_dedup_and_seen() {
    let _guard: MutexGuard<'static, ()> = database_test_lock().lock().await;
    let pool = test_pool().await;
    let privileged_pool = admin_pool().await;
    reset_tables(&privileged_pool).await;

    sqlx::query(
        "CREATE OR REPLACE FUNCTION stage3b2_reject_queue_insert()
         RETURNS trigger
         LANGUAGE plpgsql
         AS $$
         BEGIN
             RAISE EXCEPTION 'stage3b2 forced queue failure';
         END;
         $$",
    )
    .execute(&privileged_pool)
    .await
    .expect("failed to create queue failure function");

    sqlx::query(
        "CREATE TRIGGER stage3b2_reject_queue_insert_trigger
         BEFORE INSERT ON telegram_delivery_queue
         FOR EACH ROW
         EXECUTE FUNCTION stage3b2_reject_queue_insert()",
    )
    .execute(&privileged_pool)
    .await
    .expect("failed to create queue failure trigger");

    let id = next_id();
    let wallet = format!("kaspa:stage3b2-wallet-{id}");
    let outpoint = format!("stage3b2-outpoint-{id}:0");
    let alert_key = format!("stage3b2-alert-{id}");
    let chat_ids = [id];
    subscribe_fixture(&pool, &wallet, &chat_ids).await;

    let result = commit_alert_outbox(
        &pool,
        request(
            &wallet,
            &outpoint,
            &alert_key,
            "<b>forced failure</b>",
            &chat_ids,
        ),
    )
    .await;

    sqlx::query(
        "DROP TRIGGER IF EXISTS stage3b2_reject_queue_insert_trigger
         ON telegram_delivery_queue",
    )
    .execute(&privileged_pool)
    .await
    .expect("failed to drop queue failure trigger");

    sqlx::query("DROP FUNCTION IF EXISTS stage3b2_reject_queue_insert()")
        .execute(&privileged_pool)
        .await
        .expect("failed to drop queue failure function");

    assert!(matches!(result, Err(AppError::DatabaseError(_))));

    let dedup_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM wallet_alert_dedup WHERE wallet = $1 AND alert_key = $2",
    )
    .bind(&wallet)
    .bind(&alert_key)
    .fetch_one(&pool)
    .await
    .expect("dedup count should succeed");

    let seen_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM wallet_seen_utxos WHERE wallet = $1 AND outpoint = $2",
    )
    .bind(&wallet)
    .bind(&outpoint)
    .fetch_one(&pool)
    .await
    .expect("seen count should succeed");

    assert_eq!(dedup_count, 0);
    assert_eq!(seen_count, 0);
}

#[tokio::test]
async fn existing_dedup_without_queue_is_reconciled() {
    let _guard: MutexGuard<'static, ()> = database_test_lock().lock().await;
    let pool = test_pool().await;
    let privileged_pool = admin_pool().await;
    reset_tables(&privileged_pool).await;

    let id = next_id();
    let wallet = format!("kaspa:stage3b2-wallet-{id}");
    let outpoint = format!("stage3b2-outpoint-{id}:0");
    let alert_key = format!("stage3b2-alert-{id}");
    let chat_ids = [id, id + 1];
    subscribe_fixture(&pool, &wallet, &chat_ids).await;

    sqlx::query("INSERT INTO wallet_alert_dedup (wallet, alert_key) VALUES ($1, $2)")
        .bind(&wallet)
        .bind(&alert_key)
        .execute(&privileged_pool)
        .await
        .expect("dedup fixture should insert");

    let outcome = commit_alert_outbox(
        &pool,
        request(
            &wallet,
            &outpoint,
            &alert_key,
            "<b>reconcile</b>",
            &chat_ids,
        ),
    )
    .await
    .expect("reconciliation should succeed");

    assert_eq!(outcome, AlertOutboxOutcome::Reconciled { recipients: 2 });

    let row = sqlx::query(
        "SELECT COUNT(*)::BIGINT AS queue_count,
                COUNT(DISTINCT chat_id)::BIGINT AS recipient_count
         FROM telegram_delivery_queue
         WHERE event_key = $1",
    )
    .bind(&outpoint)
    .fetch_one(&pool)
    .await
    .expect("reconciled queue query should succeed");

    assert_eq!(row.try_get::<i64, _>("queue_count").unwrap(), 2);
    assert_eq!(row.try_get::<i64, _>("recipient_count").unwrap(), 2);
}

#[tokio::test]
async fn disabled_delivery_commits_dedup_and_seen_without_queue_rows() {
    let _guard: MutexGuard<'static, ()> = database_test_lock().lock().await;
    let pool = test_pool().await;
    let privileged_pool = admin_pool().await;
    reset_tables(&privileged_pool).await;

    sqlx::query(
        "UPDATE system_settings
         SET value_data = 'false', updated_at = NOW()
         WHERE key_name = 'ENABLE_ALERT_DELIVERY'",
    )
    .execute(&privileged_pool)
    .await
    .expect("failed to disable alert delivery");

    let id = next_id();
    let wallet = format!("kaspa:stage3b2-wallet-{id}");
    let outpoint = format!("stage3b2-outpoint-{id}:0");
    let alert_key = format!("stage3b2-alert-{id}");
    let chat_ids = [id, id + 1];
    subscribe_fixture(&pool, &wallet, &chat_ids).await;

    let outcome = commit_alert_outbox(
        &pool,
        request(
            &wallet,
            &outpoint,
            &alert_key,
            "<b>suppressed</b>",
            &chat_ids,
        ),
    )
    .await
    .expect("suppressed outbox commit should succeed");

    assert_eq!(outcome, AlertOutboxOutcome::Suppressed { recipients: 2 });

    let queue_count = sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM telegram_delivery_queue WHERE event_key = $1",
    )
    .bind(&outpoint)
    .fetch_one(&pool)
    .await
    .expect("queue count should succeed");

    assert_eq!(queue_count, 0);
}

#[tokio::test]
async fn alert_delivery_setting_lookup_fails_closed_during_database_outage() {
    let pool = PgPoolOptions::new()
        .max_connections(1)
        .acquire_timeout(Duration::from_millis(250))
        .connect_lazy("postgres://stage3b2@127.0.0.1:1/stage3b2?sslmode=disable")
        .expect("invalid-endpoint pool URL should parse");

    let result = kaspa_pulse::wallet::alert_delivery_gate::is_alert_delivery_enabled(&pool).await;

    assert!(matches!(result, Err(AppError::DatabaseError(_))));
}

#[test]
fn monitor_no_longer_falls_back_to_direct_send_after_queue_failure() {
    let source = include_str!("../src/presentation/telegram/workers/utxo_monitor.rs");

    assert!(source.contains("commit_alert_outbox"));
    assert!(source.contains("retry_next_scan"));
    assert!(!source.contains("BOT OUT FALLBACK"));
    assert!(!source.contains("Falling back to direct send"));
}

#[test]
fn wallet_processing_defers_dedup_and_seen_to_the_transactional_outbox() {
    let source = include_str!("../src/wallet/wallet_use_cases.rs");

    assert!(!source.contains("try_claim_alert_key("));
    assert!(source.contains("source_outpoint: utxo.outpoint"));
    assert!(source.contains("alert_key,"));
    assert!(!source.contains("processed_reward_seen_upsert_failed"));
}

#[tokio::test]
async fn fetched_delivery_is_revoked_when_user_is_deleted_before_send() {
    use kaspa_pulse::domain::entities::TrackedWallet;
    use kaspa_pulse::infrastructure::database::postgres_adapter::PostgresRepository;
    let _guard = database_test_lock().lock().await;
    let pool = test_pool().await;
    let admin = admin_pool().await;
    reset_tables(&admin).await;
    let repo = PostgresRepository::new(pool.clone());
    let chat_id = next_id();
    let wallet = format!("kaspa:pre-send-delete-{chat_id}");
    repo.add_tracked_wallet(TrackedWallet {
        address: wallet.clone(),
        chat_id,
    })
    .await
    .unwrap();
    let recipients = [chat_id];
    assert_eq!(
        commit_alert_outbox(
            &pool,
            request(
                &wallet,
                "pre-send-delete:0",
                "pre-send-delete",
                "synthetic queued alert",
                &recipients,
            ),
        )
        .await
        .unwrap(),
        AlertOutboxOutcome::Enqueued { recipients: 1 }
    );
    let mut claimed = fetch_pending_batch(&pool, 1).await.unwrap();
    assert_eq!(claimed.len(), 1);
    let item = claimed.remove(0);
    assert!(
        delivery_claim_is_current(&pool, item.id, &item.claim_token)
            .await
            .unwrap(),
        "freshly fetched claim must be deliverable"
    );

    repo.remove_all_user_data(chat_id, chat_id as u64)
        .await
        .unwrap();

    assert!(
        acquire_delivery_claim_guard(&pool, item.id, &item.claim_token)
            .await
            .unwrap()
            .is_none(),
        "an in-memory item must be revoked after privacy deletion removes its durable queue row"
    );
    let residual: i64 =
        sqlx::query_scalar("SELECT COUNT(*) FROM telegram_delivery_queue WHERE chat_id = $1")
            .bind(chat_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(residual, 0);
}

#[tokio::test]
async fn claimed_alert_is_suppressed_after_single_wallet_unsubscribe() {
    use kaspa_pulse::domain::entities::TrackedWallet;
    use kaspa_pulse::infrastructure::database::postgres_adapter::PostgresRepository;
    let _guard = database_test_lock().lock().await;
    let pool = test_pool().await;
    let admin = admin_pool().await;
    reset_tables(&admin).await;
    let repo = PostgresRepository::new(pool.clone());
    let chat_id = next_id();
    let wallet = format!("kaspa:pre-send-unsubscribe-{chat_id}");
    repo.add_tracked_wallet(TrackedWallet {
        address: wallet.clone(),
        chat_id,
    })
    .await
    .unwrap();
    let outpoint = format!("pre-send-unsubscribe-{chat_id}:0");
    commit_alert_outbox(
        &pool,
        request(
            &wallet,
            &outpoint,
            "pre-send-unsubscribe",
            "synthetic",
            &[chat_id],
        ),
    )
    .await
    .unwrap();
    let item = fetch_pending_batch(&pool, 1).await.unwrap().remove(0);

    repo.remove_tracked_wallet(&wallet, chat_id).await.unwrap();
    assert!(
        acquire_delivery_claim_guard(&pool, item.id, &item.claim_token)
            .await
            .unwrap()
            .is_none()
    );

    let state: (String, Option<String>) =
        sqlx::query_as("SELECT status, last_error FROM telegram_delivery_queue WHERE id = $1")
            .bind(item.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(state.0, "suppressed");
    assert!(state.1.unwrap_or_default().contains("subscription"));
}

#[tokio::test]
async fn resubscribe_does_not_revive_old_claim_but_new_event_is_deliverable() {
    use kaspa_pulse::domain::entities::TrackedWallet;
    use kaspa_pulse::infrastructure::database::postgres_adapter::PostgresRepository;
    let _guard = database_test_lock().lock().await;
    let pool = test_pool().await;
    let admin = admin_pool().await;
    reset_tables(&admin).await;
    let repo = PostgresRepository::new(pool.clone());
    let chat_id = next_id();
    let wallet = format!("kaspa:resubscribe-claim-{chat_id}");
    let tracked = TrackedWallet {
        address: wallet.clone(),
        chat_id,
    };
    repo.add_tracked_wallet(tracked.clone()).await.unwrap();
    commit_alert_outbox(
        &pool,
        request(
            &wallet,
            "resubscribe-old:0",
            "resubscribe-old",
            "old",
            &[chat_id],
        ),
    )
    .await
    .unwrap();
    let old_item = fetch_pending_batch(&pool, 1).await.unwrap().remove(0);
    repo.remove_tracked_wallet(&wallet, chat_id).await.unwrap();
    tokio::time::sleep(Duration::from_millis(2)).await;
    repo.add_tracked_wallet(tracked).await.unwrap();

    assert!(
        acquire_delivery_claim_guard(&pool, old_item.id, &old_item.claim_token)
            .await
            .unwrap()
            .is_none(),
        "a new subscription incarnation must not authorize an older queued claim"
    );

    commit_alert_outbox(
        &pool,
        request(
            &wallet,
            "resubscribe-new:0",
            "resubscribe-new",
            "new",
            &[chat_id],
        ),
    )
    .await
    .unwrap();
    let new_item = fetch_pending_batch(&pool, 1).await.unwrap().remove(0);
    let guard = acquire_delivery_claim_guard(&pool, new_item.id, &new_item.claim_token)
        .await
        .unwrap()
        .expect("new subscription event must be deliverable");
    guard.mark_sent().await.unwrap();
    let state: String =
        sqlx::query_scalar("SELECT status FROM telegram_delivery_queue WHERE id=$1")
            .bind(new_item.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(state, "sent");
}

#[tokio::test]
async fn pre_send_guard_waits_for_inflight_unsubscribe_and_then_revokes_claim() {
    use kaspa_pulse::domain::entities::TrackedWallet;
    use kaspa_pulse::infrastructure::database::postgres_adapter::PostgresRepository;
    let _guard = database_test_lock().lock().await;
    let pool = test_pool().await;
    let admin = admin_pool().await;
    reset_tables(&admin).await;
    let repo = PostgresRepository::new(pool.clone());
    let chat_id = next_id();
    let wallet = format!("kaspa:guard-overlap-{chat_id}");
    repo.add_tracked_wallet(TrackedWallet {
        address: wallet.clone(),
        chat_id,
    })
    .await
    .unwrap();
    commit_alert_outbox(
        &pool,
        request(
            &wallet,
            "guard-overlap:0",
            "guard-overlap",
            "synthetic",
            &[chat_id],
        ),
    )
    .await
    .unwrap();
    let item = fetch_pending_batch(&pool, 1).await.unwrap().remove(0);

    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE OR REPLACE FUNCTION audit_pause_unsubscribe() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN PERFORM pg_advisory_xact_lock(1263552333, 911); RETURN OLD; END $$;
         CREATE TRIGGER audit_pause_unsubscribe AFTER DELETE ON user_wallets
         FOR EACH ROW WHEN (OLD.chat_id = {chat_id}) EXECUTE FUNCTION audit_pause_unsubscribe()"
    )))
    .execute(&admin)
    .await
    .unwrap();

    let mut gate = admin.begin().await.unwrap();
    let gate_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(1263552333, 911)")
        .execute(&mut *gate)
        .await
        .unwrap();

    let url = std::env::var("DATABASE_URL").unwrap();
    let unsubscribe_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    let guard_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    let unsubscribe_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&unsubscribe_pool)
        .await
        .unwrap();
    let guard_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&guard_pool)
        .await
        .unwrap();
    let unsubscribe_repo = PostgresRepository::new(unsubscribe_pool.clone());
    let wallet_for_remove = wallet.clone();
    let removing = tokio::spawn(async move {
        unsubscribe_repo
            .remove_tracked_wallet(&wallet_for_remove, chat_id)
            .await
    });
    assert!(
        backend_waits_for(&admin, unsubscribe_pid, gate_pid).await,
        "unsubscribe did not reach controlled pause"
    );

    let guard_claim = item.claim_token.clone();
    let item_id = item.id;
    let acquiring = tokio::spawn(async move {
        acquire_delivery_claim_guard(&guard_pool, item_id, &guard_claim)
            .await
            .map(|claim| claim.is_none())
    });
    assert!(
        backend_waits_for(&admin, guard_pid, unsubscribe_pid).await,
        "pre-send guard did not serialize behind unsubscribe"
    );

    gate.rollback().await.unwrap();
    assert!(
        tokio::time::timeout(Duration::from_secs(3), removing)
            .await
            .unwrap()
            .unwrap()
            .is_ok()
    );
    let revoked = tokio::time::timeout(Duration::from_secs(3), acquiring)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert!(
        revoked,
        "claim must be revoked after unsubscribe commits first"
    );

    sqlx::raw_sql("DROP TRIGGER audit_pause_unsubscribe ON user_wallets; DROP FUNCTION audit_pause_unsubscribe()")
        .execute(&admin).await.unwrap();
    unsubscribe_pool.close().await;
}

#[tokio::test]
async fn unverifiable_legacy_queue_row_is_suppressed_before_send() {
    let _guard = database_test_lock().lock().await;
    let pool = test_pool().await;
    let admin = admin_pool().await;
    reset_tables(&admin).await;
    let chat_id = next_id();
    enqueue_message(&pool, chat_id, "legacy synthetic queue row")
        .await
        .unwrap();
    let item = fetch_pending_batch(&pool, 1).await.unwrap().remove(0);

    assert!(
        acquire_delivery_claim_guard(&pool, item.id, &item.claim_token)
            .await
            .unwrap()
            .is_none()
    );
    let state: (String, Option<String>) =
        sqlx::query_as("SELECT status, last_error FROM telegram_delivery_queue WHERE id=$1")
            .bind(item.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(state.0, "suppressed");
    assert!(
        state
            .1
            .unwrap_or_default()
            .contains("verifiable wallet identity")
    );
}

#[tokio::test]
async fn privacy_delete_waits_for_authorized_send_boundary_then_removes_state() {
    use kaspa_pulse::domain::entities::TrackedWallet;
    use kaspa_pulse::infrastructure::database::postgres_adapter::PostgresRepository;
    let _guard = database_test_lock().lock().await;
    let pool = test_pool().await;
    let admin = admin_pool().await;
    reset_tables(&admin).await;
    let chat_id = next_id();
    let wallet = format!("kaspa:guard-first-{chat_id}");
    let repo = PostgresRepository::new(pool.clone());
    repo.add_tracked_wallet(TrackedWallet {
        address: wallet.clone(),
        chat_id,
    })
    .await
    .unwrap();
    commit_alert_outbox(
        &pool,
        request(
            &wallet,
            "guard-first:0",
            "guard-first",
            "synthetic",
            &[chat_id],
        ),
    )
    .await
    .unwrap();

    let item = fetch_pending_batch(&pool, 1).await.unwrap().remove(0);
    let url = std::env::var("DATABASE_URL").unwrap();
    let guard_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    let delete_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&url)
        .await
        .unwrap();
    let guard_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&guard_pool)
        .await
        .unwrap();
    let delete_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&delete_pool)
        .await
        .unwrap();
    let claim = acquire_delivery_claim_guard(&guard_pool, item.id, &item.claim_token)
        .await
        .unwrap()
        .expect("current subscription must authorize this synthetic send boundary");

    let delete_repo = PostgresRepository::new(delete_pool.clone());
    let deleting = tokio::spawn(async move {
        delete_repo
            .remove_all_user_data(chat_id, chat_id as u64)
            .await
    });
    assert!(
        backend_waits_for(&admin, delete_pid, guard_pid).await,
        "privacy delete must serialize behind an already-authorized send boundary"
    );

    claim.mark_sent().await.unwrap();
    let summary = tokio::time::timeout(Duration::from_secs(3), deleting)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(summary.wallets_deleted, 1);
    let residual: (i64, i64) = sqlx::query_as(
        "SELECT
           (SELECT COUNT(*) FROM user_wallets WHERE chat_id=$1),
           (SELECT COUNT(*) FROM telegram_delivery_queue WHERE chat_id=$1)",
    )
    .bind(chat_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(residual, (0, 0));
    delete_pool.close().await;
}

#[tokio::test]
async fn fetched_delivery_rejects_stale_or_replaced_claim_identity() {
    let _guard = database_test_lock().lock().await;
    let pool = test_pool().await;
    let admin = admin_pool().await;
    reset_tables(&admin).await;
    let chat_id = next_id();
    enqueue_message(&pool, chat_id, "synthetic claim identity")
        .await
        .unwrap();

    let mut claimed = fetch_pending_batch(&pool, 1).await.unwrap();
    let item = claimed.remove(0);
    assert!(
        delivery_claim_is_current(&pool, item.id, &item.claim_token)
            .await
            .unwrap()
    );
    assert!(
        !delivery_claim_is_current(&pool, item.id, "stale-claim-token")
            .await
            .unwrap()
    );

    sqlx::query(
        "UPDATE telegram_delivery_queue
         SET locked_by = 'newer-attempt-token', locked_at = NOW(), status = 'processing'
         WHERE id = $1",
    )
    .bind(item.id)
    .execute(&pool)
    .await
    .unwrap();

    assert!(
        !delivery_claim_is_current(&pool, item.id, &item.claim_token)
            .await
            .unwrap(),
        "an older in-memory claim must not authorize a newer attempt"
    );
    assert!(
        delivery_claim_is_current(&pool, item.id, "newer-attempt-token")
            .await
            .unwrap(),
        "the durable current attempt must remain deliverable"
    );
}

#[tokio::test]
async fn fetched_delivery_rejects_non_processing_or_missing_rows() {
    let _guard = database_test_lock().lock().await;
    let pool = test_pool().await;
    let admin = admin_pool().await;
    reset_tables(&admin).await;
    let chat_id = next_id();
    enqueue_message(&pool, chat_id, "synthetic status transition")
        .await
        .unwrap();

    let mut claimed = fetch_pending_batch(&pool, 1).await.unwrap();
    let item = claimed.remove(0);
    sqlx::query("UPDATE telegram_delivery_queue SET status='failed' WHERE id=$1")
        .bind(item.id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        !delivery_claim_is_current(&pool, item.id, &item.claim_token)
            .await
            .unwrap()
    );

    sqlx::query("DELETE FROM telegram_delivery_queue WHERE id=$1")
        .bind(item.id)
        .execute(&pool)
        .await
        .unwrap();
    assert!(
        !delivery_claim_is_current(&pool, item.id, &item.claim_token)
            .await
            .unwrap()
    );
}

#[tokio::test]
async fn stale_snapshot_after_forget_excludes_deleted_chat() {
    use kaspa_pulse::domain::entities::TrackedWallet;
    use kaspa_pulse::infrastructure::database::postgres_adapter::PostgresRepository;
    let _guard = database_test_lock().lock().await;
    let pool = test_pool().await;
    let admin = admin_pool().await;
    reset_tables(&admin).await;
    let repo = PostgresRepository::new(pool.clone());
    let removed = next_id();
    let retained = next_id();
    let wallet = format!("kaspa:stale-recipient-{removed}");
    for chat_id in [removed, retained] {
        repo.add_tracked_wallet(TrackedWallet {
            address: wallet.clone(),
            chat_id,
        })
        .await
        .unwrap();
    }
    let snapshot = repo.get_subscribers_for_wallet(&wallet).await.unwrap();
    assert_eq!(snapshot, vec![removed, retained]);
    let summary = repo
        .remove_all_user_data(removed, removed as u64)
        .await
        .unwrap();
    assert_eq!(summary.wallets_deleted, 1);
    assert_eq!(
        repo.get_subscribers_for_wallet(&wallet).await.unwrap(),
        vec![retained]
    );
    let outcome = commit_alert_outbox(
        &pool,
        request(
            &wallet,
            "stale-snapshot:0",
            "stale-snapshot-key",
            "synthetic alert",
            &snapshot,
        ),
    )
    .await
    .unwrap();
    let queued: Vec<i64> = sqlx::query_scalar("SELECT chat_id FROM telegram_delivery_queue WHERE event_key='stale-snapshot:0' ORDER BY chat_id")
        .fetch_all(&pool).await.unwrap();
    println!(
        "stale snapshot={snapshot:?}; after delete expected=[{retained}]; actual queued={queued:?}; outcome={outcome:?}"
    );
    assert_eq!(
        queued,
        vec![retained],
        "outbox recreated user-linked queue data after deletion"
    );
}

#[tokio::test]
async fn stale_snapshot_after_last_subscriber_forget_creates_no_state() {
    use kaspa_pulse::domain::entities::TrackedWallet;
    use kaspa_pulse::infrastructure::database::postgres_adapter::PostgresRepository;
    let _guard = database_test_lock().lock().await;
    let pool = test_pool().await;
    let admin = admin_pool().await;
    reset_tables(&admin).await;
    let repo = PostgresRepository::new(pool.clone());
    let removed = next_id();
    let wallet = format!("kaspa:stale-orphan-{removed}");
    repo.add_tracked_wallet(TrackedWallet {
        address: wallet.clone(),
        chat_id: removed,
    })
    .await
    .unwrap();
    let snapshot = repo.get_subscribers_for_wallet(&wallet).await.unwrap();
    repo.remove_all_user_data(removed, removed as u64)
        .await
        .unwrap();
    assert!(
        repo.get_subscribers_for_wallet(&wallet)
            .await
            .unwrap()
            .is_empty()
    );
    let outcome = commit_alert_outbox(
        &pool,
        request(
            &wallet,
            "stale-orphan:0",
            "stale-orphan-key",
            "synthetic alert",
            &snapshot,
        ),
    )
    .await
    .unwrap();
    let queued: i64 =
        sqlx::query_scalar("SELECT count(*) FROM telegram_delivery_queue WHERE chat_id=$1")
            .bind(removed)
            .fetch_one(&pool)
            .await
            .unwrap();
    let wallet_rows: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM wallet_alert_dedup WHERE wallet=$1) + (SELECT count(*) FROM wallet_seen_utxos WHERE wallet=$1)").bind(&wallet).fetch_one(&pool).await.unwrap();
    println!(
        "removed last subscriber={removed}; queued={queued}; orphan rows={wallet_rows}; outcome={outcome:?}"
    );
    assert_eq!(
        (queued, wallet_rows),
        (0, 0),
        "stale work recreated deleted user or orphan wallet state"
    );
}

#[tokio::test]
async fn stale_wallet_writes_after_forget_do_not_recreate_orphan_state() {
    use kaspa_pulse::domain::entities::{MinedBlock, TrackedWallet};
    use kaspa_pulse::domain::models::UtxoRecord;
    use kaspa_pulse::infrastructure::database::postgres_adapter::PostgresRepository;
    let _guard = database_test_lock().lock().await;
    let pool = test_pool().await;
    let admin = admin_pool().await;
    reset_tables(&admin).await;
    let repo = PostgresRepository::new(pool.clone());
    let chat_id = next_id();
    let wallet = format!("kaspa:stale-writers-{chat_id}");
    let utxo = UtxoRecord {
        address: wallet.clone(),
        amount: 100,
        script_public_key: String::new(),
        outpoint: "orphan-writer:0".into(),
        block_daa_score: 10,
        is_coinbase: true,
        transaction_id: "synthetic".into(),
    };
    let block = MinedBlock {
        wallet_address: wallet.clone(),
        outpoint: utxo.outpoint.clone(),
        amount: 100,
        daa_score: 10,
    };
    repo.add_tracked_wallet(TrackedWallet {
        address: wallet.clone(),
        chat_id,
    })
    .await
    .unwrap();
    // Prove the same writers work while subscribed, then delete through the real API.
    repo.upsert_seen_utxos(&wallet, std::slice::from_ref(&utxo.outpoint))
        .await
        .unwrap();
    repo.upsert_pending_reward(&wallet, &utxo, 11, 1, 10)
        .await
        .unwrap();
    repo.record_mined_block(block.clone()).await.unwrap();
    repo.remove_all_user_data(chat_id, chat_id as u64)
        .await
        .unwrap();
    assert!(
        repo.get_subscribers_for_wallet(&wallet)
            .await
            .unwrap()
            .is_empty()
    );
    // These are the writes a previously started scan can issue after deletion.
    repo.upsert_seen_utxos(&wallet, std::slice::from_ref(&utxo.outpoint))
        .await
        .unwrap();
    repo.upsert_pending_reward(&wallet, &utxo, 11, 1, 10)
        .await
        .unwrap();
    repo.record_mined_block(block).await.unwrap();
    let counts: (i64, i64, i64) = sqlx::query_as("SELECT (SELECT count(*) FROM wallet_seen_utxos WHERE wallet=$1), (SELECT count(*) FROM pending_rewards WHERE wallet=$1), (SELECT count(*) FROM mined_blocks WHERE wallet=$1)")
        .bind(&wallet).fetch_one(&pool).await.unwrap();
    println!("after actual forget and stale worker writes: seen/pending/mined={counts:?}");
    assert_eq!(
        counts,
        (0, 0, 0),
        "stale workers restored deleted orphan wallet data"
    );
}

async fn backend_waits_for(pool: &PgPool, waiter: i32, blocker: i32) -> bool {
    tokio::time::timeout(Duration::from_secs(2), async {
        loop {
            let waiting: bool = sqlx::query_scalar("SELECT $2 = ANY(pg_blocking_pids($1))")
                .bind(waiter)
                .bind(blocker)
                .fetch_one(pool)
                .await
                .unwrap();
            if waiting {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .is_ok()
}

#[tokio::test]
async fn outbox_serializes_with_real_inflight_privacy_deletion() {
    use kaspa_pulse::domain::entities::TrackedWallet;
    use kaspa_pulse::infrastructure::database::postgres_adapter::PostgresRepository;
    let _guard = database_test_lock().lock().await;
    let pool = test_pool().await;
    let admin = admin_pool().await;
    reset_tables(&admin).await;
    let repo = PostgresRepository::new(pool.clone());
    let chat = next_id();
    let wallet = format!("kaspa:privacy-overlap-{chat}");
    repo.add_tracked_wallet(TrackedWallet {
        address: wallet.clone(),
        chat_id: chat,
    })
    .await
    .unwrap();
    let snapshot = repo.get_subscribers_for_wallet(&wallet).await.unwrap();
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE OR REPLACE FUNCTION audit_pause_privacy_delete() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN PERFORM pg_advisory_xact_lock(1263552333, 909); RETURN OLD; END $$;
         CREATE TRIGGER audit_pause_privacy_delete AFTER DELETE ON user_wallets
         FOR EACH ROW WHEN (OLD.chat_id = {chat}) EXECUTE FUNCTION audit_pause_privacy_delete()"
    ))).execute(&admin).await.unwrap();
    let url = std::env::var("DATABASE_URL").unwrap();
    let make_pool = || {
        PgPoolOptions::new()
            .max_connections(1)
            .after_connect(|conn, _| {
                Box::pin(async move {
                    sqlx::query("SET statement_timeout = '8000'")
                        .execute(conn)
                        .await?;
                    Ok(())
                })
            })
            .connect(&url)
    };
    let deleting_pool = make_pool().await.unwrap();
    let outbox_pool = make_pool().await.unwrap();
    let delete_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&deleting_pool)
        .await
        .unwrap();
    let outbox_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&outbox_pool)
        .await
        .unwrap();
    let mut gate = admin.begin().await.unwrap();
    let gate_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(1263552333, 909)")
        .execute(&mut *gate)
        .await
        .unwrap();
    let deletion_repo = PostgresRepository::new(deleting_pool.clone());
    let deleting =
        tokio::spawn(async move { deletion_repo.remove_all_user_data(chat, chat as u64).await });
    let deletion_paused = backend_waits_for(&admin, delete_pid, gate_pid).await;
    let outbox_conn = outbox_pool.clone();
    let enqueuing = tokio::spawn(async move {
        commit_alert_outbox(
            &outbox_conn,
            request(
                &wallet,
                "privacy-overlap:0",
                "privacy-overlap",
                "synthetic",
                &snapshot,
            ),
        )
        .await
    });
    let outbox_waited = backend_waits_for(&admin, outbox_pid, delete_pid).await;
    // Release only the synthetic transaction gate; join both real repository calls.
    gate.rollback().await.unwrap();
    let deletion_result = tokio::time::timeout(Duration::from_secs(3), deleting).await;
    let outbox_result = tokio::time::timeout(Duration::from_secs(3), enqueuing).await;
    sqlx::raw_sql("DROP TRIGGER audit_pause_privacy_delete ON user_wallets; DROP FUNCTION audit_pause_privacy_delete()")
        .execute(&admin).await.unwrap();
    deleting_pool.close().await;
    outbox_pool.close().await;
    assert!(
        deletion_paused,
        "actual privacy transaction did not reach controlled gate"
    );
    assert!(
        outbox_waited,
        "outbox did not wait for the actual deletion ownership lock"
    );
    assert!(deletion_result.unwrap().unwrap().is_ok());
    assert_eq!(
        outbox_result.unwrap().unwrap().unwrap(),
        AlertOutboxOutcome::NoCurrentRecipients
    );
    let count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM telegram_delivery_queue WHERE chat_id=$1")
            .bind(chat)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(count, 0);
    println!(
        "actual deletion backend={delete_pid}; outbox backend={outbox_pid}; observed serialized=true; residual queue=0"
    );
}

#[tokio::test]
async fn removed_subscription_is_excluded_but_new_subscription_works() {
    use kaspa_pulse::domain::entities::TrackedWallet;
    use kaspa_pulse::infrastructure::database::postgres_adapter::PostgresRepository;
    let _guard = database_test_lock().lock().await;
    let pool = test_pool().await;
    let admin = admin_pool().await;
    reset_tables(&admin).await;
    let repo = PostgresRepository::new(pool.clone());
    let chat = next_id();
    let wallet = format!("kaspa:reactivated-{chat}");
    let tracked = TrackedWallet {
        address: wallet.clone(),
        chat_id: chat,
    };
    repo.add_tracked_wallet(tracked.clone()).await.unwrap();
    repo.remove_tracked_wallet(&wallet, chat).await.unwrap();
    assert_eq!(
        commit_alert_outbox(
            &pool,
            request(
                &wallet,
                "reactivated:0",
                "reactivated-0",
                "synthetic",
                &[chat]
            )
        )
        .await
        .unwrap(),
        AlertOutboxOutcome::NoCurrentRecipients
    );
    repo.add_tracked_wallet(tracked).await.unwrap();
    assert_eq!(
        commit_alert_outbox(
            &pool,
            request(
                &wallet,
                "reactivated:1",
                "reactivated-1",
                "synthetic",
                &[chat]
            )
        )
        .await
        .unwrap(),
        AlertOutboxOutcome::Enqueued { recipients: 1 }
    );
    repo.remove_all_user_wallets(chat).await.unwrap();
    assert_eq!(
        commit_alert_outbox(
            &pool,
            request(
                &wallet,
                "reactivated:2",
                "reactivated-2",
                "synthetic",
                &[chat]
            )
        )
        .await
        .unwrap(),
        AlertOutboxOutcome::NoCurrentRecipients
    );
}
