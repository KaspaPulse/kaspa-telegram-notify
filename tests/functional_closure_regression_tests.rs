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

#[tokio::test]
async fn f03_admin_diagnostics_fail_closed_on_schema_query_failure() {
    let _guard = db_lock().lock().await;
    let admin = admin_pool().await;
    sqlx::query("DROP SCHEMA IF EXISTS f03_fault CASCADE")
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("CREATE SCHEMA f03_fault")
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("CREATE TABLE f03_fault.user_wallets (wallet TEXT, chat_id BIGINT)")
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("CREATE TABLE f03_fault.system_settings (key_name TEXT, value_data TEXT)")
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("GRANT USAGE ON SCHEMA f03_fault TO kaspa_pulse_app")
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("GRANT SELECT ON ALL TABLES IN SCHEMA f03_fault TO kaspa_pulse_app")
        .execute(&admin)
        .await
        .unwrap();

    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL is required");
    let fault_pool = PgPoolOptions::new()
        .max_connections(2)
        .after_connect(move |conn, _meta| {
            Box::pin(async move {
                sqlx::query("SET search_path TO f03_fault")
                    .execute(conn)
                    .await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();

    let snapshot = kaspa_pulse::infrastructure::admin_diagnostics::collect(&fault_pool).await;
    assert!(snapshot.connection_ok());
    assert!(!snapshot.required_queries_ok());
    assert_eq!(snapshot.database_status(), "DEGRADED");
    assert!(snapshot.users_count.is_ok());
    assert!(snapshot.wallets_count.is_ok());
    assert!(snapshot.settings_count.is_ok());
    assert!(snapshot.mined_count.is_err());
    assert!(snapshot.last_alert.is_err());
    assert_eq!(
        kaspa_pulse::infrastructure::admin_diagnostics::display_count(&snapshot.mined_count),
        "UNAVAILABLE"
    );
    assert_eq!(
        kaspa_pulse::infrastructure::admin_diagnostics::display_last_alert(&snapshot.last_alert),
        "UNAVAILABLE"
    );

    fault_pool.close().await;
    sqlx::query("DROP SCHEMA f03_fault CASCADE")
        .execute(&admin)
        .await
        .unwrap();
}

#[test]
fn f06_logs_use_bounded_production_tracing_buffer_instead_of_missing_files() {
    let admin = include_str!("../src/presentation/telegram/handlers/admin.rs");
    let main = include_str!("../src/main.rs");
    let logs = include_str!("../src/infrastructure/recent_logs.rs");

    assert!(main.contains("recent_logs::layer()"));
    assert!(admin.contains("recent_logs::recent_lines(25)"));
    assert!(!admin.contains("bot.log"));
    assert!(!admin.contains("logs/bot.log"));
    assert!(!admin.contains("target/debug/bot.log"));
    assert!(logs.contains("VecDeque<String>"));
    assert!(logs.contains("sanitize_for_log"));
    assert!(logs.contains("while guard.len() > capacity"));
}

#[test]
fn f04_broadcast_is_not_advertised_or_partially_implemented() {
    let commands = include_str!("../src/presentation/telegram/commands.rs");
    let handlers = include_str!("../src/presentation/telegram/handlers/mod.rs");
    let admin = include_str!("../src/presentation/telegram/handlers/admin.rs");

    assert!(!commands.contains("Broadcast("));
    assert!(!commands.contains("BotCommand::new(\"broadcast\""));
    assert!(!handlers.contains("Command::Broadcast"));
    assert!(!admin.contains("handle_broadcast"));
    assert!(!admin.contains("Broadcast prepared"));
}

#[test]
fn f05_restart_is_explicitly_information_only() {
    let commands = include_str!("../src/presentation/telegram/commands.rs");
    let handlers = include_str!("../src/presentation/telegram/handlers/mod.rs");
    let menu = include_str!("../src/presentation/telegram/menus.rs");
    let admin = include_str!("../src/presentation/telegram/handlers/admin.rs");
    let security = include_str!("../src/domain/models/telegram_security.rs");

    assert!(commands.contains("rename = \"restart_info\""));
    assert!(commands.contains("show restart instructions"));
    assert!(!commands.contains("BotCommand::new(\"restart\""));
    assert!(menu.contains("Restart Info"));
    assert!(menu.contains("cmd_restart_info"));
    assert!(handlers.contains("Command::RestartInfo"));
    assert!(handlers.contains("handle_restart_info"));
    assert!(admin.contains("does not restart its own process"));
    assert!(!security.contains("Restart,"));
    assert!(!handlers.contains("SensitiveAction::Restart"));
}

#[tokio::test]
async fn f09_startup_persisted_settings_default_only_when_missing_and_fail_on_db_error() {
    let _guard = db_lock().lock().await;
    let admin = admin_pool().await;
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL is required");

    sqlx::query("DROP SCHEMA IF EXISTS f09_defaults CASCADE")
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("CREATE SCHEMA f09_defaults")
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("CREATE TABLE f09_defaults.system_settings (key_name TEXT PRIMARY KEY, value_data TEXT NOT NULL, updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW())")
        .execute(&admin).await.unwrap();
    sqlx::query("GRANT USAGE ON SCHEMA f09_defaults TO kaspa_pulse_app")
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("GRANT SELECT, INSERT, UPDATE ON f09_defaults.system_settings TO kaspa_pulse_app")
        .execute(&admin)
        .await
        .unwrap();
    let default_pool = PgPoolOptions::new()
        .max_connections(2)
        .after_connect(|conn, _| {
            Box::pin(async move {
                sqlx::query("SET search_path TO f09_defaults")
                    .execute(conn)
                    .await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    let default_repo = PostgresRepository::new(default_pool.clone());
    let defaults = default_repo
        .load_persisted_runtime_settings()
        .await
        .unwrap();
    assert!(!defaults.memory_cleaner_enabled);
    assert!(defaults.live_sync_enabled);
    assert!(!defaults.maintenance_mode);
    let inserted: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM f09_defaults.system_settings")
        .fetch_one(&admin)
        .await
        .unwrap();
    assert_eq!(inserted, 3);
    default_pool.close().await;

    sqlx::query("DROP SCHEMA IF EXISTS f09_fault CASCADE")
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("CREATE SCHEMA f09_fault")
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("GRANT USAGE ON SCHEMA f09_fault TO kaspa_pulse_app")
        .execute(&admin)
        .await
        .unwrap();
    let fault_pool = PgPoolOptions::new()
        .max_connections(2)
        .after_connect(|conn, _| {
            Box::pin(async move {
                sqlx::query("SET search_path TO f09_fault")
                    .execute(conn)
                    .await?;
                Ok(())
            })
        })
        .connect(&url)
        .await
        .unwrap();
    let fault_repo = PostgresRepository::new(fault_pool.clone());
    let error = fault_repo
        .load_persisted_runtime_settings()
        .await
        .expect_err("missing settings schema must fail startup settings load");
    assert!(matches!(
        error,
        kaspa_pulse::domain::errors::AppError::DatabaseError(_)
    ));
    fault_pool.close().await;

    sqlx::query("DROP SCHEMA f09_defaults CASCADE")
        .execute(&admin)
        .await
        .unwrap();
    sqlx::query("DROP SCHEMA f09_fault CASCADE")
        .execute(&admin)
        .await
        .unwrap();

    let main = include_str!("../src/main.rs");
    assert!(main.contains("load_persisted_runtime_settings()"));
    assert!(main.contains("Failed to load required persisted runtime settings"));
    assert!(!main.contains(".get_setting(\"MAINTENANCE_MODE\", \"false\")\n            .await\n            .unwrap_or_else"));
}

#[test]
fn f08_help_registered_commands_and_admin_buttons_are_consistent() {
    use kaspa_pulse::presentation::telegram::commands::admin_bot_commands;

    let handlers = include_str!("../src/presentation/telegram/handlers/mod.rs");
    let menus = include_str!("../src/presentation/telegram/menus.rs");
    for command in admin_bot_commands() {
        let documented = format!("/{}", command.command);
        assert!(
            handlers.contains(&documented),
            "registered command {documented} is missing from /help"
        );
    }

    for real_button in [
        "Health",
        "System",
        "Stats",
        "Settings",
        "Pause",
        "Resume",
        "Restart Info",
        "DB Diagnostics",
        "Logs",
        "Events",
        "Errors",
        "Delivery",
        "Cleanup Events",
        "Stop Alerts",
        "Resume Alerts",
        "Alert Status",
    ] {
        assert!(
            menus.contains(real_button),
            "admin menu missing {real_button}"
        );
        assert!(
            handlers.contains(&format!("<b>{real_button}</b>")),
            "help missing {real_button}"
        );
    }
    assert!(!handlers.contains("<b>Subscribers</b> -"));
    assert!(!handlers.contains("<b>Wallet Events</b> -"));
    assert!(handlers.contains("<b>Market</b> - KAS market information."));
    assert!(!handlers.contains("<b>Price</b> - KAS price and market info."));
}

#[test]
fn f10_unknown_commands_and_maintenance_messages_are_never_silent() {
    use kaspa_pulse::presentation::telegram::handlers::raw_message::{
        MAINTENANCE_MESSAGE, UNKNOWN_COMMAND_MESSAGE, is_unknown_command_text,
    };

    assert!(is_unknown_command_text("/does_not_exist"));
    assert!(is_unknown_command_text("  /does_not_exist arg"));
    assert!(!is_unknown_command_text("kaspa:qexample"));
    assert!(UNKNOWN_COMMAND_MESSAGE.contains("/help"));
    assert!(MAINTENANCE_MESSAGE.contains("Maintenance Mode"));

    let raw = include_str!("../src/presentation/telegram/handlers/raw_message.rs");
    assert!(raw.contains("send_logged!(bot, msg, MAINTENANCE_MESSAGE)"));
    assert!(raw.contains("send_logged!(bot, msg, UNKNOWN_COMMAND_MESSAGE)"));
}
