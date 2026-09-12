use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

pub struct TelegramMenus;

impl TelegramMenus {
    pub fn main_menu_markup() -> InlineKeyboardMarkup {
        let row1 = vec![
            InlineKeyboardButton::callback("💰 Balance", "cmd_balance"),
            InlineKeyboardButton::callback("👛 Wallets", "cmd_wallets"),
            InlineKeyboardButton::callback("⛏️ Hashrate", "cmd_miner"),
        ];

        let row2 = vec![
            InlineKeyboardButton::callback("🧱 Blocks", "cmd_blocks"),
            InlineKeyboardButton::callback("📈 Market", "cmd_market"),
            InlineKeyboardButton::callback("🛠️ Network", "cmd_network"),
        ];

        let row3 = vec![
            InlineKeyboardButton::callback("📊 DAG", "cmd_dag"),
            InlineKeyboardButton::callback("⛽ Fees", "cmd_fees"),
            InlineKeyboardButton::callback("🪙 Supply", "cmd_supply"),
        ];

        let row4 = vec![
            InlineKeyboardButton::callback("❤️ Donate", "cmd_donate"),
            InlineKeyboardButton::callback("➕ Add Wallet", "cmd_add_wallet"),
            InlineKeyboardButton::callback("➖ Remove Wallet", "cmd_remove_wallets"),
        ];

        InlineKeyboardMarkup::new(vec![row1, row2, row3, row4])
    }

    pub fn admin_menu_markup() -> InlineKeyboardMarkup {
        let mut rows = Self::main_menu_markup().inline_keyboard;

        let divider = vec![InlineKeyboardButton::callback(
            "🛡️ Admin Panel",
            "cmd_ignore",
        )];

        let admin_row1 = vec![
            InlineKeyboardButton::callback("🩺 Health", "cmd_health"),
            InlineKeyboardButton::callback("⚙️ System", "cmd_sys"),
            InlineKeyboardButton::callback("📊 Stats", "cmd_stats"),
        ];

        let admin_row2 = vec![
            InlineKeyboardButton::callback("⚙️ Settings", "cmd_settings"),
            InlineKeyboardButton::callback("⏸️ Pause", "cmd_pause"),
            InlineKeyboardButton::callback("▶️ Resume", "cmd_resume"),
        ];

        let admin_row3 = vec![
            InlineKeyboardButton::callback("ℹ️ Restart Info", "cmd_restart_info"),
            InlineKeyboardButton::callback("🗄️ DB Diagnostics", "cmd_db_diag"),
            InlineKeyboardButton::callback("📜 Logs", "cmd_logs"),
        ];

        let admin_row4 = vec![
            InlineKeyboardButton::callback("📜 Events", "cmd_events"),
            InlineKeyboardButton::callback("🚨 Errors", "cmd_errors"),
            InlineKeyboardButton::callback("📬 Delivery", "cmd_delivery"),
        ];

        let admin_row5 = vec![InlineKeyboardButton::callback(
            "🧹 Cleanup Events",
            "cmd_cleanup_events",
        )];

        let admin_alerts_row = vec![
            InlineKeyboardButton::callback("🔕 Stop Alerts", "cmd_mute_alerts"),
            InlineKeyboardButton::callback("🔔 Resume Alerts", "cmd_unmute_alerts"),
            InlineKeyboardButton::callback("📣 Alert Status", "cmd_alerts_status"),
        ];

        let admin_delete_row = vec![InlineKeyboardButton::callback(
            "🚨 Delete My Data",
            "confirm_forget_all",
        )];

        rows.push(divider);
        rows.push(admin_row1);
        rows.push(admin_row2);
        rows.push(admin_row3);
        rows.push(admin_row4);
        rows.push(admin_row5);
        rows.push(admin_alerts_row);
        rows.push(admin_delete_row);

        InlineKeyboardMarkup::new(rows)
    }

    pub fn wallet_menu_markup() -> InlineKeyboardMarkup {
        InlineKeyboardMarkup::new(vec![
            vec![
                InlineKeyboardButton::callback("➕ Add Wallet", "cmd_add_wallet"),
                InlineKeyboardButton::callback("➖ Remove Wallet", "cmd_remove_wallets"),
            ],
            vec![InlineKeyboardButton::callback(
                "🗑️ Clear Wallets",
                "confirm_forget_wallets",
            )],
            vec![InlineKeyboardButton::callback("🔙 Main Menu", "cmd_start")],
        ])
    }

    pub fn confirm_wallet_clear_markup() -> InlineKeyboardMarkup {
        InlineKeyboardMarkup::new(vec![
            vec![
                InlineKeyboardButton::callback("✅ Yes, clear wallets", "do_forget_wallets"),
                InlineKeyboardButton::callback("❌ Cancel", "cancel_action"),
            ],
            vec![InlineKeyboardButton::callback("🔙 Main Menu", "cmd_start")],
        ])
    }

    pub fn confirm_full_delete_markup() -> InlineKeyboardMarkup {
        InlineKeyboardMarkup::new(vec![
            vec![
                InlineKeyboardButton::callback("🚨 Yes, delete my data", "do_forget_all"),
                InlineKeyboardButton::callback("❌ Cancel", "cancel_action"),
            ],
            vec![InlineKeyboardButton::callback("🔙 Main Menu", "cmd_start")],
        ])
    }
}
