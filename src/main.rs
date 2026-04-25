use crate::infrastructure::database::postgres_adapter::PostgresRepository;
use crate::infrastructure::node::kaspa_adapter::KaspaRpcAdapter;
mod ai;
mod config;
mod network;
mod wallet;

mod application;
mod domain;
mod infrastructure;
mod presentation;

pub mod utils;

use dotenvy::dotenv;
use std::env;
use std::str::FromStr;
use std::sync::Arc;
use teloxide::dptree;
use teloxide::prelude::*;
use tracing::info;
use tracing_subscriber::{fmt, layer::SubscriberExt, registry, util::SubscriberInitExt, EnvFilter};

// Infrastructure Adapters
use crate::infrastructure::ai::ai_engine_adapter::AiEngineAdapter;

use crate::infrastructure::market::coingecko_adapter::CoinGeckoAdapter;
use crate::infrastructure::news::rss_adapter::RssAdapter;
// Application Use Cases
use crate::ai::ai_use_cases::AiChatUseCase;
use crate::network::analyze_dag::AnalyzeDagUseCase;
use crate::network::stats_use_cases::GetMinerStatsUseCase;
use crate::network::stats_use_cases::NetworkStatsUseCase;
use crate::wallet::wallet_use_cases::SyncWalletUseCase;
use crate::wallet::wallet_use_cases::WalletManagementUseCase;
use crate::wallet::wallet_use_cases::WalletQueriesUseCase;

