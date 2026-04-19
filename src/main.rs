pub mod resilience;
pub mod enterprise;
#[allow(clippy::manual_range_contains)]

pub mod agent;
mod ai;
mod commands;
mod context;
mod handlers;
pub mod services;
mod kaspa_features;
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
        .unwrap_or_else(|_| NetworkId::from_str("testnet-12").unwrap()); // FIXME_PHASE3: DANGER! Bot will crash here if it fails. Use '?' or 'safe_unwrap!'

    let db_url = env::var("DATABASE_URL").expect("CRITICAL: DATABASE_URL is missing in .env"); // FIXME_PHASE3: DANGER! Bot will crash here if it fails. Use '?' or 'safe_unwrap!'
    let pool = crate::state::init_db(&db_url).await?;

    // Redis removed: State is now natively managed via DashMap for ultra-low latency.
    let cancel_token = CancellationToken::new();

    // Graceful Shutdown Registry
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
        std::process::exit(0);
    });

    let state = Arc::new(DashMap::new());
    let rate_limiter = crate::context::AppContext::new_rate_limiter();

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
    let rpc_client = KaspaRpcClient::new( // FIXME_PHASE4_RETRY: Wrap this in 'crate::resilience::with_retries(|| async { ... }, 5).await'
        WrpcEncoding::SerdeJson,
        Some(&ws_url),
        None,
        Some(network_id),
        None,
    )
    .map_err(|e| BotError::RpcConnection(format!("Tunnel failed: {}", e)))?;

    // Initialize Cloud AI Engine (Instant Boot)
    info!("[INIT] Starting Cloud AI Engine... (Instant Boot)");
    let ai_engine = Arc::new(
        crate::ai::LocalAiEngine::new()
            .expect("CRITICAL: Failed to load Cloud AI Engine. Check API Key."), // FIXME_PHASE3: DANGER! Bot will crash here if it fails. Use '?' or 'safe_unwrap!'
    );

    let ctx = AppContext {
        rpc: Arc::new(rpc_client),
        pool,
        state,
        utxo_state: Arc::new(DashMap::new()),
        monitoring: Arc::new(AtomicBool::new(true)),
        price_cache: Arc::new(RwLock::new((0.0, 0.0))),
        admin_id,
        rate_limiter,
        ai_engine,
    };

    let bot_token =
        env::var("BOT_TOKEN").map_err(|_| BotError::EnvVarMissing("BOT_TOKEN".into()))?;
    let bot = Bot::new(bot_token);

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
    if let Err(e) = bot.set_my_commands(public_commands).await { tracing::error!("[TELEGRAM API ERROR] Failed to execute: {}", e); }
    let _ = bot
        .set_my_commands(Command::bot_commands())
        .scope(teloxide::types::BotCommandScope::Chat {
            chat_id: teloxide::types::Recipient::Id(teloxide::types::ChatId(admin_id)),
        })
        .await;

    // 🚀 Start detached background workers
    crate::workers::start_all(ctx.clone(), bot.clone(), cancel_token.clone());

    // 🕸️ Start RSS Crawler for Dynamic AI Knowledge Base (RAG)
    crate::workers::rss::spawn_rss_crawler(ctx.pool.clone(), cancel_token.clone());

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

    info!("🚀 Dispatcher is LIVE! Ready for users.");

    let mut dispatcher = Dispatcher::builder(bot.clone(), handler) // FIXME_PHASE4_SHUTDOWN: Bind 'crate::resilience::shutdown_signal()' to this to prevent DB corruption on exit.
        .dependencies(dptree::deps![ctx])
        .enable_ctrlc_handler()
        .default_handler(|update: Arc<Update>| async move {
            tracing::debug!(
                "[SYSTEM] Dropped completely unhandled update type: {:?}",
                update.id
            );
        })
        .build();

    let use_webhook =
        std::env::var("USE_WEBHOOK").unwrap_or_else(|_| "false".to_string()) == "true";

    if use_webhook {
        let domain = std::env::var("WEBHOOK_DOMAIN").expect("CRITICAL: WEBHOOK_DOMAIN required"); // FIXME_PHASE3: DANGER! Bot will crash here if it fails. Use '?' or 'safe_unwrap!'
        let port: u16 = std::env::var("WEBHOOK_PORT")
            .unwrap_or_else(|_| "8443".to_string())
            .parse()
            .unwrap(); // FIXME_PHASE3: DANGER! Bot will crash here if it fails. Use '?' or 'safe_unwrap!'
        let addr = ([0, 0, 0, 0], port).into();
        let url = format!("https://{}/webhook", domain).parse().unwrap(); // FIXME_PHASE3: DANGER! Bot will crash here if it fails. Use '?' or 'safe_unwrap!'

        // ✅ Delete any rogue polling updates BEFORE setting the Webhook
        if let Err(e) = bot.delete_webhook().drop_pending_updates(true).send().await { tracing::error!("[TELEGRAM API ERROR] Failed to execute: {}", e); }

        tracing::info!(
            "🌐 [NETWORK] Enterprise Webhook Mode Active. Listening on port {} for domain {}",
            port,
            domain
        );

        let listener = teloxide::update_listeners::webhooks::axum(
            bot,
            teloxide::update_listeners::webhooks::Options::new(addr, url),
        )
        .await
        .expect("Failed to setup webhook"); // FIXME_PHASE3: DANGER! Bot will crash here if it fails. Use '?' or 'safe_unwrap!'
        let error_handler =
            teloxide::error_handlers::LoggingErrorHandler::with_custom_text("Webhook Error");

        dispatcher
            .dispatch_with_listener(listener, error_handler)
            .await;
    } else {
        // ✅ Delete Webhook explicitly to allow Polling to work
        if let Err(e) = bot.delete_webhook().drop_pending_updates(true).send().await { tracing::error!("[TELEGRAM API ERROR] Failed to execute: {}", e); }

        tracing::info!("🔄 [NETWORK] Polling Mode Active (Standard Development Fallback).");
        dispatcher.dispatch().await; // FIXME_PHASE4_SHUTDOWN: Bind 'crate::resilience::shutdown_signal()' to this to prevent DB corruption on exit.
    }

    Ok(())
}







