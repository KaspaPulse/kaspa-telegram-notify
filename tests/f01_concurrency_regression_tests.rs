use kaspa_pulse::domain::entities::TrackedWallet;
use kaspa_pulse::infrastructure::database::postgres_adapter::PostgresRepository;
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;

async fn scoped_pool(variable: &str, schema: &str, connections: u32) -> PgPool {
    let url = std::env::var(variable).expect("PostgreSQL test URL is required");
    let schema = schema.to_string();
    PgPoolOptions::new()
        .max_connections(connections)
        .after_connect(move |connection, _| {
            let schema = schema.clone();
            Box::pin(async move {
                sqlx::query(
                    "SELECT set_config('search_path', $1, false),
                            set_config('statement_timeout', '15000', false)",
                )
                .bind(schema)
                .execute(connection)
                .await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .expect("isolated PostgreSQL pool must connect")
}

async fn waits_for(admin: &PgPool, waiter: i32, blocker: i32) -> bool {
    for _ in 0..150 {
        let blocked: bool = sqlx::query_scalar("SELECT $2 = ANY(pg_blocking_pids($1))")
            .bind(waiter)
            .bind(blocker)
            .fetch_one(admin)
            .await
            .unwrap();
        if blocked {
            return true;
        }
        tokio::time::sleep(Duration::from_millis(20)).await;
    }
    false
}

#[tokio::test]
async fn f01_concurrent_subscriber_keeps_shared_wallet_state() {
    // A private schema keeps trigger fault injection away from other test binaries.
    // Dynamic fixture SQL uses only a numeric process ID and hard-coded table
    // names/constants; no externally supplied value is interpolated into SQL.
    let schema = format!("f01_concurrent_{}", std::process::id());
    let admin = scoped_pool("DATABASE_ADMIN_URL", &schema, 4).await;
    sqlx::query(sqlx::AssertSqlSafe(format!("CREATE SCHEMA {schema}")))
        .execute(&admin)
        .await
        .unwrap();
    for table in [
        "user_wallets",
        "telegram_delivery_queue",
        "bot_event_log",
        "chat_history",
        "admin_audit_log",
        "wallet_seen_utxos",
        "wallet_alert_dedup",
        "pending_rewards",
        "mined_blocks",
    ] {
        sqlx::query(sqlx::AssertSqlSafe(format!(
            "CREATE TABLE {schema}.{table} (LIKE public.{table} INCLUDING ALL)"
        )))
        .execute(&admin)
        .await
        .unwrap();
    }
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "GRANT USAGE ON SCHEMA {schema} TO kaspa_pulse_app;
         GRANT SELECT, INSERT, UPDATE, DELETE ON ALL TABLES IN SCHEMA {schema} TO kaspa_pulse_app"
    )))
    .execute(&admin)
    .await
    .unwrap();

    const OLD_CHAT: i64 = 9_611_001;
    const NEW_CHAT: i64 = 9_611_002;
    const SHARED: &str = "kaspa:f01-concurrent-shared";
    const ORPHAN: &str = "kaspa:f01-concurrent-orphan";
    for wallet in [SHARED, ORPHAN] {
        sqlx::query("INSERT INTO user_wallets (wallet, chat_id) VALUES ($1, $2)")
            .bind(wallet)
            .bind(OLD_CHAT)
            .execute(&admin)
            .await
            .unwrap();
        for statement in [
            "INSERT INTO wallet_seen_utxos (wallet, outpoint) VALUES ($1, 'race:seen')",
            "INSERT INTO wallet_alert_dedup (wallet, alert_key) VALUES ($1, 'race:dedup')",
            "INSERT INTO pending_rewards (wallet,outpoint,txid,amount,reward_daa_score,virtual_daa_score) VALUES ($1,'race:pending','race:tx',1,1,1)",
            "INSERT INTO mined_blocks (wallet,outpoint,amount,daa_score) VALUES ($1,$1 || ':race:mined',1,1)",
        ] {
            sqlx::query(statement)
                .bind(wallet)
                .execute(&admin)
                .await
                .unwrap();
        }
    }

    // Pause B's real add transaction after INSERT but before COMMIT. Without the
    // shared wallet lock, A cannot see B and incorrectly deletes shared history.
    sqlx::raw_sql(sqlx::AssertSqlSafe(format!(
        "CREATE FUNCTION {schema}.pause_new_subscriber() RETURNS trigger LANGUAGE plpgsql AS $$
         BEGIN PERFORM pg_advisory_xact_lock(1263552333, 1); RETURN NEW; END $$;
         CREATE TRIGGER pause_new_subscriber AFTER INSERT ON {schema}.user_wallets
         FOR EACH ROW WHEN (NEW.chat_id = {NEW_CHAT})
         EXECUTE FUNCTION {schema}.pause_new_subscriber()"
    )))
    .execute(&admin)
    .await
    .unwrap();
    let add_pool = scoped_pool("DATABASE_URL", &schema, 1).await;
    let delete_pool = scoped_pool("DATABASE_URL", &schema, 1).await;
    let add_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&add_pool)
        .await
        .unwrap();
    let delete_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&delete_pool)
        .await
        .unwrap();
    let mut gate = admin.begin().await.unwrap();
    let gate_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut *gate)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock(1263552333, 1)")
        .execute(&mut *gate)
        .await
        .unwrap();

    let add_repo = PostgresRepository::new(add_pool.clone());
    let adding = tokio::spawn(async move {
        add_repo
            .add_tracked_wallet(TrackedWallet {
                address: SHARED.to_string(),
                chat_id: NEW_CHAT,
            })
            .await
    });
    let subscriber_paused = waits_for(&admin, add_pid, gate_pid).await;
    let delete_repo = PostgresRepository::new(delete_pool.clone());
    let deleting = tokio::spawn(async move {
        delete_repo
            .remove_all_user_data(OLD_CHAT, OLD_CHAT as u64)
            .await
    });
    let deletion_waited_for_subscriber = waits_for(&admin, delete_pid, add_pid).await;
    // Always open the gate before asserting, including on the unfixed code path.
    gate.commit().await.unwrap();
    let add_result = adding.await.unwrap();
    let delete_result = deleting.await.unwrap();

    let remaining_links: Vec<i64> =
        sqlx::query_scalar("SELECT chat_id FROM user_wallets ORDER BY chat_id")
            .fetch_all(&admin)
            .await
            .unwrap();
    let mut counts = Vec::new();
    for table in [
        "wallet_seen_utxos",
        "wallet_alert_dedup",
        "pending_rewards",
        "mined_blocks",
    ] {
        let shared: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {table} WHERE wallet = $1"
        )))
        .bind(SHARED)
        .fetch_one(&admin)
        .await
        .unwrap();
        let orphan: i64 = sqlx::query_scalar(sqlx::AssertSqlSafe(format!(
            "SELECT COUNT(*) FROM {table} WHERE wallet = $1"
        )))
        .bind(ORPHAN)
        .fetch_one(&admin)
        .await
        .unwrap();
        counts.push((table, shared, orphan));
    }

    // Both operations have settled; clean fault injection before assertions.
    add_pool.close().await;
    delete_pool.close().await;
    sqlx::query(sqlx::AssertSqlSafe(format!("DROP SCHEMA {schema} CASCADE")))
        .execute(&admin)
        .await
        .unwrap();
    admin.close().await;

    assert!(subscriber_paused, "test INSERT must reach the trigger gate");
    add_result.unwrap();
    let summary = delete_result.unwrap();
    assert_eq!(remaining_links, vec![NEW_CHAT]);
    for (table, shared, orphan) in counts {
        assert_eq!(
            shared, 1,
            "concurrent subscriber lost shared state in {table}"
        );
        assert_eq!(orphan, 0, "unshared orphan state survived in {table}");
    }
    assert!(
        deletion_waited_for_subscriber,
        "deletion must wait for concurrent subscription"
    );
    assert_eq!(summary.wallets_deleted, 2);
    assert_eq!(summary.orphan_wallet_addresses, vec![ORPHAN.to_string()]);
}
