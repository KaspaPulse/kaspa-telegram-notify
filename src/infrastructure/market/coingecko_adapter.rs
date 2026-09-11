use crate::domain::errors::AppError;
use chrono::{TimeZone, Utc};
use reqwest::Client;
use serde_json::Value;
use std::collections::BTreeMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;

// Type aliases to satisfy clippy::type_complexity and improve readability
type MarketData = (f64, f64);
type CachedEntry = Option<(MarketData, Instant)>;
type SharedCache = Arc<RwLock<CachedEntry>>;

pub struct CoinGeckoAdapter {
    client: Client,
    cache: SharedCache,
    circuit_breaker: crate::infrastructure::resilience::circuit_breaker::CircuitBreaker,
}

impl CoinGeckoAdapter {
    pub fn new() -> Result<Self, AppError> {
        Ok(Self {
            client: crate::infrastructure::resilience::runtime::build_http_client()?,
            cache: Arc::new(RwLock::new(None)),
            circuit_breaker:
                crate::infrastructure::resilience::circuit_breaker::CircuitBreaker::new(3, 300), // 3 failures = block for 5 minutes
        })
    }
}

#[async_trait]
impl MarketProvider for CoinGeckoAdapter {
    async fn get_kaspa_market_data(&self) -> Result<(f64, f64), AppError> {
        if let Some((data, timestamp)) = *self.cache.read().await
            && timestamp.elapsed() < Duration::from_secs(60)
        {
            return Ok(data);
        }

        let url = match std::env::var("COINGECKO_API_URL") {
            Ok(value) if !value.trim().is_empty() => value,
            _ => {
                tracing::warn!(
                    "[COINGECKO] COINGECKO_API_URL is missing. Serving stale cache if available."
                );

                if let Some((data, _)) = *self.cache.read().await {
                    return Ok(data);
                }

                self.circuit_breaker.record_failure();
                return Err(AppError::ApiError(
                    "COINGECKO_API_URL is missing and no stale market cache is available"
                        .to_string(),
                ));
            }
        };

        if !self.circuit_breaker.is_allowed() {
            tracing::warn!(
                "⚡ [API BLOCKED] Circuit Breaker is OPEN. Serving stale cache if available..."
            );
            if let Some((data, _)) = *self.cache.read().await {
                return Ok(data);
            } else {
                return Err(AppError::ApiError(
                    "CoinGecko service unavailable while circuit breaker is open".to_string(),
                ));
            }
        }

        let res = self.client.get(&url).send().await.map_err(|e| {
            self.circuit_breaker.record_failure();
            AppError::ApiError(format!("CoinGecko request failed: {e}"))
        })?;

        let status = res.status();
        if !status.is_success() {
            self.circuit_breaker.record_failure();
            return Err(AppError::ApiError(format!(
                "CoinGecko request failed with HTTP status {status}"
            )));
        }

        let json: Value = res.json().await.map_err(|e| {
            self.circuit_breaker.record_failure();
            AppError::ApiError(format!("CoinGecko response was not valid JSON: {e}"))
        })?;

        let data = parse_current_market_data(&json).inspect_err(|_| {
            self.circuit_breaker.record_failure();
        })?;

        self.circuit_breaker.record_success();
        let mut cache_write = self.cache.write().await;
        *cache_write = Some((data, Instant::now()));

        Ok(data)
    }

