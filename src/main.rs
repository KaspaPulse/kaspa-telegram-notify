#![allow(clippy::manual_range_contains)]

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

use dashmap::DashMap;
use dotenvy::dotenv;
use kaspa_consensus_core::network::NetworkId;
use kaspa_wrpc_client::{KaspaRpcClient, WrpcEncoding};
use std::collections::{HashMap, HashSet};
use std::env;
use std::str::FromStr;
use std::sync::atomic::AtomicBool;
use std::sync::Arc;
use teloxide::dispatching::{Dispatcher, UpdateFilterExt};
use teloxide::dptree;
use teloxide::prelude::*;
use teloxide::types::Update;
use teloxide::utils::command::BotCommands;
use tokio::fs;
use tokio::sync::RwLock;
use tokio::time::{sleep, Duration};
use tokio_util::sync::CancellationToken;
use tracing::{error, info, warn};
use tracing_subscriber::{
    fmt::writer::MakeWriterExt, layer::SubscriberExt, util::SubscriberInitExt, EnvFilter,
};

use crate::commands::Command;
use crate::context::AppContext;

#[derive(thiserror::Error, Debug)]
pub enum BotError {
    #[error("Environment Variable Missing: {0}")]
    EnvVarMissing(String),
    #[error("Database Initialization Failed: {0}")]
    DatabaseInit(#[from] sqlx::Error),
    #[error("RPC Connection Failed: {0}")]
    RpcConnection(String),
}

#[tokio::main]
async fn main() -> Result<(), BotError> {
    dotenv().ok();

    // Setup logging telemetry
    let file_appender = tracing_appender::rolling::never(".", "bot.log");
    let (non_blocking, _guard) = tracing_appender::non_blocking(file_appender);
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));

    tracing_subscriber::registry()
        .with(console_subscriber::spawn())
        .with(
            tracing_subscriber::fmt::layer()
                .with_writer(non_blocking.and(std::io::stdout))
                .with_ansi(false)
                .with_target(false)
                .with_thread_ids(true),
        )
        .with(filter)
        .init();

    info!("[INIT] Secure Enterprise Rust Engine Started");

    let admin_id_str =
        env::var("ADMIN_ID").map_err(|_| BotError::EnvVarMissing("ADMIN_ID".into()))?;
    let admin_id: i64 = admin_id_str.parse().unwrap_or(0);
    let ws_url = env::var("WS_URL").unwrap_or_else(|_| "ws://127.0.0.1:18110".to_string());
    let network_id = NetworkId::from_str("mainnet")
        .unwrap_or_else(|_| NetworkId::from_str("testnet-12").unwrap());

    let pool = crate::state::init_db().await?;
    let cancel_token = CancellationToken::new();

    // FIX: Graceful Shutdown Registry - Provides a buffer for DB and Worker cleanup
    let pool_shutdown = pool.clone();
    let ct_ctrlc = cancel_token.clone();
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        warn!("[SYSTEM] CRITICAL: SIGINT received. Executing Graceful Database Shutdown...");
        pool_shutdown.close().await;
        info!("[SYSTEM] Database connections closed safely. Cleaning up workers...");
        ct_ctrlc.cancel();
        sleep(Duration::from_secs(2)).await;
        info!("[SYSTEM] Shutdown complete.");
    });

    let state = Arc::new(DashMap::new());
    let memory: crate::context::ContextMemory = Arc::new(DashMap::new());
    let rate_limiter = crate::context::AppContext::new_rate_limiter();

    // Initialize AI RAG Knowledge Base
    crate::rag::init_knowledge_base().await;

    // Migrate old wallets.json if it exists
    if let Ok(data) = fs::read_to_string("wallets.json").await {
        if let Ok(parsed) = serde_json::from_str::<HashMap<String, HashSet<i64>>>(&data) {
            for (k, v) in parsed {
                for chat_id in v {
                    crate::state::add_wallet_to_db(&pool, &k, chat_id).await;
                }
            }
            let _ = fs::rename("wallets.json", "wallets.json.migrated").await;
        }
    }

    // Load active state into RAM
    if let Err(e) = crate::state::load_state_from_db(&pool, &state).await {
        error!("[DB ERROR] Data load failed: {}", e);
    }

    // Establish Kaspa wRPC Tunnel
    let rpc_client = KaspaRpcClient::new(
        WrpcEncoding::SerdeJson,
        Some(&ws_url),
        None,
        Some(network_id),
        None,
    )
    .map_err(|e| BotError::RpcConnection(format!("Tunnel failed: {}", e)))?;

    // The Global Context (Dependency Injection)
    // Initialize Local AI Engine (Loads weights into memory ONCE)
    info!("[INIT] Starting Local AI Engine... This may take a moment to load weights.");
    let ai_engine = Arc::new(std::sync::Mutex::new(
        crate::ai::LocalAiEngine::new()
            .expect("CRITICAL: Failed to load Local AI Engine. Check HuggingFace connection."),
    ));

    let ctx = AppContext {
        rpc: Arc::new(rpc_client),
        pool,
        state,
        utxo_state: Arc::new(DashMap::new()),
        monitoring: Arc::new(AtomicBool::new(true)),
        price_cache: Arc::new(RwLock::new((0.0, 0.0))),
        admin_id,
        memory,
        rate_limiter,
        ai_engine,
    };

    let bot_token =
        env::var("BOT_TOKEN").map_err(|_| BotError::EnvVarMissing("BOT_TOKEN".into()))?;
    let bot = Bot::new(bot_token);

    // Clear old updates to prevent message flooding on restart
    let _ = bot.delete_webhook().drop_pending_updates(true).send().await;

    // Command Menu Setup
    let public_commands = vec![
        teloxide::types::BotCommand::new("start", "Start the bot and show help"),
        teloxide::types::BotCommand::new("help", "Show the ultimate guide and features"),
        teloxide::types::BotCommand::new("add", "Track a wallet"),
        teloxide::types::BotCommand::new("remove", "Stop tracking a wallet"),
        teloxide::types::BotCommand::new("balance", "Check live balance & UTXOs"),
        teloxide::types::BotCommand::new("blocks", "Count unspent mined blocks"),
        teloxide::types::BotCommand::new("miner", "Estimate solo-mining hashrate"),
        teloxide::types::BotCommand::new("network", "View node & network stats"),
    ];
    let _ = bot.set_my_commands(public_commands).await;
    let _ = bot
        .set_my_commands(Command::bot_commands())
        .scope(teloxide::types::BotCommandScope::Chat {
            chat_id: teloxide::types::Recipient::Id(teloxide::types::ChatId(admin_id)),
        })
        .await;

    // Start detached background workers
    crate::workers::start_all(ctx.clone(), bot.clone(), cancel_token);

    // Advanced Routing Engine (dptree)
    let handler = dptree::entry()
        .branch(
            Update::filter_message()
                .filter_command::<crate::commands::Command>()
                .endpoint(handlers::handle_command),
        )
        .branch(Update::filter_callback_query().endpoint(handlers::handle_callback))
        .branch(Update::filter_my_chat_member().endpoint(handlers::handle_block_user))
        .branch(Update::filter_message().endpoint(handlers::handle_raw_message_v2));

    crate::rag::init_knowledge_base().await;
    info!("🚀 Dispatcher is LIVE! Ready for users.");

    Dispatcher::builder(bot, handler)
        .dependencies(dptree::deps![ctx])
        .enable_ctrlc_handler()
        .default_handler(|update: Arc<Update>| async move {
            tracing::debug!(
                "[SYSTEM] Dropped completely unhandled update type: {:?}",
                update.id
            );
        })
        .build()
        .dispatch()
        .await;

    Ok(())
}
