use crate::domain::models::{BotEventRecord, BotEventType, EventSeverity};
use crate::infrastructure::database::postgres_adapter::PostgresRepository;
use crate::infrastructure::node::kaspa_adapter::KaspaRpcAdapter;

mod application;
mod build_info;
mod config;
mod domain;
mod infrastructure;
mod network;
mod presentation;
mod wallet;

pub mod utils;

use dotenvy::dotenv;
use std::env;
use std::fs;
use std::net::SocketAddr;
use std::path::PathBuf;
use std::str::FromStr;
use std::sync::Arc;
use teloxide::dptree;
use teloxide::prelude::*;
use teloxide::types::BotCommandScope;
use tracing::info;
use tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, registry, util::SubscriberInitExt};

use crate::infrastructure::market::coingecko_adapter::CoinGeckoAdapter;
use crate::network::analyze_dag::AnalyzeDagUseCase;
use crate::network::stats_use_cases::GetMinerStatsUseCase;
use crate::network::stats_use_cases::NetworkStatsUseCase;
use crate::presentation::telegram::commands::Command;
use crate::wallet::wallet_use_cases::WalletManagementUseCase;
use crate::wallet::wallet_use_cases::WalletQueriesUseCase;

fn panic_event_marker_path() -> PathBuf {
    env::var("PANIC_EVENT_MARKER_PATH")
        .map(PathBuf::from)
        .unwrap_or_else(|_| PathBuf::from("panic_event_pending.json"))
}

fn truncate_panic_text(value: &str, max_chars: usize) -> String {
    let mut text = value
        .replace('\u{0000}', "")
        .replace(['\r', '\n'], " ")
        .trim()
        .to_string();

    if text.chars().count() > max_chars {
        text = text.chars().take(max_chars).collect::<String>();
        text.push_str("...[truncated]");
    }

    text
}

fn write_pending_panic_marker(location: &str, message: &str) {
    let marker_path = panic_event_marker_path();

    let payload = serde_json::json!({
        "event_type": "PANIC_EVENT",
        "status": "pending_recovery",
        "location": truncate_panic_text(location, 300),
        "message": truncate_panic_text(message, 1000),
        "created_at": chrono::Utc::now().to_rfc3339(),
        "pid": std::process::id()
    });

    if let Err(e) = fs::write(&marker_path, payload.to_string()) {
        tracing::error!(
            "[PANIC_EVENT] Failed to write pending panic marker at {:?}: {}",
            marker_path,
            e
        );
    }
}

async fn record_pending_panic_marker(
    db_repo: &Arc<PostgresRepository>,
) -> Result<(), crate::domain::errors::AppError> {
    let marker_path = panic_event_marker_path();

    if !marker_path.exists() {
        return Ok(());
    }

    let marker_content = match fs::read_to_string(&marker_path) {
        Ok(content) => content,
        Err(e) => {
            tracing::error!(
                "[PANIC_EVENT] Failed to read pending panic marker at {:?}: {}",
                marker_path,
                e
            );
            return Ok(());
        }
    };

    let marker_json: serde_json::Value =
        serde_json::from_str(&marker_content).unwrap_or_else(|_| {
            serde_json::json!({
                "event_type": "PANIC_EVENT",
                "status": "pending_recovery",
                "message": truncate_panic_text(&marker_content, 1000)
            })
        });

    let message = marker_json
        .get("message")
        .and_then(|value| value.as_str())
        .unwrap_or("panic marker recovered");

    let metadata_json = marker_json.to_string();

    db_repo
        .record_bot_event_typed(
            BotEventType::PanicEvent,
            EventSeverity::Error,
            None,
            None,
            None,
            None,
            None,
            None,
            None,
            Some("recovered_after_restart"),
            Some(message),
            None,
            &metadata_json,
        )
        .await?;

    if let Err(e) = fs::remove_file(&marker_path) {
        tracing::warn!(
            "[PANIC_EVENT] Failed to remove pending panic marker at {:?}: {}",
            marker_path,
            e
        );
    } else {
        tracing::info!("[PANIC_EVENT] Recovered pending panic marker into bot_event_log.");
    }

    Ok(())
}

