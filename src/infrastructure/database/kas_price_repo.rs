use crate::domain::errors::AppError;
use crate::infrastructure::database::postgres_adapter::PostgresRepository;
use std::collections::HashMap;

impl PostgresRepository {
    pub async fn upsert_kas_price_usd(
        &self,
        day: &str,
        price_usd: f64,
        source: &str,
    ) -> Result<(), AppError> {
        if !(price_usd.is_finite() && price_usd > 0.0) {
            return Err(AppError::DatabaseError(
                "invalid KAS/USD price for kas_price_history".to_string(),
            ));
        }

        sqlx::query(
            "INSERT INTO kas_price_history (day, price_usd, source, fetched_at, created_at, updated_at)
             VALUES ($1::DATE, $2, $3, NOW(), NOW(), NOW())
             ON CONFLICT (day) DO UPDATE SET
                price_usd = EXCLUDED.price_usd,
                source = EXCLUDED.source,
                fetched_at = EXCLUDED.fetched_at,
                updated_at = NOW()",
        )
        .bind(day)
        .bind(price_usd)
        .bind(source)
        .execute(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(())
    }

    pub async fn get_latest_kas_price_usd(&self) -> Result<Option<f64>, AppError> {
        let price = sqlx::query_scalar::<_, f64>(
            "SELECT price_usd::DOUBLE PRECISION
             FROM kas_price_history
             ORDER BY day DESC
             LIMIT 1",
        )
        .fetch_optional(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(price)
    }

    pub async fn get_kas_price_usd_map_for_days(
        &self,
        days: &[String],
    ) -> Result<HashMap<String, f64>, AppError> {
        if days.is_empty() {
            return Ok(HashMap::new());
        }

        let rows: Vec<(String, f64)> = sqlx::query_as(
            "SELECT TO_CHAR(day, 'YYYY-MM-DD') AS day, price_usd::DOUBLE PRECISION AS price_usd
             FROM kas_price_history
             WHERE TO_CHAR(day, 'YYYY-MM-DD') = ANY($1)",
        )
        .bind(days)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(rows.into_iter().collect())
    }

    pub async fn get_missing_kas_price_days_for_mined_blocks(
        &self,
        limit_days: i64,
    ) -> Result<Vec<String>, AppError> {
        let bounded_days = limit_days.clamp(1, 3650) as i32;

        let days: Vec<String> = sqlx::query_scalar(
            "SELECT TO_CHAR(mined_days.day, 'YYYY-MM-DD')
             FROM (
                SELECT DISTINCT DATE(timestamp) AS day
                FROM mined_blocks
                WHERE timestamp >= CURRENT_DATE - make_interval(days => $1)
             ) mined_days
             LEFT JOIN kas_price_history prices ON prices.day = mined_days.day
             WHERE prices.day IS NULL
             ORDER BY mined_days.day ASC",
        )
        .bind(bounded_days)
        .fetch_all(&self.pool)
        .await
        .map_err(|e| AppError::DatabaseError(e.to_string()))?;

        Ok(days)
    }
}
