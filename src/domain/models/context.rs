use super::telegram_security::{ActorChatKey, ConfirmationSession, PendingInputSession};
use dashmap::DashMap;
use kaspa_wrpc_client::KaspaRpcClient;
use sqlx::PgPool;
use std::collections::HashSet;
use std::sync::Arc;
use std::sync::atomic::AtomicBool;
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
    pub admin_user_id: u64,
    pub admin_chat_id: i64,
    pub memory_cleaner_enabled: Arc<AtomicBool>,
    pub live_sync_enabled: Arc<AtomicBool>,
    pub maintenance_mode: Arc<AtomicBool>,
    #[allow(dead_code)]
    pub webhook_enabled: Arc<AtomicBool>,
    pub state: Arc<DashMap<String, Vec<i64>>>,
    pub utxo_state: Arc<DashMap<String, HashSet<i64>>>,
    pub admin_confirmations: Arc<DashMap<String, ConfirmationSession>>,
    pub pending_input_sessions: Arc<DashMap<ActorChatKey, PendingInputSession>>,
    pub price_cache: Arc<RwLock<(f64, f64)>>,
    pub rate_limiter: Arc<RateLimiterStub>,
}

impl AppContext {
    pub fn new(
        rpc: Arc<KaspaRpcClient>,
        pool: PgPool,
        admin_user_id: u64,
        admin_chat_id: i64,
    ) -> Self {
        Self {
            rpc,
            pool,
            admin_user_id,
            admin_chat_id,
            memory_cleaner_enabled: Arc::new(AtomicBool::new(true)),
            live_sync_enabled: Arc::new(AtomicBool::new(true)),
            maintenance_mode: Arc::new(AtomicBool::new(false)),
            webhook_enabled: Arc::new(AtomicBool::new(false)),
            state: Arc::new(DashMap::new()),
            utxo_state: Arc::new(DashMap::new()),
            admin_confirmations: Arc::new(DashMap::new()),
            pending_input_sessions: Arc::new(DashMap::new()),
            price_cache: Arc::new(RwLock::new((0.0, 0.0))),
            rate_limiter: Arc::new(RateLimiterStub),
        }
    }
}