// Presentation Handlers & Commands
use crate::presentation::telegram::commands::Command;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    dotenv().ok();

    // 1. Setup Logging
    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    registry().with(fmt::layer()).with(filter).init();
    info!("🚀 Kaspa Pulse Enterprise Engine Starting...");

    // 2. Initialize Infrastructure Layer
    let db_url = env::var("DATABASE_URL").expect("DATABASE_URL must be set in .env");
    let rpc_url =
        env::var("NODE_URL_01").expect("CRITICAL SECURITY: NODE_URL_01 must be set in .env");

    // Setup Postgres Pool
    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(20)
        .connect(&db_url)
        .await?;
    let db_repo = Arc::new(PostgresRepository::new(pool.clone()));

    // Setup Kaspa RPC Client
    let network_id =
        kaspa_consensus_core::network::NetworkId::from_str("mainnet").unwrap_or_else(|_| {
            kaspa_consensus_core::network::NetworkId::from_str("testnet-12").unwrap()
        });

    let rpc_client = kaspa_wrpc_client::KaspaRpcClient::new(
        kaspa_wrpc_client::WrpcEncoding::SerdeJson,
        Some(&rpc_url),
        None,
        Some(network_id),
        None,
    )
    .map_err(|e| anyhow::anyhow!("RPC Connection Failed: {}", e))?;

    let rpc_client_arc = Arc::new(rpc_client);
    let node_provider = Arc::new(KaspaRpcAdapter::new(rpc_client_arc.clone()));

    // 🛡️ ENTERPRISE PRE-FLIGHT CHECK (Activating wrappers & bounds)
    tracing::info!("[SYSTEM] Running node pre-flight diagnostic...");
    let _ = node_provider.get_server_info().await;
    let _ = node_provider.get_sync_status().await;
    let _ = node_provider.get_block_dag_info().await;
    let _ = node_provider.get_coin_supply().await;
    let _ = node_provider.get_utxos_by_addresses(vec![]).await;
    let _ = node_provider.connect(false).await;

    // Setup AI Engine
    let ai_provider: Arc<crate::infrastructure::ai::ai_engine_adapter::AiEngineAdapter> =
        Arc::new(AiEngineAdapter::new(
            env::var("AI_CHAT_API_KEY").unwrap_or_default(),
            env::var("AI_CHAT_BASE_URL")
                .expect("CRITICAL SECURITY: AI_CHAT_BASE_URL must be set in .env"),
            env::var("AI_AUDIO_API_KEY").unwrap_or_default(),
            env::var("AI_AUDIO_BASE_URL")
                .expect("CRITICAL SECURITY: AI_AUDIO_BASE_URL must be set in .env"),
            env::var("AI_CHAT_MODEL")
                .expect("CRITICAL SECURITY: AI_CHAT_MODEL must be set in .env"),
            env::var("AI_AUDIO_MODEL")
                .expect("CRITICAL SECURITY: AI_AUDIO_MODEL must be set in .env"),
        ));

    // Setup Market & News Providers
    let news_provider: Arc<dyn crate::infrastructure::news::rss_adapter::NewsProvider> =
        Arc::new(RssAdapter::new());
    let market_provider: Arc<dyn crate::infrastructure::market::coingecko_adapter::MarketProvider> =
        Arc::new(CoinGeckoAdapter::new());

    // 3. Initialize Application Use Cases (Clean Architecture)
    let wallet_management_uc = Arc::new(WalletManagementUseCase::new(db_repo.clone()));
    let wallet_queries_uc = Arc::new(WalletQueriesUseCase::new(
        db_repo.clone(),
        node_provider.clone(),
    ));
    let ai_chat_uc = Arc::new(AiChatUseCase::new(ai_provider.clone(), db_repo.clone()));
    let network_stats_uc = Arc::new(NetworkStatsUseCase::new(node_provider.clone()));
    let dag_uc = Arc::new(AnalyzeDagUseCase::new(node_provider.clone()));
    let sync_uc = Arc::new(SyncWalletUseCase::new(
        db_repo.clone(),
        node_provider.clone(),
    ));
    let get_miner_stats_uc = Arc::new(GetMinerStatsUseCase::new(
        db_repo.clone(),
        node_provider.clone(),
    ));
    let crawl_news_uc = Arc::new(crate::application::background_jobs::CrawlNewsUseCase::new(
        db_repo.clone(),
        news_provider.clone(),
    ));
    let market_stats_uc = Arc::new(crate::network::stats_use_cases::GetMarketStatsUseCase::new(
        node_provider.clone(),
        market_provider.clone(),
    ));
    let ai_rag_uc = Arc::new(crate::ai::ai_use_cases::AiRagUseCase::new(
        db_repo.clone(),
        ai_provider.clone(),
    ));

    // 4. Setup Telegram Layer
    let bot_token = env::var("BOT_TOKEN").expect("BOT_TOKEN must be set in .env");
    let bot = Bot::new(bot_token);
    use teloxide::utils::command::BotCommands;
    let _ = bot
        .set_my_commands(crate::presentation::telegram::commands::Command::bot_commands())
        .await;
    tracing::info!("✅ [SYSTEM] Telegram Commands Synced!");
    let admin_id: i64 = env::var("ADMIN_ID")
        .unwrap_or_default()
        .parse()
        .unwrap_or(0);

    // 5. Context & Graceful Shutdown Registry
    let cancel_token = tokio_util::sync::CancellationToken::new();
    let app_context = std::sync::Arc::new(crate::domain::models::AppContext::new(
        rpc_client_arc.clone(),
        pool.clone(),
        admin_id,
    ));

    let pool_shutdown = pool.clone();
    let ct_ctrlc = cancel_token.clone();

    // 🛡️ Enterprise Graceful Shutdown (No Fork Bombs)
    tokio::spawn(async move {
        let _ = tokio::signal::ctrl_c().await;
        tracing::warn!("🛑 [SYSTEM] SIGINT received. Executing Graceful Shutdown...");
        pool_shutdown.close().await;
        tracing::info!("✅ [SYSTEM] Database connections closed safely.");
        ct_ctrlc.cancel();
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        std::process::exit(0); // Clean exit, absolutely NO spawning of new processes.
    });

    // 6. Start Background Workers
    crate::presentation::telegram::workers::utxo_monitor::start_utxo_monitor(
        bot.clone(),
        node_provider.clone(),
        db_repo.clone(),
    );

    crate::infrastructure::external_services::system::spawn_node_monitor(
        (*app_context).clone(),
        bot.clone(),
        cancel_token.clone(),
    );
    crate::infrastructure::external_services::system::spawn_price_monitor(
        (*app_context).clone(),
        cancel_token.clone(),
    );

    // 🚀 PHASE 1 WIRES: RSS Crawler & Memory Cleaner
    tracing::info!("[SYSTEM] Wiring Phase 1: Activating RSS Crawler & Memory Cleaner...");
    crate::infrastructure::external_services::rss::spawn_rss_crawler(
        pool.clone(),
        cancel_token.clone(),
    );
    crate::infrastructure::external_services::system::spawn_memory_cleaner(
        (*app_context).clone(),
        cancel_token.clone(),
    );

    let system_tasks_uc = Arc::new(
        crate::application::background_jobs::SystemTasksUseCase::new(
            db_repo.clone(),
            ai_provider.clone(),
        ),
    );
    // 🚀 Start Enterprise Periodic Tasks (RSS & Memory GC)
    crate::presentation::telegram::workers::periodic_tasks::start_system_monitors(
        system_tasks_uc.clone(),
    );
    crate::presentation::telegram::workers::periodic_tasks::start_rss_crawler(
        crawl_news_uc.clone(),
    );
    let sys_ai = system_tasks_uc.clone();
    let ai_token = cancel_token.clone();

    tokio::spawn(async move {
        loop {
            tokio::select! {
                _ = ai_token.cancelled() => { break; }
                _ = tokio::time::sleep(std::time::Duration::from_secs(10)) => {
                    sys_ai.execute_ai_vectorizer().await;
                }
            }
        }
    });

    // 7. Build Dispatcher (Fixed Dependency Injection)
    use crate::presentation::telegram::handlers;

    let handler = dptree::entry()
        .branch(
            Update::filter_message()
                .filter_command::<Command>()
                .endpoint(handlers::handle_command),
        )
        .branch(Update::filter_callback_query().endpoint(handlers::handle_callback))
        .branch(Update::filter_my_chat_member().endpoint(handlers::handle_block_user))
        .branch(Update::filter_message().endpoint(handlers::handle_raw_message));

    // 🛡️ Fixed: Removed all duplicate dependencies
    let bot_use_cases = crate::presentation::telegram::handlers::BotUseCases {
        wallet_mgt: wallet_management_uc.clone(),
        wallet_query: wallet_queries_uc.clone(),
        network_stats: network_stats_uc.clone(),
        market_stats: market_stats_uc.clone(),
        miner_stats: get_miner_stats_uc.clone(),
        sync_uc: sync_uc.clone(),
        dag_uc: dag_uc.clone(),
    };

    let mut dispatcher = Dispatcher::builder(bot.clone(), handler)
        .dependencies(dptree::deps![
            db_repo,
            node_provider,
            ai_chat_uc,
            app_context,
            dag_uc,
            bot_use_cases,
            ai_rag_uc,
            ai_provider
        ])
        .enable_ctrlc_handler()
        .build();

    // 8. Run Mode
    if env::var("USE_WEBHOOK").unwrap_or_else(|_| "false".to_string()) == "true" {
        info!("🌐 Running in WEBHOOK mode");
        let domain = env::var("WEBHOOK_DOMAIN").expect("WEBHOOK_DOMAIN required");
        let port: u16 = env::var("WEBHOOK_PORT")
            .unwrap_or_else(|_| "8443".to_string())
            .parse()?;
        let addr = ([0, 0, 0, 0], port).into();
        let url = format!("https://{}/webhook", domain).parse()?;

        let listener = teloxide::update_listeners::webhooks::axum(
            bot,
            teloxide::update_listeners::webhooks::Options::new(addr, url),
        )
        .await?;

        dispatcher
            .dispatch_with_listener(
                listener,
                LoggingErrorHandler::with_custom_text("Webhook Error"),
            )
            .await;
    } else {
        info!("🔄 Running in POLLING mode");
        bot.delete_webhook().await?;
        dispatcher.dispatch().await;
    }

    Ok(())
}