    async fn get_kaspa_usd_history(
        &self,
        from_unix: i64,
        to_unix: i64,
    ) -> Result<Vec<(String, f64)>, AppError> {
        if from_unix >= to_unix {
            return Ok(Vec::new());
        }

        if !self.circuit_breaker.is_allowed() {
            return Err(AppError::ApiError(
                "CoinGecko service unavailable while circuit breaker is open".to_string(),
            ));
        }

        let url = std::env::var("COINGECKO_MARKET_CHART_RANGE_URL").unwrap_or_else(|_| {
            "https://api.coingecko.com/api/v3/coins/kaspa/market_chart/range".to_string()
        });

        let from = from_unix.to_string();
        let to = to_unix.to_string();

        let res = self
            .client
            .get(&url)
            .query(&[
                ("vs_currency", "usd"),
                ("from", from.as_str()),
                ("to", to.as_str()),
            ])
            .send()
            .await
            .map_err(|e| {
                self.circuit_breaker.record_failure();
                AppError::ApiError(format!("CoinGecko history request failed: {e}"))
            })?;

        let status = res.status();
        if !status.is_success() {
            self.circuit_breaker.record_failure();
            return Err(AppError::ApiError(format!(
                "CoinGecko history request failed with status {status}"
            )));
        }

        let json: Value = res.json().await.map_err(|e| {
            self.circuit_breaker.record_failure();
            AppError::ApiError(format!(
                "CoinGecko history response was not valid JSON: {e}"
            ))
        })?;

        let prices = json
            .get("prices")
            .and_then(|value| value.as_array())
            .ok_or_else(|| {
                self.circuit_breaker.record_failure();
                AppError::ApiError("CoinGecko history response missing prices".to_string())
            })?;

        let mut by_day: BTreeMap<String, f64> = BTreeMap::new();

        for point in prices {
            let Some(values) = point.as_array() else {
                continue;
            };

            if values.len() < 2 {
                continue;
            }

            let Some(timestamp_ms) = values[0].as_f64() else {
                continue;
            };

            let Some(price) = values[1].as_f64() else {
                continue;
            };

            if !(price.is_finite() && price > 0.0) {
                continue;
            }

            if let Some(datetime) = Utc.timestamp_millis_opt(timestamp_ms as i64).single() {
                by_day.insert(datetime.date_naive().format("%Y-%m-%d").to_string(), price);
            }
        }

        if by_day.is_empty() {
            self.circuit_breaker.record_failure();
            return Err(AppError::ApiError(
                "CoinGecko history response did not contain usable daily prices".to_string(),
            ));
        }

        self.circuit_breaker.record_success();

        Ok(by_day.into_iter().collect())
    }
}

fn parse_current_market_data(json: &Value) -> Result<MarketData, AppError> {
    let price = json
        .pointer("/kaspa/usd")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| {
            AppError::ApiError(
                "CoinGecko response missing a finite positive kaspa.usd value".to_string(),
            )
        })?;

    let market_cap = json
        .pointer("/kaspa/usd_market_cap")
        .and_then(Value::as_f64)
        .filter(|value| value.is_finite() && *value > 0.0)
        .ok_or_else(|| {
            AppError::ApiError(
                "CoinGecko response missing a finite positive kaspa.usd_market_cap value"
                    .to_string(),
            )
        })?;

    Ok((price, market_cap))
}

use async_trait::async_trait;

#[async_trait]
pub trait MarketProvider: Send + Sync {
    /// Returns (Price in USD, Market Cap)
    async fn get_kaspa_market_data(&self) -> Result<(f64, f64), AppError>;

    /// Returns stored-history candidates as (YYYY-MM-DD, price_usd).
    async fn get_kaspa_usd_history(
        &self,
        from_unix: i64,
        to_unix: i64,
    ) -> Result<Vec<(String, f64)>, AppError>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn parses_valid_current_market_data() {
        let data = parse_current_market_data(&json!({
            "kaspa": {
                "usd": 0.1234,
                "usd_market_cap": 3_500_000_000.0
            }
        }))
        .expect("valid market response");

        assert_eq!(data, (0.1234, 3_500_000_000.0));
    }

    #[test]
    fn rejects_missing_or_non_positive_current_market_data() {
        assert!(parse_current_market_data(&json!({"kaspa": {}})).is_err());
        assert!(
            parse_current_market_data(&json!({
                "kaspa": {"usd": 0.0, "usd_market_cap": 3_500_000_000.0}
            }))
            .is_err()
        );
        assert!(
            parse_current_market_data(&json!({
                "kaspa": {"usd": 0.1234, "usd_market_cap": -1.0}
            }))
            .is_err()
        );
    }
}
