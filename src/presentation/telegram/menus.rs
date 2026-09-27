use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

pub struct TelegramMenus;

impl TelegramMenus {
    pub fn main_menu_markup() -> InlineKeyboardMarkup {
        InlineKeyboardMarkup::new(vec![
            vec![
                InlineKeyboardButton::callback("💰 Balance", "cmd_balance"),
                InlineKeyboardButton::callback("👛 Wallets", "cmd_wallets"),
            ],
            vec![
                InlineKeyboardButton::callback("⛏️ Mining", "menu_mining"),
                InlineKeyboardButton::callback("🌐 Network", "menu_network"),
            ],
            vec![
                InlineKeyboardButton::callback("📈 Market", "cmd_market"),
                InlineKeyboardButton::callback("☰ More", "menu_more"),
            ],
        ])
    }

    pub fn home_menu_markup(is_admin: bool) -> InlineKeyboardMarkup {
        let mut rows = Self::main_menu_markup().inline_keyboard;
        if is_admin {
            rows.push(vec![InlineKeyboardButton::callback(
                "🛡️ Admin",
                "cmd_admin",
            )]);
        }
        InlineKeyboardMarkup::new(rows)
    }

    pub fn mining_menu_markup() -> InlineKeyboardMarkup {
        InlineKeyboardMarkup::new(vec![
            vec![
                InlineKeyboardButton::callback("⚡ Hashrate", "cmd_miner"),
                InlineKeyboardButton::callback("🧱 Mined Blocks", "cmd_blocks"),
            ],
            vec![InlineKeyboardButton::callback("⬅️ Back", "cmd_start")],
        ])
    }

    pub fn network_menu_markup() -> InlineKeyboardMarkup {
        InlineKeyboardMarkup::new(vec![
            vec![
                InlineKeyboardButton::callback("🩺 Network Health", "cmd_network"),
                InlineKeyboardButton::callback("📊 BlockDAG", "cmd_dag"),
            ],
            vec![
                InlineKeyboardButton::callback("⛽ Fees", "cmd_fees"),
                InlineKeyboardButton::callback("🪙 Supply", "cmd_supply"),
            ],
            vec![InlineKeyboardButton::callback("⬅️ Back", "cmd_start")],
        ])
    }

    pub fn more_menu_markup() -> InlineKeyboardMarkup {
        InlineKeyboardMarkup::new(vec![
            vec![
                InlineKeyboardButton::callback("📚 Help", "cmd_help"),
                InlineKeyboardButton::callback("❤️ Donate", "cmd_donate"),
            ],
            vec![InlineKeyboardButton::callback(
                "🗑️ Delete My Data",
                "confirm_forget_all",
            )],
            vec![InlineKeyboardButton::callback("⬅️ Back", "cmd_start")],
        ])
    }

    pub fn admin_menu_markup() -> InlineKeyboardMarkup {
        InlineKeyboardMarkup::new(vec![
            vec![
                InlineKeyboardButton::callback("🩺 Overview", "admin_overview"),
                InlineKeyboardButton::callback("⚙️ Operations", "admin_operations"),
            ],
            vec![
                InlineKeyboardButton::callback("📣 Alerts", "admin_alerts"),
                InlineKeyboardButton::callback("🔧 Diagnostics", "admin_diagnostics"),
            ],
            vec![InlineKeyboardButton::callback(
                "⚙️ Settings",
                "admin_settings",
            )],
            vec![InlineKeyboardButton::callback("⬅️ Main Menu", "cmd_start")],
        ])
    }

    pub fn admin_operations_markup(
        monitoring_enabled: bool,
        maintenance_enabled: bool,
    ) -> InlineKeyboardMarkup {
        let monitoring = if monitoring_enabled {
            InlineKeyboardButton::callback("⏸ Pause Monitoring", "cmd_pause")
        } else {
            InlineKeyboardButton::callback("▶️ Resume Monitoring", "cmd_resume")
        };
        let maintenance = if maintenance_enabled {
            InlineKeyboardButton::callback("🚧 Disable Maintenance", "btn_toggle_MAINTENANCE_MODE")
        } else {
            InlineKeyboardButton::callback("🚧 Enable Maintenance", "btn_toggle_MAINTENANCE_MODE")
        };
        InlineKeyboardMarkup::new(vec![
            vec![monitoring],
            vec![maintenance],
            vec![
                InlineKeyboardButton::callback("ℹ️ Service Information", "cmd_restart_info"),
                InlineKeyboardButton::callback("🧹 Maintenance Tools", "admin_maintenance_tools"),
            ],
            vec![InlineKeyboardButton::callback("⬅️ Admin", "cmd_admin")],
        ])
    }

    pub fn admin_alerts_markup(alerts_enabled: Option<bool>) -> InlineKeyboardMarkup {
        let mut rows = Vec::new();
        if let Some(enabled) = alerts_enabled {
            rows.push(vec![if enabled {
                InlineKeyboardButton::callback("🔕 Disable Alerts", "cmd_mute_alerts")
            } else {
                InlineKeyboardButton::callback("🔔 Enable Alerts", "cmd_unmute_alerts")
            }]);
        }
        rows.push(vec![InlineKeyboardButton::callback(
            "📬 Delivery Details",
            "cmd_delivery",
        )]);
        rows.push(vec![InlineKeyboardButton::callback(
            "⬅️ Admin",
            "cmd_admin",
        )]);
        InlineKeyboardMarkup::new(rows)
    }

