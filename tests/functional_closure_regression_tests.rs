use kaspa_pulse::domain::entities::WalletRemovalOutcome;
use kaspa_pulse::infrastructure::database::postgres_adapter::PostgresRepository;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use std::sync::OnceLock;
use tokio::sync::Mutex;

fn db_lock() -> &'static Mutex<()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
}

async fn app_pool() -> PgPool {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL is required");
    PgPoolOptions::new()
        .max_connections(8)
        .connect(&url)
        .await
        .expect("application PostgreSQL must be reachable")
}

async fn admin_pool() -> PgPool {
    let url = std::env::var("DATABASE_ADMIN_URL").expect("DATABASE_ADMIN_URL is required");
    PgPoolOptions::new()
        .max_connections(4)
        .connect(&url)
        .await
        .expect("admin PostgreSQL must be reachable")
}

#[tokio::test]
async fn f01_forget_all_removes_every_allowed_user_link_and_orphan_wallet_state() {
    let _guard = db_lock().lock().await;
    let pool = app_pool().await;
    let admin = admin_pool().await;
    let repo = PostgresRepository::new(pool);
    let chat_id = 9_601_001_i64;
    let actor_user_id = 9_601_001_u64;
    let wallet = "kaspa:functional-closure-f01-wallet";

    for statement in [
        "DELETE FROM telegram_delivery_queue WHERE chat_id = $1",
        "DELETE FROM bot_event_log WHERE chat_id = $1",
        "DELETE FROM chat_history WHERE chat_id = $1",
        "DELETE FROM user_wallets WHERE chat_id = $1",
    ] {
        sqlx::query(statement)
            .bind(chat_id)
            .execute(&admin)
            .await
            .unwrap();
    }
    sqlx::query("DELETE FROM admin_audit_log WHERE action = 'f01_test' OR admin_chat_id = $1 OR admin_actor_user_id = $1")
        .bind(chat_id)
        .execute(&admin)
        .await
        .unwrap();
    for statement in [
        "DELETE FROM wallet_seen_utxos WHERE wallet = $1",
        "DELETE FROM wallet_alert_dedup WHERE wallet = $1",
        "DELETE FROM pending_rewards WHERE wallet = $1",
        "DELETE FROM mined_blocks WHERE wallet = $1",
    ] {
        sqlx::query(statement)
            .bind(wallet)
            .execute(&admin)
            .await
            .unwrap();
    }

    sqlx::query("INSERT INTO user_wallets (wallet, chat_id) VALUES ($1, $2)")
        .bind(wallet)
        .bind(chat_id)
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("INSERT INTO bot_event_log (event_type, severity, chat_id) VALUES ('ADMIN_ACTION','info',$1)")
        .bind(chat_id).execute(&admin).await.unwrap();
    sqlx::query("INSERT INTO chat_history (chat_id, role, content) VALUES ($1,'user','legacy')")
        .bind(chat_id)
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO telegram_delivery_queue (chat_id, message_html) VALUES ($1,'<b>queued</b>')",
    )
    .bind(chat_id)
    .execute(&admin)
    .await
    .unwrap();
    sqlx::query("INSERT INTO admin_audit_log (admin_actor_user_id, admin_chat_id, action, status) VALUES ($1,$1,'f01_test','confirmed')")
        .bind(chat_id).execute(&admin).await.unwrap();
    sqlx::query("INSERT INTO wallet_seen_utxos (wallet, outpoint) VALUES ($1,'f01:seen')")
        .bind(wallet)
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("INSERT INTO wallet_alert_dedup (wallet, alert_key) VALUES ($1,'f01-alert')")
        .bind(wallet)
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("INSERT INTO pending_rewards (wallet,outpoint,txid,amount,reward_daa_score,virtual_daa_score) VALUES ($1,'f01:pending','f01tx',1,1,1)")
        .bind(wallet).execute(&admin).await.unwrap();
    sqlx::query(
        "INSERT INTO mined_blocks (wallet,outpoint,amount,daa_score) VALUES ($1,'f01:mined',1,1)",
    )
    .bind(wallet)
    .execute(&admin)
    .await
    .unwrap();

    let summary = repo
        .remove_all_user_data(chat_id, actor_user_id)
        .await
        .unwrap();
    assert_eq!(summary.wallets_deleted, 1);
    assert_eq!(summary.event_rows_deleted, 1);
    assert_eq!(summary.chat_history_rows_deleted, 1);
    assert_eq!(summary.queue_rows_deleted, 1);
    assert_eq!(summary.admin_audit_rows_anonymized, 1);
    assert_eq!(summary.orphan_wallet_addresses, vec![wallet.to_string()]);

    for statement in [
        "SELECT COUNT(*) FROM user_wallets WHERE chat_id = $1",
        "SELECT COUNT(*) FROM bot_event_log WHERE chat_id = $1",
        "SELECT COUNT(*) FROM chat_history WHERE chat_id = $1",
        "SELECT COUNT(*) FROM telegram_delivery_queue WHERE chat_id = $1",
        "SELECT COUNT(*) FROM admin_audit_log WHERE admin_chat_id = $1 OR admin_actor_user_id = $1",
    ] {
        let count: i64 = sqlx::query_scalar(statement)
            .bind(chat_id)
            .fetch_one(&admin)
            .await
            .unwrap();
        assert_eq!(count, 0, "direct user linkage remained for {statement}");
    }
    let anonymized: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM admin_audit_log WHERE action='f01_test' AND admin_chat_id=0 AND admin_actor_user_id IS NULL",
    ).fetch_one(&admin).await.unwrap();
    assert_eq!(anonymized, 1);

    for statement in [
        "SELECT COUNT(*) FROM wallet_seen_utxos WHERE wallet = $1",
        "SELECT COUNT(*) FROM wallet_alert_dedup WHERE wallet = $1",
        "SELECT COUNT(*) FROM pending_rewards WHERE wallet = $1",
        "SELECT COUNT(*) FROM mined_blocks WHERE wallet = $1",
    ] {
        let count: i64 = sqlx::query_scalar(statement)
            .bind(wallet)
            .fetch_one(&admin)
            .await
            .unwrap();
        assert_eq!(count, 0, "orphan wallet state remained for {statement}");
    }
}

