use std::sync::Arc;
use dashmap::DashMap;
use std::collections::HashSet;
use sqlx::SqlitePool;
use tokio::sync::RwLock;
use std::sync::atomic::AtomicBool;
use kaspa_rpc_core::api::rpc::RpcApi;

#[allow(dead_code)]
pub struct AppState {
    pub pool: SqlitePool,
    pub state: Arc<DashMap<String, HashSet<i64>>>,
    pub utxo_state: Arc<DashMap<String, HashSet<String>>>,
    pub rpc: Arc<dyn RpcApi>,
    pub price_cache: Arc<RwLock<(f64, f64)>>,
    pub monitoring: Arc<AtomicBool>,
    pub admin_id: i64,
}

pub type AppContext = Arc<AppState>;

