use teloxide::utils::command::BotCommands;

#[derive(BotCommands, Clone, std::fmt::Debug)]
#[command(rename_rule = "lowercase", description = "Kaspa Node Bot Commands:")]
pub enum Command {
    #[command(description = "Start the bot and show help.")]
    Start,
    #[command(description = "Show the ultimate guide and features.")]
    Help,
    #[command(description = "Add a wallet: /add <address>")]
    Add(String),
    #[command(description = "Remove a wallet: /remove <address>")]
    Remove(String),
    #[command(description = "List tracked wallets.")]
    List,
    #[command(description = "Show full node and network stats.")]
    Network,
    #[command(description = "Show BlockDAG details.")]
    Dag,
    #[command(description = "Check Live Balance & UTXOs.")]
    Balance,
    #[command(description = "Estimate your solo-mining hashrate.")]
    Miner,
    #[command(description = "Count your unspent mined blocks.")]
    Blocks,
    #[command(description = "Check KAS Price.")]
    Price,
    #[command(description = "Check Market Cap.")]
    Market,
    #[command(description = "Check Supply.")]
    Supply,
    #[command(description = "Check Mempool Fees.")]
    Fees,
    #[command(description = "Admin Analytics")]
    Stats,
    #[command(description = "Admin Command")]
    Sys,
    #[command(description = "Admin Command")]
    Pause,
    #[command(description = "Admin Command")]
    Resume,
    #[command(description = "Admin Command")]
    Restart,
    #[command(description = "Admin Command")]
    Broadcast(String),
    #[command(description = "Admin Command")]
    Logs,
    #[command(description = "Support the Developer")]
    Donate,
    #[command(description = "Admin Command: Teach AI")]
    Learn(String),
    #[command(description = "Auto-fetch latest official Kaspa news")]
    AutoLearn,
}
