mod ai;
mod commands;
mod context;
mod error;
mod handlers;
mod kaspa_features;
mod rag;
mod state;
mod utils;
mod workers;

use std::sync::Arc;
use dashmap::DashMap;
use teloxide::prelude::*;
use std::sync::atomic::AtomicBool;
use tokio::sync::RwLock;
use kaspa_rpc_core::api::rpc::RpcApi;
use kaspa_wrpc_client::prelude::*;
use crate::context::AppState;



#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenvy::dotenv().ok();
    tracing_subscriber::fmt::init();
    
    let bot_token = std::env::var("BOT_TOKEN").expect("BOT_TOKEN must be set");
    let admin_id = std::env::var("ADMIN_ID").expect("ADMIN_ID must be set").parse::<i64>()?;
    let ws_url = std::env::var("WS_URL").unwrap_or_else(|_| "wss://kaspadns.net/json".to_string());
    
    let pool = state::init_db().await?;
    let state = state::load_state(&pool).await?;
    
    // Fix: Using correct wRPC Client initiation for v1.1.0
    let rpc_client = KaspaRpcClient::new(
        WrpcEncoding::Borsh,
        Some(&ws_url),
        None,
        None,
        None
    )?;
    
    rpc_client.connect(None).await?;
    let rpc: Arc<dyn RpcApi> = Arc::new(rpc_client);


    let ctx = Arc::new(AppState {
        pool,
        state,
        utxo_state: Arc::new(DashMap::new()),
        rpc,
        price_cache: Arc::new(RwLock::new((0.0, 0.0))),
        monitoring: Arc::new(AtomicBool::new(true)),
        admin_id,

    });

    tokio::spawn(workers::start_price_worker(Arc::clone(&ctx)));
    tokio::spawn(workers::start_mining_worker(Arc::clone(&ctx)));
    
    let bot = Bot::new(bot_token);
    let handler = Update::filter_message()
        .branch(dptree::entry().filter_command::<commands::Command>().endpoint(handlers::handle_command))
        .branch(dptree::endpoint(handlers::handle_text_router));

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![ctx])
        .enable_ctrlc_handler()
        .build()
        .dispatch()
        .await;

    Ok(())
}

