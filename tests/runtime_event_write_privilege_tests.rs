use kaspa_pulse::domain::models::{BotEventRecord, BotEventType, EventSeverity};
use kaspa_pulse::infrastructure::database::postgres_adapter::PostgresRepository;
use sqlx::postgres::PgPoolOptions;

#[tokio::test]
async fn f11_runtime_role_persists_startup_events_with_generated_ids() {
    let url = std::env::var("DATABASE_URL").expect("DATABASE_URL is required");
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect(&url)
        .await
        .unwrap();
    let role: String = sqlx::query_scalar("SELECT current_user")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(
        role, "kaspa_pulse_app",
        "this gate must exercise the actual runtime role"
    );

    const MARKER: &str = "f11-runtime-event-write";
    sqlx::query("DELETE FROM bot_event_log WHERE metadata->>'regression' = $1")
        .bind(MARKER)
        .execute(&pool)
        .await
        .unwrap();
    let repo = PostgresRepository::new(pool.clone());
    let mut writes = Vec::new();
    for event_type in [BotEventType::SystemStart, BotEventType::WebhookStart] {
        let mut record = BotEventRecord::new(event_type, EventSeverity::Info);
        record.status = Some("started");
        record.metadata_json = r#"{"regression":"f11-runtime-event-write"}"#;
        // These are the same repository calls used by process/webhook startup.
        writes.push(repo.record_bot_event_record(record).await);
    }
    let rows: Vec<(i64, String)> = sqlx::query_as(
        "SELECT id, event_type FROM bot_event_log
         WHERE metadata->>'regression' = $1 AND severity = 'info' AND status = 'started'
         ORDER BY id",
    )
    .bind(MARKER)
    .fetch_all(&pool)
    .await
    .unwrap();

    // Remove only this test's records before checking the outcome, including the
    // pre-migration failure path. No role grants or schema changes occur here.
    sqlx::query("DELETE FROM bot_event_log WHERE metadata->>'regression' = $1")
        .bind(MARKER)
        .execute(&pool)
        .await
        .unwrap();
    pool.close().await;
    for result in writes {
        result.expect("runtime role must persist startup events after migrations");
    }
    assert_eq!(rows.len(), 2);
    assert!(
        rows[0].0 > 0 && rows[1].0 > rows[0].0,
        "database must allocate distinct sequence IDs"
    );
    assert_eq!(rows[0].1, "SYSTEM_START");
    assert_eq!(rows[1].1, "WEBHOOK_START");
}