    pub fn admin_diagnostics_markup() -> InlineKeyboardMarkup {
        InlineKeyboardMarkup::new(vec![
            vec![
                InlineKeyboardButton::callback("🚨 Errors", "cmd_errors"),
                InlineKeyboardButton::callback("📜 Events", "cmd_events"),
            ],
            vec![
                InlineKeyboardButton::callback("📬 Delivery", "cmd_delivery"),
                InlineKeyboardButton::callback("🗄 Database", "cmd_db_diag"),
            ],
            vec![InlineKeyboardButton::callback("📄 Logs", "cmd_logs")],
            vec![InlineKeyboardButton::callback("⬅️ Admin", "cmd_admin")],
        ])
    }

    pub fn admin_settings_markup(
        monitoring_enabled: bool,
        maintenance_enabled: bool,
    ) -> InlineKeyboardMarkup {
        let monitoring = if monitoring_enabled {
            InlineKeyboardButton::callback("⏸ Pause Monitoring", "cmd_pause")
        } else {
            InlineKeyboardButton::callback("▶️ Resume Monitoring", "cmd_resume")
        };
        let maintenance = if maintenance_enabled {
            InlineKeyboardButton::callback("🚧 Disable Maintenance", "btn_toggle_MAINTENANCE_MODE")
        } else {
            InlineKeyboardButton::callback("🚧 Enable Maintenance", "btn_toggle_MAINTENANCE_MODE")
        };
        InlineKeyboardMarkup::new(vec![
            vec![monitoring],
            vec![maintenance],
            vec![InlineKeyboardButton::callback("⬅️ Admin", "cmd_admin")],
        ])
    }

    pub fn admin_maintenance_tools_markup() -> InlineKeyboardMarkup {
        InlineKeyboardMarkup::new(vec![
            vec![InlineKeyboardButton::callback(
                "🧹 Cleanup Old Events",
                "cmd_cleanup_events",
            )],
            vec![InlineKeyboardButton::callback(
                "⬅️ Operations",
                "admin_operations",
            )],
        ])
    }

    pub fn wallet_menu_markup() -> InlineKeyboardMarkup {
        InlineKeyboardMarkup::new(vec![
            vec![InlineKeyboardButton::callback(
                "📋 My Wallets",
                "cmd_wallets",
            )],
            vec![
                InlineKeyboardButton::callback("➕ Add Wallet", "cmd_add_wallet"),
                InlineKeyboardButton::callback("➖ Remove Wallet", "cmd_remove_wallets"),
            ],
            vec![InlineKeyboardButton::callback(
                "🗑️ Clear Wallets",
                "confirm_forget_wallets",
            )],
            vec![InlineKeyboardButton::callback("⬅️ Back", "cmd_start")],
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

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(markup: InlineKeyboardMarkup) -> Vec<String> {
        markup
            .inline_keyboard
            .into_iter()
            .flatten()
            .map(|button| button.text)
            .collect()
    }

    #[test]
    fn public_home_has_exact_progressive_disclosure_surface() {
        assert_eq!(
            labels(TelegramMenus::main_menu_markup()),
            [
                "💰 Balance",
                "👛 Wallets",
                "⛏️ Mining",
                "🌐 Network",
                "📈 Market",
                "☰ More"
            ]
        );
    }

    #[test]
    fn admin_top_level_has_five_areas_plus_back() {
        assert_eq!(
            labels(TelegramMenus::admin_menu_markup()),
            [
                "🩺 Overview",
                "⚙️ Operations",
                "📣 Alerts",
                "🔧 Diagnostics",
                "⚙️ Settings",
                "⬅️ Main Menu"
            ]
        );
    }

    #[test]
    fn monitoring_control_is_state_aware() {
        let enabled = labels(TelegramMenus::admin_operations_markup(true, false));
        let disabled = labels(TelegramMenus::admin_operations_markup(false, false));
        assert!(enabled.contains(&"⏸ Pause Monitoring".to_string()));
        assert!(!enabled.contains(&"▶️ Resume Monitoring".to_string()));
        assert!(disabled.contains(&"▶️ Resume Monitoring".to_string()));
        assert!(!disabled.contains(&"⏸ Pause Monitoring".to_string()));
    }

    #[test]
    fn alert_control_is_state_aware() {
        assert_eq!(
            labels(TelegramMenus::admin_alerts_markup(Some(true)))[0],
            "🔕 Disable Alerts"
        );
        assert_eq!(
            labels(TelegramMenus::admin_alerts_markup(Some(false)))[0],
            "🔔 Enable Alerts"
        );
    }

    #[test]
    fn memory_cleaner_is_not_exposed_in_any_menu() {
        let mut all = Vec::new();
        all.extend(labels(TelegramMenus::main_menu_markup()));
        all.extend(labels(TelegramMenus::admin_menu_markup()));
        all.extend(labels(TelegramMenus::admin_operations_markup(true, false)));
        all.extend(labels(TelegramMenus::admin_settings_markup(true, false)));
        all.extend(labels(TelegramMenus::admin_maintenance_tools_markup()));
        assert!(!all.iter().any(|label| label.contains("Memory Cleaner")));
    }
}
