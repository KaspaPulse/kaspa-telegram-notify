use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

pub fn main_menu_markup() -> InlineKeyboardMarkup {
    let row1 = vec![
        InlineKeyboardButton::callback("💰 My Balances", "cmd_balance"),
        InlineKeyboardButton::callback("⛏️ My Hashrate", "cmd_miner"),
    ];

    let row2 = vec![
        InlineKeyboardButton::callback("🧱 Mined Blocks", "cmd_blocks"),
        InlineKeyboardButton::callback("💼 Tracked Wallets", "cmd_list"),
    ];

    let row3 = vec![
        InlineKeyboardButton::callback("🌐 Network Stats", "cmd_network"),
        InlineKeyboardButton::callback("💵 KAS Price", "cmd_price"),
    ];

    let row4 = vec![
        InlineKeyboardButton::callback("🪙 Coin Supply", "cmd_supply"),
        InlineKeyboardButton::callback("⛽ Mempool Fees", "cmd_fees"),
    ];

    let row5 = vec![
        InlineKeyboardButton::callback("📦 BlockDAG Details", "cmd_dag"),
        InlineKeyboardButton::callback("❤️ Support", "cmd_donate"),
    ];

    InlineKeyboardMarkup::new(vec![row1, row2, row3, row4, row5])
}

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
