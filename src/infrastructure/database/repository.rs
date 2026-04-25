use sqlx::PgPool;

pub struct MiningRepository;

#[derive(Debug)]
pub struct WalletAnalytics {
    pub count: i64,
    pub sum: i64,
}

impl MiningRepository {
    pub async fn get_wallet_analytics(
        pool: &PgPool,
        wallet: &str,
    ) -> Result<WalletAnalytics, crate::domain::errors::BotError> {
        // The query string MUST exactly match the hash in .sqlx cache (including the $1)
        let record = sqlx::query!(
            r#"SELECT COUNT(*) as "count!", (COALESCE(SUM(amount), 0))::BIGINT as "sum!" FROM mined_blocks WHERE wallet = $1"#,
            wallet
        )
        .fetch_one(pool)
        .await?;

        Ok(WalletAnalytics {
            count: record.count,
            sum: record.sum,
        })
    }
}
