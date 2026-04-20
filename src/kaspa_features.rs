use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

/// 🟢 PUBLIC USER MENU: Modern UI with Categorized Grid Layout
pub fn main_menu_markup() -> InlineKeyboardMarkup {
    let rows = vec![
        // --- 💰 Wallet & Mining ---
        vec![
            InlineKeyboardButton::callback("💰 My Balances", "cmd_balance"),
            InlineKeyboardButton::callback("💼 Tracked Wallets", "cmd_list"),
        ],
        vec![
            InlineKeyboardButton::callback("⛏️ My Hashrate", "cmd_miner"),
            InlineKeyboardButton::callback("🧱 Mined Blocks", "cmd_blocks"),
        ],
        // --- 📈 Market & Price ---
        vec![
            InlineKeyboardButton::callback("💵 KAS Price", "cmd_price"),
            InlineKeyboardButton::callback("📈 Market Data", "cmd_market"),
        ],
        // --- 🌐 Network & Blockchain ---
        vec![
            InlineKeyboardButton::callback("🌐 Network Stats", "cmd_network"),
            InlineKeyboardButton::callback("📦 BlockDAG Details", "cmd_dag"),
        ],
        vec![
            InlineKeyboardButton::callback("🪙 Coin Supply", "cmd_supply"),
            InlineKeyboardButton::callback("⛽ Mempool Fees", "cmd_fees"),
        ],
        // --- 🆘 Help & Support ---
        vec![
            InlineKeyboardButton::callback("❤️ Support Developer", "cmd_donate"),
        ],
    ];

    InlineKeyboardMarkup::new(rows)
}

/// 🔴 ADMIN TERMINAL: Enterprise Command Center
pub fn admin_menu_markup() -> InlineKeyboardMarkup {
    let rows = vec![
        // --- ⚙️ SYSTEM & CONTROL ---
        vec![InlineKeyboardButton::callback("─── ⚙️ SYSTEM CONTROL ⚙️ ───", "none")],
        vec![
            InlineKeyboardButton::callback("⚙️ Enterprise Settings", "cmd_settings"),
            InlineKeyboardButton::callback("📊 Global Analytics", "cmd_stats"),
        ],
        vec![
            InlineKeyboardButton::callback("🖥️ Hardware Monitor", "cmd_sys"),
            InlineKeyboardButton::callback("📜 View Bot Logs", "cmd_logs"),
        ],
        
        // --- 🛠️ CORE OPERATIONS ---
        vec![InlineKeyboardButton::callback("─── 🛠️ OPERATIONS ───", "none")],
        vec![
            InlineKeyboardButton::callback("🔄 Global Node Sync", "admin_sync_blocks"),
        ],
        vec![
            InlineKeyboardButton::callback("⏸️ Pause Engine", "cmd_pause"),
            InlineKeyboardButton::callback("▶️ Resume Engine", "cmd_resume"),
        ],
        vec![
            InlineKeyboardButton::callback("⚠️ Restart System", "cmd_restart"),
        ],

        // --- 👤 PUBLIC SHORTCUTS ---
        vec![InlineKeyboardButton::callback("─── 👤 PUBLIC FEATURES ───", "none")],
        vec![
            InlineKeyboardButton::callback("💰 Balances", "cmd_balance"),
            InlineKeyboardButton::callback("💼 Wallets", "cmd_list"),
            InlineKeyboardButton::callback("🌐 Network", "cmd_network"),
        ],
    ];

    InlineKeyboardMarkup::new(rows)
}

// ==============================================================================
// FORMATTING UTILITIES
// ==============================================================================

pub fn format_difficulty(val: f64) -> String {
    if val <= 0.0 {
        return "0.00".to_string();
    }
    if val >= 1e15 {
        format!("{:.2} P", val / 1e15)
    } else if val >= 1e12 {
        format!("{:.2} T", val / 1e12)
    } else if val >= 1e9 {
        format!("{:.2} G", val / 1e9)
    } else {
        format!("{:.2}", val)
    }
}

pub fn format_hashrate(h: f64) -> String {
    if h >= 1e15 {
        format!("{:.2} PH/s", h / 1e15)
    } else if h >= 1e12 {
        format!("{:.2} TH/s", h / 1e12)
    } else if h >= 1e9 {
        format!("{:.2} GH/s", h / 1e9)
    } else if h >= 1e6 {
        format!("{:.2} MH/s", h / 1e6)
    } else {
        format!("{:.2} H/s", h)
    }
}
