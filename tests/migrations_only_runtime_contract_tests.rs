use kaspa_pulse::domain::entities::{MinedBlock, TrackedWallet};
use kaspa_pulse::domain::models::{BotEventRecord, BotEventType, EventSeverity, UtxoRecord};
use kaspa_pulse::infrastructure::database::postgres_adapter::PostgresRepository;
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::error::Error;
use std::str::FromStr;
use std::time::{SystemTime, UNIX_EPOCH};

type TestResult<T> = Result<T, Box<dyn Error + Send + Sync>>;

async fn exercise_migrated_database(
    admin: &PgPool,
    app: &PgPool,
) -> TestResult<Vec<(&'static str, bool)>> {
    let mut migrations = std::fs::read_dir(concat!(env!("CARGO_MANIFEST_DIR"), "/migrations"))?
        .map(|entry| entry.map(|entry| entry.path()))
        .collect::<Result<Vec<_>, _>>()?;
    migrations.retain(|path| path.extension().is_some_and(|ext| ext == "sql"));
    migrations.sort();
    for migration in &migrations {
        // SQL comes only from reviewed, versioned repository migration files.
        sqlx::raw_sql(sqlx::AssertSqlSafe(std::fs::read_to_string(migration)?))
            .execute(admin)
            .await?;
    }
    // No fixture GRANT, runtime schema ensure, or CI preparation runs in this DB.
    let role: String = sqlx::query_scalar("SELECT current_user")
        .fetch_one(app)
        .await?;
    if role != "kaspa_pulse_app" {
        return Err("migration contract must run as the actual application role".into());
    }
    let repo = PostgresRepository::new(app.clone());
    repo.load_persisted_runtime_settings().await?;
    repo.update_setting("MAINTENANCE_MODE", "true").await?;
    let maintenance = repo
        .load_persisted_runtime_settings()
        .await?
        .maintenance_mode;
    repo.update_setting("MAINTENANCE_MODE", "false").await?;
    let chat_id = 9_612_001_i64;
    let address = "kaspa:f12-migrations-only-wallet";
    let wallet = TrackedWallet {
        address: address.into(),
        chat_id,
    };
    repo.add_tracked_wallet(wallet.clone()).await?;
    repo.add_tracked_wallet(wallet).await?;
    let wallet_count = repo.count_user_wallets(chat_id).await?;
    let outpoints = vec!["f12-outpoint".to_string()];
    repo.upsert_seen_utxos(address, &outpoints).await?;
    repo.upsert_seen_utxos(address, &outpoints).await?;
    let seen = repo.get_seen_utxos(address).await?;
    let utxo = UtxoRecord {
        address: address.into(),
        amount: 100,
        script_public_key: String::new(),
        outpoint: outpoints[0].clone(),
        block_daa_score: 10,
        is_coinbase: true,
        transaction_id: "f12-tx".into(),
    };
    repo.upsert_pending_reward(address, &utxo, 11, 1, 10)
        .await?;
    repo.upsert_pending_reward(address, &utxo, 12, 2, 10)
        .await?;
    let pending: (i64, i64) =
        sqlx::query_as("SELECT attempts, confirmations FROM pending_rewards WHERE wallet=$1")
            .bind(address)
            .fetch_one(app)
            .await?;
    let block = MinedBlock {
        wallet_address: address.into(),
        outpoint: outpoints[0].clone(),
        amount: 100,
        daa_score: 10,
    };
    repo.record_mined_block(block.clone()).await?;
    repo.record_mined_block(block).await?;
    let mined = repo.get_lifetime_stats(address).await?;
    let first_claim = repo
        .try_claim_alert_key(address, "f12-alert", None, None)
        .await?;
    let repeated_claim = repo
        .try_claim_alert_key(address, "f12-alert", None, None)
        .await?;
    repo.record_bot_event_record(BotEventRecord::new(
        BotEventType::SystemStart,
        EventSeverity::Info,
    ))
    .await?;
    let deleted = repo.remove_all_user_data(chat_id, chat_id as u64).await?;
    let remaining: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM user_wallets) + (SELECT count(*) FROM wallet_seen_utxos) + (SELECT count(*) FROM pending_rewards) + (SELECT count(*) FROM mined_blocks) + (SELECT count(*) FROM wallet_alert_dedup)")
        .fetch_one(app).await?;
    println!(
        "F12: {} versioned migrations; actual application role; no CI grants",
        migrations.len()
    );
    Ok(vec![
        ("persisted settings read/insert/update", maintenance),
        ("wallet subscription insert/update", wallet_count == 1),
        (
            "seen UTXO insert/update",
            seen.len() == 1 && seen.contains(&outpoints[0]),
        ),
        ("pending reward insert/update", pending == (2, 2)),
        ("mined block insert and deduplication", mined == (1, 100)),
        (
            "alert claim insert and deduplication",
            first_claim && !repeated_claim,
        ),
        (
            "forget-all deletes owned state",
            deleted.wallets_deleted == 1 && deleted.orphan_wallets_cleaned == 1 && remaining == 0,
        ),
    ])
}

#[tokio::test]
async fn f12_versioned_migrations_alone_enable_real_runtime_workflows() {
    let admin_options = PgConnectOptions::from_str(
        &std::env::var("DATABASE_ADMIN_URL").expect("DATABASE_ADMIN_URL is required"),
    )
    .unwrap();
    let app_options = PgConnectOptions::from_str(
        &std::env::var("DATABASE_URL").expect("DATABASE_URL is required"),
    )
    .unwrap();
    let control = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(admin_options.clone())
        .await
        .unwrap();
    let suffix = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_nanos();
    let database = format!("f12_migrations_{}_{}", std::process::id(), suffix);
    // The identifier is a fixed prefix plus numeric process/time values.
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE DATABASE {database}")))
        .execute(&control)
        .await
        .unwrap();
    let admin = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(admin_options.database(&database))
        .await
        .unwrap();
    let app = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(app_options.database(&database))
        .await
        .unwrap();
    let outcome = exercise_migrated_database(&admin, &app).await;
    // Database isolation keeps grants and data out of parallel test fixtures.
    // Close/drop before checking the outcome, including the expected RED path.
    app.close().await;
    admin.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!(
        "DROP DATABASE {database} WITH (FORCE)"
    )))
    .execute(&control)
    .await
    .unwrap();
    control.close().await;
    for (operation, passed) in outcome.expect("migrations alone must support runtime operations") {
        assert!(passed, "runtime contract failed: {operation}");
    }
}