async fn wait_for_shutdown_signal() -> &'static str {
    #[cfg(unix)]
    {
        use tokio::signal::unix::{SignalKind, signal};

        match signal(SignalKind::terminate()) {
            Ok(mut sigterm) => {
                tokio::select! {
                    result = tokio::signal::ctrl_c() => {
                        if let Err(error) = result {
                            tracing::error!("[SYSTEM] SIGINT listener failed: {}", error);
                        }
                        "SIGINT"
                    }
                    _ = sigterm.recv() => "SIGTERM",
                }
            }
            Err(error) => {
                tracing::error!("[SYSTEM] Failed to install SIGTERM handler: {}", error);
                if let Err(ctrl_c_error) = tokio::signal::ctrl_c().await {
                    tracing::error!("[SYSTEM] SIGINT listener failed: {}", ctrl_c_error);
                }
                "SIGINT"
            }
        }
    }

    #[cfg(not(unix))]
    {
        if let Err(error) = tokio::signal::ctrl_c().await {
            tracing::error!("[SYSTEM] SIGINT listener failed: {}", error);
        }
        "SIGINT"
    }
}

fn install_rustls_crypto_provider() -> anyhow::Result<()> {
    if rustls::crypto::CryptoProvider::get_default().is_none() {
        let _ = rustls::crypto::ring::default_provider().install_default();
    }

    anyhow::ensure!(
        rustls::crypto::CryptoProvider::get_default().is_some(),
        "Rustls CryptoProvider could not be installed during application startup"
    );
    Ok(())
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    install_rustls_crypto_provider()?;
    dotenv().ok();

    let filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    registry().with(fmt::layer()).with(filter).init();

    std::panic::set_hook(Box::new(|panic_info| {
        let location = panic_info
            .location()
            .map(|loc| format!("{}:{}", loc.file(), loc.line()))
            .unwrap_or_else(|| "unknown".to_string());

        let message = panic_info
            .payload()
            .downcast_ref::<&str>()
            .map(|value| (*value).to_string())
            .or_else(|| {
                panic_info
                    .payload()
                    .downcast_ref::<String>()
                    .map(|value| value.to_string())
            })
            .unwrap_or_else(|| "unknown panic payload".to_string());

        write_pending_panic_marker(&location, &message);

        tracing::error!(
            event_type = "PANIC_EVENT",
            location = %location,
            message = %message,
            "panic captured by global panic hook"
        );
    }));

    info!(
        version = build_info::VERSION,
        source_revision = build_info::source_revision(),
        "Kaspa Pulse starting."
    );

    let startup = crate::config::StartupConfig::from_env()?;

    tracing::info!(
        "[SYSTEM] Environment: {} | DB max connections: {} | Verbose logs: {}",
        startup.app_env,
        startup.db_max_connections,
        startup.verbose_logs
    );

    let pool = sqlx::postgres::PgPoolOptions::new()
        .max_connections(startup.db_max_connections)
        .connect(&startup.database_url)
        .await?;

    let db_repo = Arc::new(PostgresRepository::new(pool.clone()));

    if startup.allow_runtime_schema_ensure {
        tracing::warn!(
            "[DATABASE] Runtime schema ensure is enabled. Prefer applying migrations before startup."
        );

        db_repo
            .ensure_pending_rewards_table()
            .await
            .map_err(|e| anyhow::anyhow!("Failed to ensure pending_rewards table: {}", e))?;
    } else {
        tracing::info!(
            "[DATABASE] Runtime schema ensure disabled. Database schema is expected to be managed by migrations."
        );
    }
    if let Err(e) = record_pending_panic_marker(&db_repo).await {
        tracing::error!("[PANIC_EVENT] Failed to record pending panic marker: {}", e);
    }
    let mut system_start_event =
        BotEventRecord::new(BotEventType::SystemStart, EventSeverity::Info);
    system_start_event.status = Some("ok");

    if let Err(error) = db_repo.record_bot_event_record(system_start_event).await {
        tracing::error!("[SYSTEM] Failed to persist system start event: {}", error);
    }

    let network_id = match kaspa_consensus_core::network::NetworkId::from_str("mainnet") {
        Ok(network_id) => network_id,
        Err(mainnet_error) => {
            match kaspa_consensus_core::network::NetworkId::from_str("testnet-12") {
                Ok(network_id) => network_id,
                Err(testnet_error) => {
                    return Err(anyhow::anyhow!(
                        "failed to parse fallback network ids: mainnet={mainnet_error}; testnet-12={testnet_error}"
                    ));
                }
            }
        }
    };

    let rpc_client = kaspa_wrpc_client::KaspaRpcClient::new(
        kaspa_wrpc_client::WrpcEncoding::SerdeJson,
        Some(&startup.node_url),
        None,
        Some(network_id),
        None,
    )
    .map_err(|e| anyhow::anyhow!("RPC Connection Failed: {}", e))?;

    let rpc_client_arc = Arc::new(rpc_client);
    let node_provider = Arc::new(KaspaRpcAdapter::new(rpc_client_arc.clone()));

    tracing::info!("[SYSTEM] Running node pre-flight diagnostic with safe timeouts.");
    let preflight = tokio::time::timeout(
        crate::infrastructure::resilience::runtime::rpc_timeout_duration(),
        async {
            let mut failed_operations = Vec::new();

            if node_provider.get_server_info().await.is_err() {
                failed_operations.push("get_server_info");
            }
            if node_provider.get_sync_status().await.is_err() {
                failed_operations.push("get_sync_status");
            }
            if node_provider.get_block_dag_info().await.is_err() {
                failed_operations.push("get_block_dag_info");
            }
            if node_provider.get_coin_supply().await.is_err() {
                failed_operations.push("get_coin_supply");
            }
            if node_provider.get_utxos_by_addresses(vec![]).await.is_err() {
                tracing::debug!(
                    "[SYSTEM] Empty-address UTXO pre-flight probe failed; continuing diagnostic."
                );
            }
            if node_provider.connect(false).await.is_err() {
                failed_operations.push("connect");
            }

            failed_operations
        },
    )
    .await;

    match preflight {
        Ok(failed_operations) if failed_operations.is_empty() => {
            tracing::info!("[SYSTEM] Node pre-flight completed successfully.");
        }
        Ok(failed_operations) => {
            tracing::warn!(
                failed_operations = ?failed_operations,
                "[SYSTEM] Node pre-flight completed with failures. Bot will start in degraded mode and node monitor will keep retrying."
            );
        }
        Err(_) => {
            crate::infrastructure::metrics::inc_rpc_timeouts();
            tracing::warn!(
                "[SYSTEM] Node pre-flight timed out. Bot will start in degraded mode and node monitor will keep retrying."
            );
        }
    }

    let market_provider: Arc<dyn crate::infrastructure::market::coingecko_adapter::MarketProvider> =
        Arc::new(CoinGeckoAdapter::new()?);

    let wallet_management_uc = Arc::new(WalletManagementUseCase::new(db_repo.clone()));

    let wallet_queries_uc = Arc::new(WalletQueriesUseCase::new(
        db_repo.clone(),
        node_provider.clone(),
    ));

    let network_stats_uc = Arc::new(NetworkStatsUseCase::new(node_provider.clone()));
    let dag_uc = Arc::new(AnalyzeDagUseCase::new(node_provider.clone()));

    let get_miner_stats_uc = Arc::new(GetMinerStatsUseCase::new(
        db_repo.clone(),
        node_provider.clone(),
    ));

    let market_stats_uc = Arc::new(crate::network::stats_use_cases::GetMarketStatsUseCase::new(
        node_provider.clone(),
        market_provider.clone(),
    ));

    let bot = Bot::new(startup.bot_token.clone());
    let admin_user_id = startup.admin_user_id;
    let admin_chat_id = startup.admin_chat_id;
    let mut telegram_command_sync_errors = 0usize;

    // Clear stale Telegram command scopes so deleted legacy commands disappear.
    if let Err(error) = bot.delete_my_commands().await {
        telegram_command_sync_errors += 1;
        tracing::warn!(
            operation = "delete_default_commands",
            error = %crate::utils::sanitize_for_log(&error.to_string()),
            "[SYSTEM] Telegram command synchronization operation failed."
        );
    }
    if let Err(error) = bot
        .delete_my_commands()
        .scope(BotCommandScope::AllPrivateChats)
        .await
    {
        telegram_command_sync_errors += 1;
        tracing::warn!(
            operation = "delete_private_commands",
            error = %crate::utils::sanitize_for_log(&error.to_string()),
            "[SYSTEM] Telegram command synchronization operation failed."
        );
    }
    if let Err(error) = bot
        .delete_my_commands()
        .scope(BotCommandScope::AllGroupChats)
        .await
    {
        telegram_command_sync_errors += 1;
        tracing::warn!(
            operation = "delete_group_commands",
            error = %crate::utils::sanitize_for_log(&error.to_string()),
            "[SYSTEM] Telegram command synchronization operation failed."
        );
    }
    if let Err(error) = bot
        .delete_my_commands()
        .scope(BotCommandScope::AllChatAdministrators)
        .await
    {
        telegram_command_sync_errors += 1;
        tracing::warn!(
            operation = "delete_chat_admin_commands",
            error = %crate::utils::sanitize_for_log(&error.to_string()),
            "[SYSTEM] Telegram command synchronization operation failed."
        );
    }
    if let Err(error) = bot
        .delete_my_commands()
        .scope(BotCommandScope::Chat {
            chat_id: teloxide::types::Recipient::Id(ChatId(admin_chat_id)),
        })
        .await
    {
        telegram_command_sync_errors += 1;
        tracing::warn!(
            operation = "delete_admin_chat_commands",
            error = %crate::utils::sanitize_for_log(&error.to_string()),
            "[SYSTEM] Telegram command synchronization operation failed."
        );
    }

    // Public commands for all users.
    if let Err(error) = bot
        .set_my_commands(crate::presentation::telegram::commands::public_bot_commands())
        .await
    {
        telegram_command_sync_errors += 1;
        tracing::warn!(
            operation = "set_public_commands",
            error = %crate::utils::sanitize_for_log(&error.to_string()),
            "[SYSTEM] Telegram command synchronization operation failed."
        );
    }

    // Admin commands only in the admin chat.
    if let Err(error) = bot
        .set_my_commands(crate::presentation::telegram::commands::admin_bot_commands())
        .scope(BotCommandScope::Chat {
            chat_id: teloxide::types::Recipient::Id(ChatId(admin_chat_id)),
        })
        .await
    {
        telegram_command_sync_errors += 1;
        tracing::warn!(
            operation = "set_admin_commands",
            error = %crate::utils::sanitize_for_log(&error.to_string()),
            "[SYSTEM] Telegram command synchronization operation failed."
        );
    }

    if telegram_command_sync_errors == 0 {
        tracing::info!("[SYSTEM] Telegram commands synced.");
    } else {
        tracing::warn!(
            failed_operations = telegram_command_sync_errors,
            "[SYSTEM] Telegram command synchronization completed with errors; bot startup will continue."
        );
    }

    let cancel_token = tokio_util::sync::CancellationToken::new();

    let app_context = std::sync::Arc::new(crate::domain::models::AppContext::new(
        rpc_client_arc.clone(),
        pool.clone(),
        admin_user_id,
        admin_chat_id,
    ));

    {
        let is_mem = db_repo
            .get_setting("ENABLE_MEMORY_CLEANER", "false")
            .await
            .unwrap_or_else(|_| "false".to_string())
            == "true";
        app_context
            .memory_cleaner_enabled
            .store(is_mem, std::sync::atomic::Ordering::Relaxed);

        let is_sync = db_repo
            .get_setting("ENABLE_LIVE_SYNC", "true")
            .await
            .unwrap_or_else(|_| "true".to_string())
            == "true";
        app_context
            .live_sync_enabled
            .store(is_sync, std::sync::atomic::Ordering::Relaxed);

        let is_maint = db_repo
            .get_setting("MAINTENANCE_MODE", "false")
            .await
            .unwrap_or_else(|_| "false".to_string())
            == "true";
        app_context
            .maintenance_mode
            .store(is_maint, std::sync::atomic::Ordering::Relaxed);
    }

    crate::presentation::telegram::workers::utxo_monitor::start_utxo_monitor(
        bot.clone(),
        node_provider.clone(),
        db_repo.clone(),
        app_context.live_sync_enabled.clone(),
        cancel_token.clone(),
    );

    crate::presentation::telegram::workers::telegram_delivery::start_telegram_delivery_worker(
        bot.clone(),
        app_context.pool.clone(),
        cancel_token.clone(),
    );
    crate::infrastructure::external_services::system::spawn_node_monitor(
        (*app_context).clone(),
        bot.clone(),
        cancel_token.clone(),
    );

    crate::infrastructure::external_services::system::spawn_price_monitor(
        (*app_context).clone(),
        cancel_token.clone(),
    )?;

    crate::infrastructure::external_services::system::spawn_memory_cleaner(
        (*app_context).clone(),
        cancel_token.clone(),
    );

    let system_tasks_uc = Arc::new(
        crate::application::background_jobs::SystemTasksUseCase::new(
            db_repo.clone(),
            market_provider.clone(),
        ),
    );

    crate::presentation::telegram::workers::periodic_tasks::start_system_monitors(
        system_tasks_uc.clone(),
        cancel_token.clone(),
    );

    crate::infrastructure::webhook_security::spawn_health_endpoint(cancel_token.clone());

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

    let bot_use_cases = crate::presentation::telegram::handlers::BotUseCases {
        wallet_mgt: wallet_management_uc.clone(),
        wallet_query: wallet_queries_uc.clone(),
        network_stats: network_stats_uc.clone(),
        market_stats: market_stats_uc.clone(),
        miner_stats: get_miner_stats_uc.clone(),
        dag_uc: dag_uc.clone(),
    };
    let callback_execution_registry = Arc::new(
        crate::presentation::telegram::callback_inflight::CallbackExecutionRegistry::default(),
    );

    let mut dispatcher = Dispatcher::builder(bot.clone(), handler)
        .dependencies(dptree::deps![
            db_repo.clone(),
            node_provider,
            app_context,
            dag_uc,
            bot_use_cases,
            callback_execution_registry
        ])
        .build();

    if startup.use_webhook {
        info!("Running in WEBHOOK mode");

        let webhook = startup
            .webhook
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("validated webhook configuration is missing"))?;

        crate::infrastructure::webhook_security::validate_webhook_runtime_settings(
            &startup.app_env,
            webhook.bind_ip,
            &webhook.domain,
            &webhook.secret_token,
        )?;

        let addr = SocketAddr::new(webhook.bind_ip, webhook.port);
        let url = format!("https://{}/webhook", webhook.domain).parse()?;
        let webhook_metadata = format!(
            r#"{{"domain":"{}","bind":"{}","port":{}}}"#,
            webhook.domain, webhook.bind_ip, webhook.port
        );

        let mut webhook_start_event =
            BotEventRecord::new(BotEventType::WebhookStart, EventSeverity::Info);
        webhook_start_event.status = Some("listening");
        webhook_start_event.metadata_json = &webhook_metadata;

        if let Err(error) = db_repo.record_bot_event_record(webhook_start_event).await {
            tracing::error!("[WEBHOOK] Failed to persist webhook start event: {}", error);
        }

        tracing::info!(
            "[WEBHOOK] Listening on {}:{} for domain {}",
            webhook.bind_ip,
            webhook.port,
            webhook.domain
        );

        let options = teloxide::update_listeners::webhooks::Options::new(addr, url)
            .secret_token(webhook.secret_token.clone())
            .max_connections(crate::infrastructure::webhook_security::webhook_max_connections());

        let listener = teloxide::update_listeners::webhooks::axum(bot, options).await?;

        tokio::select! {
            signal = wait_for_shutdown_signal() => {
                tracing::warn!("[SYSTEM] {} received. Starting graceful shutdown.", signal);
                cancel_token.cancel();
            }
            _ = dispatcher.dispatch_with_listener(
                listener,
                LoggingErrorHandler::with_custom_text("Webhook Error"),
            ) => {
                tracing::warn!("[SYSTEM] Webhook dispatcher exited. Stopping background workers.");
                cancel_token.cancel();
            }
        }
    } else {
        info!("Running in POLLING mode");
        bot.delete_webhook().await?;
        tokio::select! {
            signal = wait_for_shutdown_signal() => {
                tracing::warn!("[SYSTEM] {} received. Starting graceful shutdown.", signal);
                cancel_token.cancel();
            }
            _ = dispatcher.dispatch() => {
                tracing::warn!("[SYSTEM] Polling dispatcher exited. Stopping background workers.");
                cancel_token.cancel();
            }
        }
    }

    cancel_token.cancel();

    let mut shutdown_event = BotEventRecord::new(BotEventType::SystemShutdown, EventSeverity::Info);
    shutdown_event.status = Some("ok");
    shutdown_event.metadata_json = r#"{"reason":"graceful_shutdown"}"#;

    if let Err(error) = db_repo.record_bot_event_record(shutdown_event).await {
        tracing::error!("[SYSTEM] Failed to persist shutdown event: {}", error);
    }

    let shutdown_drain_secs = startup.shutdown_drain_secs;
    let drain_timeout = std::time::Duration::from_secs(shutdown_drain_secs);

    tracing::info!(
        "[SYSTEM] Waiting up to {} seconds for tracked background workers to stop.",
        shutdown_drain_secs
    );

    if crate::infrastructure::resilience::runtime::drain_tracked_tasks(drain_timeout).await {
        tracing::info!("[SYSTEM] All tracked background workers stopped cleanly.");
    } else {
        tracing::warn!(
            "[SYSTEM] Background worker drain timed out after {} seconds; closing database pool.",
            shutdown_drain_secs
        );
    }

    pool.close().await;
    tracing::info!("[SYSTEM] Database connections closed safely.");

    Ok(())
}

#[cfg(test)]
mod rustls_startup_tests {
    #[test]
    fn explicit_provider_prevents_ambiguous_rustls_builder_panic() {
        super::install_rustls_crypto_provider().expect("Rustls provider must install");
        let _ = rustls::ClientConfig::builder();
        assert!(rustls::crypto::CryptoProvider::get_default().is_some());
    }
}
