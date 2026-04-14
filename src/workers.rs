use std::sync::Arc;
use crate::context::AppContext;
use kaspa_rpc_core::api::rpc::RpcApi;

pub async fn start_price_worker(ctx: AppContext) {
    loop {
        if let Ok(response) = reqwest::get("https://api.coingecko.com/api/v3/simple/price?ids=kaspa&vs_currencies=usd&include_market_cap=true").await {
            if let Ok(json) = response.json::<serde_json::Value>().await {
                if let (Some(p), Some(m)) = (json["kaspa"]["usd"].as_f64(), json["kaspa"]["usd_market_cap"].as_f64()) {
                    let mut cache = ctx.price_cache.write().await;
                    *cache = (p, m);
                }
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
    }
}

pub async fn start_mining_worker(_ctx: AppContext) {
    tracing::info!("[WORKER] Mining monitor idle.");
    loop {
        tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
    }
}

pub async fn _analyze_block_payload(_rpc: &Arc<dyn RpcApi>) {
    // Reserved for future block analysis
}
