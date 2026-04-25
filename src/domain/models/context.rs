use dashmap::DashMap;
use kaspa_wrpc_client::KaspaRpcClient;
use sqlx::PgPool;
use std::collections::HashSet;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use tokio::sync::RwLock;

#[derive(Clone)]
pub struct RateLimiterStub;
impl RateLimiterStub {
    pub fn retain_recent(&self) {}
}

#[derive(Clone)]
pub struct AppContext {
    pub rpc: Arc<KaspaRpcClient>,
    pub pool: PgPool,
    pub admin_id: i64,
    pub rss_worker_enabled: Arc<AtomicBool>,
    pub memory_cleaner_enabled: Arc<AtomicBool>,
    pub live_sync_enabled: Arc<AtomicBool>,
    pub ai_vectorizer_enabled: Arc<AtomicBool>,
    pub ai_chat_enabled: Arc<AtomicBool>,
    pub ai_voice_enabled: Arc<AtomicBool>,
    pub maintenance_mode: Arc<AtomicBool>,
    #[allow(dead_code)]
    pub webhook_enabled: Arc<AtomicBool>,
    pub state: Arc<DashMap<String, Vec<i64>>>,
    pub utxo_state: Arc<DashMap<String, HashSet<i64>>>,
    pub admin_sessions: Arc<DashMap<i64, String>>,

    pub price_cache: Arc<RwLock<(f64, f64)>>,
    pub rate_limiter: Arc<RateLimiterStub>,
}

impl AppContext {
    pub fn new(rpc: Arc<KaspaRpcClient>, pool: PgPool, admin_id: i64) -> Self {
        Self {
            rpc,
            pool,
            admin_id,
            rss_worker_enabled: Arc::new(AtomicBool::new(false)),
            memory_cleaner_enabled: Arc::new(AtomicBool::new(true)),
            live_sync_enabled: Arc::new(AtomicBool::new(true)),
            ai_vectorizer_enabled: Arc::new(AtomicBool::new(false)),
            ai_chat_enabled: Arc::new(AtomicBool::new(false)),
            ai_voice_enabled: Arc::new(AtomicBool::new(false)),
            maintenance_mode: Arc::new(AtomicBool::new(false)),
            webhook_enabled: Arc::new(AtomicBool::new(false)),
            state: Arc::new(DashMap::new()),
            utxo_state: Arc::new(DashMap::new()),
            admin_sessions: Arc::new(DashMap::new()),

            price_cache: Arc::new(RwLock::new((0.0, 0.0))),
            rate_limiter: Arc::new(RateLimiterStub),
        }
    }
}
