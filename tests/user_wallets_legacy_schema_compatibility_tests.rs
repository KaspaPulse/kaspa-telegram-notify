use kaspa_pulse::domain::entities::TrackedWallet;
use kaspa_pulse::infrastructure::database::postgres_adapter::PostgresRepository;
use kaspa_pulse::infrastructure::metrics::DB_ERRORS;
use kaspa_pulse::infrastructure::telegram_delivery_queue::{
    AlertOutboxOutcome, AlertOutboxRequest, acquire_delivery_claim_guard, commit_alert_outbox,
    fetch_pending_batch,
};
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::error::Error;
use std::str::FromStr;
use std::sync::atomic::Ordering;
use std::time::{SystemTime, UNIX_EPOCH};

type TestResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

const MIGRATION_NAME: &str = "20260915_000000_user_wallets_created_at_contract.sql";
const LEGACY_WALLET: &str = "kaspa:legacy-created-at-canary";
const LEGACY_CHAT: i64 = 424_242;
const LEGACY_LAST_ACTIVE: &str = "2026-01-02T03:04:05Z";

fn migration_paths() -> TestResult<Vec<std::path::PathBuf>> {
    let mut paths = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    paths.retain(|path| path.extension().is_some_and(|ext| ext == "sql"));
    paths.sort();
    Ok(paths)
}
fn admin_options() -> PgConnectOptions {
    PgConnectOptions::from_str(
        &std::env::var("DATABASE_ADMIN_URL").expect("DATABASE_ADMIN_URL is required"),
    )
    .unwrap()
}

fn app_options() -> PgConnectOptions {
    PgConnectOptions::from_str(&std::env::var("DATABASE_URL").expect("DATABASE_URL is required"))
        .unwrap()
}

async fn create_database(prefix: &str) -> (PgPool, PgPool, String) {
    let options = admin_options();
    let control = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await
        .unwrap();
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let database = format!("{prefix}_{}_{}", std::process::id(), suffix);
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {database}")))
        .execute(&control)
        .await
        .unwrap();
    let admin = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options.database(&database))
        .await
        .unwrap();
    (control, admin, database)
}

async fn app_pool(database: &str) -> PgPool {
    PgPoolOptions::new()
        .max_connections(3)
        .connect_with(app_options().database(database))
        .await
        .unwrap()
}

async fn drop_database(control: PgPool, admin: PgPool, app: Option<PgPool>, database: &str) {
    if let Some(app) = app {
        app.close().await;
    }
    admin.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE {database} WITH (FORCE)"
    )))
    .execute(&control)
    .await
    .unwrap();
    control.close().await;
}

async fn create_exact_legacy_fixture(admin: &PgPool) -> TestResult<()> {
    sqlx::raw_sql(
        "CREATE TABLE user_wallets (
            wallet TEXT NOT NULL,
            chat_id BIGINT NOT NULL,
            last_active TIMESTAMPTZ DEFAULT CURRENT_TIMESTAMP,
            alerts_enabled BOOLEAN DEFAULT true,
            PRIMARY KEY (wallet, chat_id)
        );",
    )
    .execute(admin)
    .await?;
    sqlx::query(
        "INSERT INTO user_wallets(wallet,chat_id,last_active,alerts_enabled)
         VALUES ($1,$2,$3::timestamptz,false)",
    )
    .bind(LEGACY_WALLET)
    .bind(LEGACY_CHAT)
    .bind(LEGACY_LAST_ACTIVE)
    .execute(admin)
    .await?;
    Ok(())
}

async fn apply_all_migrations(admin: &PgPool) -> TestResult<()> {
    for path in migration_paths()? {
        let sql = std::fs::read_to_string(path)?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(sql))
            .execute(admin)
            .await?;
    }
    Ok(())
}
async fn assert_created_at_contract(admin: &PgPool) -> TestResult<()> {
    let row: (String, String, Option<String>) = sqlx::query_as(
        "SELECT data_type,is_nullable,column_default
         FROM information_schema.columns
         WHERE table_schema='public' AND table_name='user_wallets'
           AND column_name='created_at'",
    )
    .fetch_one(admin)
    .await?;
    assert_eq!(row.0, "timestamp with time zone");
    assert_eq!(row.1, "NO");
    assert!(
        row.2
            .as_deref()
            .is_some_and(|value| value.contains("now()"))
    );
    Ok(())
}

