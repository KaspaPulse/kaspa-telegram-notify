//! OP003: one shared wallet, two languages, duplicate-free transactional outbox.
//! Run only against the exact disposable PostgreSQL instance started by OP003.
//! NO Telegram, PNN or production database connection is performed.
use kaspa_pulse::domain::models::LiveBlockEvent;
use kaspa_pulse::infrastructure::telegram_delivery_queue::{
    AlertOutboxOutcome, AlertOutboxRequest, commit_alert_outbox_localized,
};
use kaspa_pulse::presentation::telegram::formatting::events_formatter::format_live_event_lang;
use kaspa_pulse::presentation::telegram::menus::Language;
use sqlx::{PgPool, postgres::PgPoolOptions};
use std::time::Duration;

const EN: &str = "⚡ <b>Kaspa Mining Reward!</b>";
const AR: &str = "⚡ <b>مكافأة تعدين كاسبا!</b>";

fn render_example(lang: Language, price_usd: Option<f64>) -> String {
    let event = LiveBlockEvent {
        is_coinbase: true,
        wallet_address: "kaspa:op003-shared-test-wallet".into(),
        amount_kas: 1.23,
        live_balance_kas: 12.34,
        source_outpoint: "op003-tx:0".into(),
        alert_key: "op003-mined-hash".into(),
        tx_id: "c".repeat(64),
        block_time_ms: 1793708190541,
        acc_block_hash: "a".repeat(64),
        mined_block_hash: Some("b".repeat(64)),
        extracted_worker: Some("miner<worker>".into()),
        daa_score: 123,
    };
    format_live_event_lang(&event, price_usd, lang)
}

fn request<'a>(
    wallet: &'a str,
    outpoint: &'a str,
    alert_key: &'a str,
    message_html: &'a str,
    chat_ids: &'a [i64],
) -> AlertOutboxRequest<'a> {
    AlertOutboxRequest {
        wallet,
        source_outpoint: outpoint,
        alert_key,
        message_html,
        chat_ids,
        txid_masked: None,
        block_hash_masked: None,
        amount_kas: Some(1.23),
        daa_score: Some(123),
    }
}

fn source_contract() -> &'static str {
    include_str!("../src/main.rs")
}

#[test]
fn english_native_menu_has_no_automatic_arabic_scope_or_new_auto_locale() {
    let main = source_contract();
    assert!(main.contains("BotCommandScope::AllPrivateChats"));
    assert!(main.contains("Telegram stale language-scoped command deletion failed"));
    assert!(main.contains("delete_my_commands()"));
    assert!(main.contains(".language_code(lang.to_string())"));
    assert!(!main.contains("public_bot_commands_ar()"));
    assert!(!main.contains("admin_bot_commands_ar()"));
}

async fn pool_with_strict_isolation() -> PgPool {
    let raw = std::env::var("LANGUAGE_TEST_DATABASE_URL")
        .expect("Only the disposable OP003 test database is permitted");
    let url = reqwest::Url::parse(&raw).expect("Postgres URL");
    assert_eq!(url.host_str(), Some("127.0.0.1"));
    assert_eq!(url.port(), Some(55444));
    assert_eq!(url.path(), "/language_op002");
    assert_eq!(url.username(), "langtest");
    PgPoolOptions::new()
        .max_connections(3)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&raw)
        .await
        .expect("isolated database only")
}

async fn fixture(pool: &PgPool) {
    sqlx::raw_sql(
        r#"
        CREATE TABLE IF NOT EXISTS user_wallets (
          wallet TEXT NOT NULL, chat_id BIGINT NOT NULL,
          PRIMARY KEY(wallet,chat_id)
        );
        CREATE TABLE IF NOT EXISTS telegram_user_language (
          user_id BIGINT PRIMARY KEY,
          language TEXT NOT NULL DEFAULT 'en' CHECK(language IN ('en','ar'))
        );
        CREATE TABLE IF NOT EXISTS system_settings (
          key_name TEXT PRIMARY KEY,
          value_data TEXT NOT NULL,
          updated_at TIMESTAMPTZ NOT NULL DEFAULT now()
        );
        CREATE TABLE IF NOT EXISTS wallet_alert_dedup (
          wallet TEXT NOT NULL, alert_key TEXT NOT NULL,
          txid_masked TEXT, block_hash_masked TEXT,
          PRIMARY KEY(wallet,alert_key)
        );
        CREATE TABLE IF NOT EXISTS wallet_seen_utxos (
          wallet TEXT NOT NULL,outpoint TEXT NOT NULL,
          first_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
          last_seen_at TIMESTAMPTZ NOT NULL DEFAULT now(),
          PRIMARY KEY(wallet,outpoint)
        );
        CREATE TABLE IF NOT EXISTS telegram_delivery_queue (
          id BIGSERIAL PRIMARY KEY, chat_id BIGINT NOT NULL,
          message_html TEXT NOT NULL, status TEXT NOT NULL,
          wallet_masked TEXT, txid_masked TEXT, block_hash_masked TEXT,
          amount_kas DOUBLE PRECISION, daa_score BIGINT,
          next_attempt_at TIMESTAMPTZ,
          event_key TEXT,
          created_at TIMESTAMPTZ NOT NULL DEFAULT now()
        );
        CREATE UNIQUE INDEX IF NOT EXISTS ux_language_op003_chat_event
        ON telegram_delivery_queue(chat_id,event_key)
        WHERE event_key IS NOT NULL;
        "#,
    )
    .execute(pool)
    .await
    .expect("create isolated fixture");
    sqlx::query("TRUNCATE telegram_delivery_queue, wallet_seen_utxos, wallet_alert_dedup, user_wallets, telegram_user_language")
        .execute(pool)
        .await
        .expect("clear dedicated disposable fixture");
}

