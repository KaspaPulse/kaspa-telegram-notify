use teloxide::utils::command::BotCommands;

#[derive(BotCommands, Clone, std::fmt::Debug)]
#[command(rename_rule = "lowercase", description = "Kaspa Pulse Bot Commands:")]
pub enum Command {
    #[command(description = "Start the bot and show main menu.")]
    Start,
    #[command(description = "Show the guide and features.")]
    Help,
    #[command(description = "Add a wallet: /add <address>")]
    Add(String),
    #[command(description = "Remove a wallet: /remove <address>")]
    Remove(String),
    #[command(description = "List all tracked wallets.")]
    List,
    #[command(description = "Check live balance and UTXOs.")]
    Balance,
    #[command(description = "Estimate your solo-mining hashrate.")]
    Miner,
    #[command(description = "Count your unspent mined blocks.")]
    Blocks,
    #[command(description = "Support the developer.")]
    Donate,

    #[command(
        rename = "forget_wallets",
        description = "Delete all my tracked wallets."
    )]
    ForgetWallets,
    #[command(rename = "forget_all", description = "Erase all my data.")]
    ForgetAll,
    #[command(rename = "hidemenu", description = "إخفاء الكيبورد الثابت")]
    HideMenu,

    #[command(description = "Show full node and network health.")]
    Network,
    #[command(description = "Show BlockDAG consensus details.")]
    Dag,
    #[command(description = "Check KAS price and market cap.")]
    Price,
    #[command(description = "Check market cap details.")]
    Market,
    #[command(description = "Check circulating and max supply.")]
    Supply,
    #[command(description = "Check real-time mempool fees.")]
    Fees,

    #[command(description = "Admin: Community bot health report.")]
    Health,
    #[command(description = "Admin: Global analytics and user report.")]
    Stats,
    #[command(description = "Admin: System hardware diagnostics.")]
    Sys,
    #[command(description = "Admin: Pause UTXO monitoring.")]
    Pause,
    #[command(description = "Admin: Resume UTXO monitoring.")]
    Resume,
    #[command(
        rename = "mute_alerts",
        description = "Admin: Stop sending mining alerts only."
    )]
    MuteAlerts,
    #[command(
        rename = "unmute_alerts",
        description = "Admin: Resume sending mining alerts."
    )]
    UnmuteAlerts,
    #[command(
        rename = "alerts_status",
        description = "Admin: Show alert delivery status."
    )]
    AlertsStatus,
    #[command(
        rename = "restart_info",
        description = "Admin: Show external service restart instructions."
    )]
    RestartInfo,
    #[command(description = "Admin: Tail last 25 lines of bot.log.")]
    Logs,
    #[command(description = "Admin: Show recent bot event log.")]
    Events,
    #[command(rename = "errors", description = "Admin: Show recent error events.")]
    Errors,
    #[command(
        rename = "delivery",
        description = "Admin: Show alert delivery summary."
    )]
    Delivery,
    #[command(
        rename = "subscribers",
        description = "Admin: Show wallet subscribers."
    )]
    Subscribers(String),
    #[command(
        rename = "wallet_events",
        description = "Admin: Show wallet event history."
    )]
    WalletEvents(String),
    #[command(
        rename = "cleanup_events",
        description = "Admin: Cleanup old bot events."
    )]
    CleanupEvents,
    #[command(
        rename = "db_diag",
        description = "Admin: Database health diagnostics."
    )]
    DbDiag,
    #[command(description = "Admin: Open settings panel.")]
    Settings,
    #[command(description = "Admin: Toggle a feature flag.")]
    Toggle(String),
    #[command(description = "Erase all my data and wallets.")]
    Forget,
}

impl Command {
    pub fn is_admin_only(&self) -> bool {
        matches!(
            self,
            Self::Health
                | Self::Stats
                | Self::Sys
                | Self::Pause
                | Self::Resume
                | Self::MuteAlerts
                | Self::UnmuteAlerts
                | Self::AlertsStatus
                | Self::RestartInfo
                | Self::Logs
                | Self::Events
                | Self::Errors
                | Self::Delivery
                | Self::Subscribers(_)
                | Self::WalletEvents(_)
                | Self::CleanupEvents
                | Self::DbDiag
                | Self::Settings
                | Self::Toggle(_)
        )
    }
}

pub fn public_bot_commands() -> Vec<teloxide::types::BotCommand> {
    vec![
        teloxide::types::BotCommand::new("start", "Open the main menu"),
        teloxide::types::BotCommand::new("help", "Show the guide and features"),
        teloxide::types::BotCommand::new("add", "Add a wallet: /add kaspa:..."),
        teloxide::types::BotCommand::new("remove", "Remove a wallet: /remove kaspa:..."),
        teloxide::types::BotCommand::new("list", "Show tracked wallets"),
        teloxide::types::BotCommand::new("balance", "Check live balance and UTXOs"),
        teloxide::types::BotCommand::new("miner", "Estimate solo-mining hashrate"),
        teloxide::types::BotCommand::new("blocks", "Show mined block stats"),
        teloxide::types::BotCommand::new("network", "Show node and network health"),
        teloxide::types::BotCommand::new("dag", "Show BlockDAG overview"),
        teloxide::types::BotCommand::new("price", "Check KAS price and market data"),
        teloxide::types::BotCommand::new("market", "Check market cap details"),
        teloxide::types::BotCommand::new("supply", "Check circulating and max supply"),
        teloxide::types::BotCommand::new("fees", "Check real-time network fees"),
        teloxide::types::BotCommand::new("donate", "Support development"),
        teloxide::types::BotCommand::new("forget_wallets", "Delete all my tracked wallets"),
        teloxide::types::BotCommand::new("forget_all", "Erase all my data"),
        teloxide::types::BotCommand::new("hidemenu", "Hide the persistent keyboard"),
    ]
}

pub fn admin_bot_commands() -> Vec<teloxide::types::BotCommand> {
    let mut commands = public_bot_commands();

    commands.extend(vec![
        teloxide::types::BotCommand::new("health", "Admin: bot health report"),
        teloxide::types::BotCommand::new("stats", "Admin: global analytics report"),
        teloxide::types::BotCommand::new("sys", "Admin: system diagnostics"),
        teloxide::types::BotCommand::new("pause", "Admin: pause monitoring"),
        teloxide::types::BotCommand::new("resume", "Admin: resume monitoring"),
        teloxide::types::BotCommand::new("restart_info", "Admin: show restart instructions"),
        teloxide::types::BotCommand::new("logs", "Admin: tail recent log lines"),
        teloxide::types::BotCommand::new("events", "Admin: show recent bot events"),
        teloxide::types::BotCommand::new("errors", "Admin: show recent error events"),
        teloxide::types::BotCommand::new("delivery", "Admin: alert delivery summary"),
        teloxide::types::BotCommand::new("mute_alerts", "Admin: stop sending mining alerts"),
        teloxide::types::BotCommand::new("unmute_alerts", "Admin: resume sending mining alerts"),
        teloxide::types::BotCommand::new("alerts_status", "Admin: alert delivery status"),
        teloxide::types::BotCommand::new("subscribers", "Admin: show wallet subscribers"),
        teloxide::types::BotCommand::new("wallet_events", "Admin: show wallet event history"),
        teloxide::types::BotCommand::new("cleanup_events", "Admin: cleanup old bot events"),
        teloxide::types::BotCommand::new("db_diag", "Admin: database diagnostics"),
        teloxide::types::BotCommand::new("settings", "Admin: open settings panel"),
        teloxide::types::BotCommand::new("toggle", "Admin: toggle a feature flag"),
    ]);

    commands
}