async fn enable_alert_delivery(admin: &PgPool) -> TestResult<()> {
    sqlx::query(
        "INSERT INTO system_settings(key_name,value_data)
         VALUES ('ENABLE_ALERT_DELIVERY','true')
         ON CONFLICT(key_name)
         DO UPDATE SET value_data=EXCLUDED.value_data,updated_at=NOW()",
    )
    .execute(admin)
    .await?;
    Ok(())
}

fn outbox_request<'a>(wallet: &'a str, chat_ids: &'a [i64]) -> AlertOutboxRequest<'a> {
    AlertOutboxRequest {
        wallet,
        source_outpoint: "legacy-created-at:0",
        alert_key: "legacy-created-at-alert",
        message_html: "legacy-created-at",
        chat_ids,
        txid_masked: Some("legacy...tx"),
        block_hash_masked: Some("legacy...block"),
        amount_kas: Some(1.0),
        daa_score: Some(1),
    }
}

#[tokio::test]
async fn legacy_production_schema_upgrades_preserving_data_and_delivery_guard() {
    let (control, admin, database) = create_database("legacy_created_at").await;
    let outcome: TestResult<()> = async {
        create_exact_legacy_fixture(&admin).await?;
        apply_all_migrations(&admin).await?;
        assert_created_at_contract(&admin).await?;

        let row: (
            String,
            i64,
            Option<chrono::DateTime<chrono::Utc>>,
            Option<bool>,
            chrono::DateTime<chrono::Utc>,
        ) = sqlx::query_as(
            "SELECT wallet,chat_id,last_active,alerts_enabled,created_at
                 FROM user_wallets WHERE wallet=$1 AND chat_id=$2",
        )
        .bind(LEGACY_WALLET)
        .bind(LEGACY_CHAT)
        .fetch_one(&admin)
        .await?;
        assert_eq!(row.0, LEGACY_WALLET);
        assert_eq!(row.1, LEGACY_CHAT);
        assert_eq!(row.3, Some(false));
        assert_eq!(
            row.4,
            row.2.expect("legacy last_active canary must be preserved")
        );
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM user_wallets WHERE wallet=$1 AND chat_id=$2")
                .bind(LEGACY_WALLET)
                .bind(LEGACY_CHAT)
                .fetch_one(&admin)
                .await?;
        assert_eq!(count, 1, "migration must not duplicate legacy rows");

        // v1.2.3-compatible write pattern must remain valid after the additive upgrade.
        sqlx::query(
            "INSERT INTO user_wallets(wallet,chat_id) VALUES ('kaspa:rollback-compat',424243)
             ON CONFLICT(wallet,chat_id) DO UPDATE SET last_active=CURRENT_TIMESTAMP",
        )
        .execute(&admin)
        .await?;
        enable_alert_delivery(&admin).await?;
        let app = app_pool(&database).await;
        let repo = PostgresRepository::new(app.clone());
        let chat_id = 424_244_i64;
        let wallet = "kaspa:legacy-claim-guard".to_string();
        repo.add_tracked_wallet(TrackedWallet {
            address: wallet.clone(),
            chat_id,
        })
        .await?;
        let db_errors_before = DB_ERRORS.load(Ordering::Relaxed);
        let outcome = commit_alert_outbox(&app, outbox_request(&wallet, &[chat_id])).await?;
        assert_eq!(outcome, AlertOutboxOutcome::Enqueued { recipients: 1 });
        let item = fetch_pending_batch(&app, 1).await?.remove(0);
        let guard = acquire_delivery_claim_guard(&app, item.id, &item.claim_token)
            .await?
            .expect("migrated legacy subscription must authorize its new queued event");
        guard.mark_sent().await?;
        assert_eq!(DB_ERRORS.load(Ordering::Relaxed), db_errors_before);

        let selected: (String, chrono::DateTime<chrono::Utc>) = sqlx::query_as(
            "SELECT wallet,created_at FROM user_wallets WHERE wallet=$1 AND chat_id=$2",
        )
        .bind(&wallet)
        .bind(chat_id)
        .fetch_one(&app)
        .await?;
        assert_eq!(selected.0, wallet);
        app.close().await;
        Ok(())
    }
    .await;
    drop_database(control, admin, None, &database).await;
    outcome.expect("exact legacy Production schema must upgrade safely");
}

