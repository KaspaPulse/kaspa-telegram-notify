use crate::ai::SharedAiEngine;
use crate::state::{SharedState, UtxoState};
use governor::{clock::DefaultClock, state::keyed::DefaultKeyedStateStore, Quota, RateLimiter};
use kaspa_wrpc_client::KaspaRpcClient;
use sqlx::postgres::PgPool;
use std::num::NonZeroU32;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::RwLock;

pub type PriceCache = Arc<RwLock<(f64, f64)>>;
pub type SpamLimiter = Arc<RateLimiter<i64, DefaultKeyedStateStore<i64>, DefaultClock>>;

#[derive(Clone)]
pub struct AppContext {
    pub rpc: Arc<KaspaRpcClient>,
    pub pool: PgPool,             // 🐘 Moved to PostgreSQL
    pub state: SharedState,
    pub utxo_state: UtxoState,
    pub monitoring: Arc<AtomicBool>,
    pub price_cache: PriceCache,
    pub admin_id: i64,
    pub rate_limiter: SpamLimiter,
    pub ai_engine: SharedAiEngine,
}

impl AppContext {
    pub fn new_rate_limiter() -> SpamLimiter {
        Arc::new(RateLimiter::keyed(
            Quota::per_second(NonZeroU32::new(2).unwrap()).allow_burst(NonZeroU32::new(3).unwrap()), // FIXME_PHASE3: DANGER! Bot will crash here if it fails. Use '?' or 'safe_unwrap!'
        ))
    }
}

