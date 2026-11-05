use crate::infrastructure::database::postgres_adapter::PostgresRepository;
use crate::infrastructure::market::coingecko_adapter::MarketProvider;
use chrono::{Duration as ChronoDuration, NaiveDate, Utc};
use std::collections::HashSet;
use std::sync::Arc;
use tracing::{error, info, warn};

pub struct SystemTasksUseCase {
    db: Arc<PostgresRepository>,
    market_provider: Arc<dyn MarketProvider>,
}

impl SystemTasksUseCase {
    pub fn new(db: Arc<PostgresRepository>, market_provider: Arc<dyn MarketProvider>) -> Self {
        Self {
            db,
            market_provider,
        }
    }

    pub async fn execute_memory_cleanup(&self) {
        let is_enabled = self
            .db
            .get_setting("ENABLE_MEMORY_CLEANER", "false")
            .await
            .unwrap_or_else(|_| "false".to_string());

        if is_enabled != "true" {
            return;
        }

        info!("[MEMORY CLEANER] Starting cleanup.");

        if let Err(e) = self.db.run_memory_cleaner().await {
            error!("[DATABASE ERROR] Failed to purge old chat rows: {}", e);
        } else {
            info!("[MEMORY CLEANER] Cleanup complete.");
        }
    }

    pub async fn execute_kas_price_sync(&self) {
        if !env_bool("KAS_PRICE_HISTORY_ENABLED", true) {
            return;
        }

        info!("[KAS PRICE] Starting stored KAS/USD price sync.");

        match self.market_provider.get_kaspa_market_data().await {
            Ok((price_usd, _market_cap)) if price_usd.is_finite() && price_usd > 0.0 => {
                let today = Utc::now().date_naive().format("%Y-%m-%d").to_string();

                if let Err(e) = self
                    .db
                    .upsert_kas_price_usd(&today, price_usd, "coingecko_current")
                    .await
                {
                    error!("[KAS PRICE] Failed to store current KAS/USD price: {}", e);
                }
            }
            Ok((price_usd, _)) => {
                warn!(
                    "[KAS PRICE] Ignoring invalid current KAS/USD price: {}",
                    price_usd
                );
            }
            Err(e) => {
                warn!("[KAS PRICE] Current price fetch failed: {}", e);
            }
        }

        let missing_days = match self
            .db
            .get_missing_kas_price_days_for_mined_blocks(env_i64(
                "KAS_PRICE_BACKFILL_LIMIT_DAYS",
                180,
            ))
            .await
        {
            Ok(days) => days,
            Err(e) => {
                warn!("[KAS PRICE] Failed to find missing mined price days: {}", e);
                return;
            }
        };

        if missing_days.is_empty() {
            info!("[KAS PRICE] No missing mined price days found.");
            return;
        }

        let Some((from_unix, to_unix)) = history_bounds(&missing_days) else {
            warn!("[KAS PRICE] Could not build history range for missing days.");
            return;
        };

        let requested_days: HashSet<String> = missing_days.iter().cloned().collect();

        match self
            .market_provider
            .get_kaspa_usd_history(from_unix, to_unix)
            .await
        {
            Ok(history) => {
                let mut stored = 0usize;

                for (day, price_usd) in history {
                    if !requested_days.contains(&day) {
                        continue;
                    }

                    match self
                        .db
                        .upsert_kas_price_usd(&day, price_usd, "coingecko_history")
                        .await
                    {
                        Ok(()) => stored += 1,
                        Err(e) => warn!("[KAS PRICE] Failed to store price for {}: {}", day, e),
                    }
                }

                info!(
                    "[KAS PRICE] Stored {} historical KAS/USD prices for mined days.",
                    stored
                );
            }
            Err(e) => {
                warn!("[KAS PRICE] Historical price fetch failed: {}", e);
            }
        }
    }
}

fn env_bool(key: &str, default_value: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|value| {
            matches!(
                value.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            )
        })
        .unwrap_or(default_value)
}

fn env_i64(key: &str, default_value: i64) -> i64 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .unwrap_or(default_value)
}

fn history_bounds(days: &[String]) -> Option<(i64, i64)> {
    let mut parsed_days = days
        .iter()
        .filter_map(|day| NaiveDate::parse_from_str(day, "%Y-%m-%d").ok());

    let first = parsed_days.next()?;
    let (min_day, max_day) = parsed_days.fold((first, first), |(min_day, max_day), day| {
        (min_day.min(day), max_day.max(day))
    });

    let from_unix = min_day.and_hms_opt(0, 0, 0)?.and_utc().timestamp();
    let to_day = max_day.checked_add_signed(ChronoDuration::days(1))?;
    let to_unix = to_day.and_hms_opt(0, 0, 0)?.and_utc().timestamp();

    Some((from_unix, to_unix))
}