#[tokio::test]
async fn fresh_install_full_migrations_has_created_at_contract() {
    let (control, admin, database) = create_database("fresh_created_at").await;
    let outcome: TestResult<()> = async {
        apply_all_migrations(&admin).await?;
        assert_created_at_contract(&admin).await?;
        let columns: Vec<String> = sqlx::query_scalar(
            "SELECT column_name FROM information_schema.columns
             WHERE table_schema='public' AND table_name='user_wallets'
             ORDER BY ordinal_position",
        )
        .fetch_all(&admin)
        .await?;
        assert!(columns.iter().any(|column| column == "created_at"));
        Ok(())
    }
    .await;
    drop_database(control, admin, None, &database).await;
    outcome.expect("fresh install migrations must retain the created_at contract");
}

#[tokio::test]
async fn compatibility_migration_is_idempotent_on_already_migrated_database() {
    let (control, admin, database) = create_database("repeat_created_at").await;
    let outcome: TestResult<()> = async {
        create_exact_legacy_fixture(&admin).await?;
        apply_all_migrations(&admin).await?;
        let before: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
            "SELECT created_at FROM user_wallets WHERE wallet=$1 AND chat_id=$2",
        )
        .bind(LEGACY_WALLET)
        .bind(LEGACY_CHAT)
        .fetch_one(&admin)
        .await?;
        let migration = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("migrations")
                .join(MIGRATION_NAME),
        )?;
        for _ in 0..2 {
            sqlx::raw_sql(sqlx::AssertSqlSafe(migration.clone()))
                .execute(&admin)
                .await?;
        }
        assert_created_at_contract(&admin).await?;
        let after: chrono::DateTime<chrono::Utc> = sqlx::query_scalar(
            "SELECT created_at FROM user_wallets WHERE wallet=$1 AND chat_id=$2",
        )
        .bind(LEGACY_WALLET)
        .bind(LEGACY_CHAT)
        .fetch_one(&admin)
        .await?;
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM user_wallets")
            .fetch_one(&admin)
            .await?;
        assert_eq!(after, before);
        assert_eq!(count, 1);
        Ok(())
    }
    .await;
    drop_database(control, admin, None, &database).await;
    outcome.expect("reapplying the compatibility migration must be safe");
}

#[tokio::test]
async fn compatibility_migration_rolls_back_atomically_on_transaction_failure() {
    let (control, admin, database) = create_database("rollback_created_at").await;
    let outcome: TestResult<()> = async {
        create_exact_legacy_fixture(&admin).await?;
        let migration = std::fs::read_to_string(
            std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
                .join("migrations")
                .join(MIGRATION_NAME),
        )?;
        let mut transaction = admin.begin().await?;
        sqlx::raw_sql(sqlx::AssertSqlSafe(migration))
            .execute(&mut *transaction)
            .await?;
        let failure = sqlx::query("SELECT * FROM deliberate_missing_relation_for_rollback_test")
            .execute(&mut *transaction)
            .await;
        assert!(failure.is_err(), "synthetic transaction failure must occur");
        transaction.rollback().await?;

        let created_at_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS (
               SELECT 1 FROM information_schema.columns
               WHERE table_schema='public' AND table_name='user_wallets'
                 AND column_name='created_at'
             )",
        )
        .fetch_one(&admin)
        .await?;
        let count: i64 =
            sqlx::query_scalar("SELECT count(*) FROM user_wallets WHERE wallet=$1 AND chat_id=$2")
                .bind(LEGACY_WALLET)
                .bind(LEGACY_CHAT)
                .fetch_one(&admin)
                .await?;
        assert!(
            !created_at_exists,
            "failed transaction must not leave a partial schema change"
        );
        assert_eq!(count, 1, "failed transaction must preserve legacy rows");
        Ok(())
    }
    .await;
    drop_database(control, admin, None, &database).await;
    outcome.expect("compatibility migration must be transaction-safe when the runner rolls back");
}
