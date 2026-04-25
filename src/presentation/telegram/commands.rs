use teloxide::utils::command::BotCommands;

#[derive(BotCommands, Clone, std::fmt::Debug)]
#[command(
    rename_rule = "lowercase",
    description = "Kaspa Enterprise AI Bot Commands:"
)]
pub enum Command {
    // --- Public Commands ---
    #[command(description = "Start the bot and show main menu.")]
    Start,
    #[command(description = "Show the ultimate guide and features.")]
    Help,
    #[command(description = "Add a wallet: /add <address>")]
    Add(String),
    #[command(description = "Remove a wallet: /remove <address>")]
    Remove(String),
    #[command(description = "List all tracked wallets.")]
    List,
    #[command(description = "Check Live Balance & UTXOs.")]
    Balance,
    #[command(description = "Estimate your solo-mining hashrate.")]
    Miner,
    #[command(description = "Count your unspent mined blocks.")]
    Blocks,
    #[command(description = "Support the Developer.")]
    Donate,

    #[command(rename = "forget_chat", description = "Clear AI chat history.")]
    ForgetChat,
    #[command(
        rename = "forget_wallets",
        description = "Delete all my tracked wallets."
    )]
    ForgetWallets,
    #[command(rename = "forget_all", description = "GDPR: Erase ALL my data.")]
    ForgetAll,

    #[command(rename = "hidemenu", description = "إخفاء الكيبورد الثابت")]
    HideMenu,

    #[command(rename = "models", description = "Manage and change AI models")]
    Models,

    // --- Node & Market Stats ---
    #[command(description = "Show full node and network health.")]
    Network,
    #[command(description = "Show BlockDAG consensus details.")]
    Dag,
    #[command(description = "Check KAS Price & Market Cap.")]
    Price,
    #[command(description = "Check Market Cap details.")]
    Market,
    #[command(description = "Check circulating and max supply.")]
    Supply,
    #[command(description = "Check real-time Mempool fees.")]
    Fees,

    // --- Admin Enterprise Commands (Restricted) ---
    #[command(description = "Admin: Global Analytics & User Report.")]
    Stats,
    #[command(description = "Admin: System Hardware Diagnostics.")]
    Sys,
    #[command(description = "Admin: Global Reverse Sync from Pruning Point.")]
    Sync,
    #[command(description = "Admin: Pause UTXO monitoring.")]
    Pause,
    #[command(description = "Admin: Resume UTXO monitoring.")]
    Resume,
    #[command(description = "Admin: Safe restart of the bot binary.")]
    Restart,
    #[command(description = "Admin: Broadcast message to all users.")]
    Broadcast(String),
    #[command(description = "Admin: Tail last 25 lines of bot.log.")]
    Logs,
    #[command(description = "Admin: Teach AI new Kaspa facts.")]
    Learn(String),
    #[command(description = "Admin: Auto-fetch latest official Kaspa news.")]
    AutoLearn,
    #[command(
        rename = "flush_knowledge",
        description = "Admin: Flush AI Knowledge Base."
    )]
    FlushKnowledge,
    #[command(
        rename = "db_diag",
        description = "Admin: Database Health Diagnostics."
    )]
    DbDiag,
    #[command(description = "Admin: Trigger Autonomous AI Agent (Deep Search).")]
    Agent(String),
    #[command(description = "Admin: Open Enterprise Settings Panel.")]
    Settings,
    #[command(description = "Admin: Toggle a feature flag.")]
    Toggle(String),
    #[command(description = "GDPR: Erase all my data & wallets.")]
    Forget,
}