#[tokio::test]
async fn f07_remove_wallet_distinguishes_removed_from_not_found() {
    let _guard = db_lock().lock().await;
    let pool = app_pool().await;
    let admin = admin_pool().await;
    let repo = PostgresRepository::new(pool);
    let chat_id = 9_607_001_i64;
    let wallet = "kaspa:functional-closure-f07-wallet";

    sqlx::query("DELETE FROM user_wallets WHERE chat_id = $1")
        .bind(chat_id)
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("INSERT INTO user_wallets (wallet, chat_id) VALUES ($1, $2)")
        .bind(wallet)
        .bind(chat_id)
        .execute(&admin)
        .await
        .unwrap();

    assert_eq!(
        repo.remove_tracked_wallet(wallet, chat_id).await.unwrap(),
        WalletRemovalOutcome::Removed
    );
    assert_eq!(
        repo.remove_tracked_wallet(wallet, chat_id).await.unwrap(),
        WalletRemovalOutcome::NotFound
    );
}

#[tokio::test]
async fn f02_stale_wallet_callback_cannot_retarget_after_list_changes() {
    use kaspa_pulse::presentation::telegram::handlers::wallet::{
        resolve_wallet_token, wallet_callback_token,
    };

    let _guard = db_lock().lock().await;
    let pool = app_pool().await;
    let admin = admin_pool().await;
    let repo = PostgresRepository::new(pool);
    let chat_id = 9_602_001_i64;
    let wallet_a = "kaspa:a-functional-closure-wallet";
    let wallet_b = "kaspa:b-functional-closure-wallet";

    sqlx::query("DELETE FROM user_wallets WHERE chat_id = $1")
        .bind(chat_id)
        .execute(&admin)
        .await
        .unwrap();
    for wallet in [wallet_a, wallet_b] {
        sqlx::query("INSERT INTO user_wallets (wallet, chat_id) VALUES ($1, $2)")
            .bind(wallet)
            .bind(chat_id)
            .execute(&admin)
            .await
            .unwrap();
    }

    let before = repo
        .get_tracked_wallets_for_chat(chat_id)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.address)
        .collect::<Vec<_>>();
    assert_eq!(before, vec![wallet_a.to_string(), wallet_b.to_string()]);
    let stale_a_token = wallet_callback_token(chat_id, wallet_a);
    assert_eq!(
        resolve_wallet_token(&before, chat_id, &stale_a_token),
        Some((0, wallet_a))
    );

    sqlx::query("DELETE FROM user_wallets WHERE chat_id = $1 AND wallet = $2")
        .bind(chat_id)
        .bind(wallet_a)
        .execute(&admin)
        .await
        .unwrap();

    let after = repo
        .get_tracked_wallets_for_chat(chat_id)
        .await
        .unwrap()
        .into_iter()
        .map(|row| row.address)
        .collect::<Vec<_>>();
    assert_eq!(after, vec![wallet_b.to_string()]);
    assert_eq!(after[0], wallet_b);
    assert!(resolve_wallet_token(&after, chat_id, &stale_a_token).is_none());
    assert_ne!(stale_a_token, wallet_callback_token(chat_id, wallet_b));
}
