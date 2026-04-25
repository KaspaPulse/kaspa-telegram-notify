use crate::domain::errors::AppError;
use crate::infrastructure::database::postgres_adapter::PostgresRepository;
use crate::infrastructure::market::coingecko_adapter::MarketProvider;
use crate::infrastructure::node::kaspa_adapter::KaspaRpcAdapter;
// ===== Migrated from network_stats.rs =====
use std::sync::Arc;

pub struct NetworkStatsUseCase {
    node: Arc<KaspaRpcAdapter>,
}

impl NetworkStatsUseCase {
    pub fn new(node: Arc<KaspaRpcAdapter>) -> Self {
        Self { node }
    }

    pub async fn get_network_overview(&self) -> Result<(bool, usize, f64), AppError> {
        let (is_online, peers) = self.node.get_node_health().await?;
        let hashrate = self.node.get_network_hashrate().await?;
        Ok((is_online, peers, hashrate))
    }
}

// ===== Migrated from market_stats.rs =====

pub struct MarketStatsResult {
    pub price: f64,
    pub mcap: f64,
    pub hashrate: f64,
    pub is_online: bool,
    pub peers: usize,
    pub pruning_point: String,
}

pub struct GetMarketStatsUseCase {
    node: Arc<KaspaRpcAdapter>,
    market: Arc<dyn MarketProvider>,
}

impl GetMarketStatsUseCase {
    pub fn new(node: Arc<KaspaRpcAdapter>, market: Arc<dyn MarketProvider>) -> Self {
        Self { node, market }
    }

    pub async fn execute(&self) -> Result<MarketStatsResult, AppError> {
        let (price, mcap) = self
            .market
            .get_kaspa_market_data()
            .await
            .unwrap_or((0.0, 0.0));
        let hashrate = self.node.get_network_hashrate().await.unwrap_or(0.0);
        let (is_online, peers) = self.node.get_node_health().await.unwrap_or((false, 0));
        let pruning_point = self
            .node
            .get_pruning_point()
            .await
            .unwrap_or_else(|_| "Unknown".to_string());

        Ok(MarketStatsResult {
            price,
            mcap,
            hashrate,
            is_online,
            peers,
            pruning_point,
        })
    }
}

// ===== Migrated from get_miner_stats.rs =====

pub struct MinerStatsResult {
    pub wallet_address: String,
    pub actual_hashrate_1h: String,
    pub actual_hashrate_24h: String,
    pub unspent_hashrate_1h: String,
    pub unspent_hashrate_24h: String,
    pub global_network_hashrate: String,
}
pub struct GetMinerStatsUseCase {
    db: Arc<PostgresRepository>,
    node: Arc<KaspaRpcAdapter>,
}

impl GetMinerStatsUseCase {
    pub fn new(db: Arc<PostgresRepository>, node: Arc<KaspaRpcAdapter>) -> Self {
        Self { db, node }
    }

    pub async fn execute(&self, wallet_address: &str) -> Result<MinerStatsResult, AppError> {
        let net_hashrate = self.node.get_network_hashrate().await?;
        let virtual_daa = self.node.get_virtual_daa_score().await?;

        let db_1h = self
            .db
            .get_blocks_count_1h(wallet_address)
            .await
            .unwrap_or(0);
        let db_24h = self
            .db
            .get_blocks_count_24h(wallet_address)
            .await
            .unwrap_or(0);

        let utxos = self
            .node
            .get_utxos(wallet_address)
            .await
            .unwrap_or_default();
        let mut live_1h = 0;
        let mut live_24h = 0;

        for u in utxos {
            if u.is_coinbase {
                let age = virtual_daa.saturating_sub(u.block_daa_score);
                if age <= 3600 {
                    live_1h += 1;
                }
                if age <= 86400 {
                    live_24h += 1;
                }
            }
        }

        let actual_1h_rate = net_hashrate * (db_1h as f64 / 3600.0);
        let actual_24h_rate = net_hashrate * (db_24h as f64 / 86400.0);
        let unspent_1h_rate = net_hashrate * (live_1h as f64 / 3600.0);
        let unspent_24h_rate = net_hashrate * (live_24h as f64 / 86400.0);

        Ok(MinerStatsResult {
            wallet_address: wallet_address.to_string(),
            actual_hashrate_1h: format!("{:.2} GH/s", actual_1h_rate / 1e9),
            actual_hashrate_24h: format!("{:.2} GH/s", actual_24h_rate / 1e9),
            unspent_hashrate_1h: format!("{:.2} GH/s", unspent_1h_rate / 1e9),
            unspent_hashrate_24h: format!("{:.2} GH/s", unspent_24h_rate / 1e9),
            global_network_hashrate: format!("{:.2} TH/s", net_hashrate / 1e12),
        })
    }
}