#[tokio::test]
#[ignore = "Requires disposable 127.0.0.1:55444/language_op002 PostgreSQL only"]
async fn two_languages_same_wallet_exactly_once_and_retry_immutable() {
    let pool = pool_with_strict_isolation().await;
    fixture(&pool).await;

    const WALLET: &str = "kaspa:op003-shared-test-wallet";
    let en_user: i64 = 8_100_001;
    let ar_user: i64 = 8_100_002;
    let ids = [en_user, ar_user, en_user];
    let en_html = render_example(Language::En, Some(0.05));
    let ar_html = render_example(Language::Ar, Some(0.05));
    assert!(render_example(Language::En, None).contains("USD unavailable"));
    assert!(render_example(Language::Ar, None).contains("الدولار غير متاح"));
    assert!(en_html.contains(EN));
    assert!(ar_html.contains(AR));
    assert!(en_html.contains("&lt;worker&gt;"));
    assert!(ar_html.contains("&lt;worker&gt;"));
    assert!(!en_html.contains("مكافأة تعدين"));
    assert!(!ar_html.contains("Kaspa Mining Reward!"));
    for chat_id in [en_user, ar_user] {
        sqlx::query("INSERT INTO user_wallets(wallet,chat_id) VALUES($1,$2)")
            .bind(WALLET)
            .bind(chat_id)
            .execute(&pool)
            .await
            .unwrap();
    }
    // The English subscriber has no row. A non-English Telegram UI is irrelevant.
    sqlx::query("INSERT INTO telegram_user_language(user_id,language) VALUES($1,'ar')")
        .bind(ar_user)
        .execute(&pool)
        .await
        .unwrap();

    let outcome = commit_alert_outbox_localized(
        &pool,
        request(WALLET, "op003-tx:0", "op003-mined-hash", &en_html, &ids),
        Some(&ar_html),
    )
    .await
    .unwrap();
    assert_eq!(outcome, AlertOutboxOutcome::Enqueued { recipients: 2 });

    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT chat_id,message_html FROM telegram_delivery_queue
         WHERE event_key='op003-tx:0' ORDER BY chat_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        rows,
        vec![(en_user, en_html.clone()), (ar_user, ar_html.clone())]
    );

    let dedup: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM wallet_alert_dedup WHERE wallet=$1")
        .bind(WALLET)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(dedup, 1);

    // Explicit language switch does NOT re-encode already committed messages.
    sqlx::query("UPDATE telegram_user_language SET language='en' WHERE user_id=$1")
        .bind(ar_user)
        .execute(&pool)
        .await
        .unwrap();

    let retried = commit_alert_outbox_localized(
        &pool,
        request(WALLET, "op003-tx:0", "op003-mined-hash", "changed-en", &ids),
        Some("changed-ar"),
    )
    .await
    .unwrap();
    assert_eq!(retried, AlertOutboxOutcome::Duplicate);
    let unchanged: Vec<(i64, String)> = sqlx::query_as(
        "SELECT chat_id,message_html FROM telegram_delivery_queue
         WHERE event_key='op003-tx:0' ORDER BY chat_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(unchanged, rows);

    // Subsequent event observes the user's explicit switch back to English.
    let next = commit_alert_outbox_localized(
        &pool,
        request(WALLET, "op003-tx:1", "op003-another-block", &en_html, &ids),
        Some(&ar_html),
    )
    .await
    .unwrap();
    assert_eq!(next, AlertOutboxOutcome::Enqueued { recipients: 2 });
    let second: Vec<(i64, String)> = sqlx::query_as(
        "SELECT chat_id,message_html FROM telegram_delivery_queue
         WHERE event_key='op003-tx:1' ORDER BY chat_id",
    )
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(second, vec![(en_user, en_html.clone()), (ar_user, en_html)]);

    let (queue_count, seen_count, dedup_count): (i64, i64, i64) = sqlx::query_as(
        "SELECT
             (SELECT COUNT(*) FROM telegram_delivery_queue),
             (SELECT COUNT(*) FROM wallet_seen_utxos),
             (SELECT COUNT(*) FROM wallet_alert_dedup)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!((queue_count, seen_count, dedup_count), (4, 2, 2));
    pool.close().await;
}
