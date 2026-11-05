use kaspa_pulse::infrastructure::database::postgres_adapter::PostgresRepository;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;

#[tokio::test]
async fn tracked_wallet_query_returns_only_the_requested_chat() {
    let database_url =
        std::env::var("DATABASE_URL").expect("DATABASE_URL is required for database tests");
    let pool = PgPoolOptions::new()
        .max_connections(5)
        .acquire_timeout(Duration::from_secs(5))
        .connect(&database_url)
        .await
        .expect("test PostgreSQL must be reachable");
    let repository = PostgresRepository::new(pool.clone());

    let chat_a = -9_024_000_101_i64;
    let chat_b = -9_024_000_102_i64;

    sqlx::query("DELETE FROM user_wallets WHERE chat_id = ANY($1)")
        .bind(vec![chat_a, chat_b])
        .execute(&pool)
        .await
        .expect("test rows must be reset");

    for (wallet, chat_id) in [
        ("kaspa:scope-a-1", chat_a),
        ("kaspa:scope-a-2", chat_a),
        ("kaspa:scope-b-1", chat_b),
    ] {
        sqlx::query("INSERT INTO user_wallets (wallet, chat_id) VALUES ($1, $2)")
            .bind(wallet)
            .bind(chat_id)
            .execute(&pool)
            .await
            .expect("test wallet must be inserted");
    }

    let wallets = repository
        .get_tracked_wallets_for_chat(chat_a)
        .await
        .expect("scoped wallet query must succeed");

    let addresses: Vec<_> = wallets.into_iter().map(|wallet| wallet.address).collect();
    assert_eq!(
        addresses,
        vec!["kaspa:scope-a-1".to_string(), "kaspa:scope-a-2".to_string()]
    );

    sqlx::query("DELETE FROM user_wallets WHERE chat_id = ANY($1)")
        .bind(vec![chat_a, chat_b])
        .execute(&pool)
        .await
        .expect("test rows must be cleaned up");
}

#[test]
fn wallet_use_case_does_not_load_all_users_before_filtering() {
    let source = include_str!("../src/wallet/wallet_use_cases.rs");

    assert!(source.contains("get_tracked_wallets_for_chat(chat_id).await?"));
    assert!(!source.contains("get_all_tracked_wallets().await?"));
}
