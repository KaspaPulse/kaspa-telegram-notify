#![allow(dead_code)]
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup};

pub struct TelegramMenus;

impl TelegramMenus {
    pub fn main_menu_markup() -> InlineKeyboardMarkup {
        // 3 Columns: Main Menu
        let row1 = vec![
            InlineKeyboardButton::callback("💰 Balance", "cmd_balance"),
            InlineKeyboardButton::callback("📋 Wallets", "cmd_list"),
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
            InlineKeyboardButton::callback("🗑️ Clear Wallets", "cmd_forget_wallets"),
            InlineKeyboardButton::callback("🧠 Clear Chat", "cmd_forget_chat"),
        ];
        InlineKeyboardMarkup::new(vec![row1, row2, row3, row4])
    }

    pub fn admin_menu_markup() -> InlineKeyboardMarkup {
        let markup = Self::main_menu_markup();

        let divider = vec![InlineKeyboardButton::callback(
            "🛡️ ━━━━ SERVICE CONTROL CENTER ━━━━ 🛡️",
            "cmd_ignore",
        )];

        // 3 Columns: Admin Menu
        let admin_row1 = vec![
            InlineKeyboardButton::callback("⚙️ Sys Diag", "cmd_sys"),
            InlineKeyboardButton::callback("📊 Analytics", "cmd_stats"),
            InlineKeyboardButton::callback("🧠 Settings", "cmd_settings"),
        ];
        let admin_row2 = vec![
            InlineKeyboardButton::callback("🔄 Sync", "cmd_sync"),
            InlineKeyboardButton::callback("⏸️ Suspend", "cmd_pause"),
            InlineKeyboardButton::callback("▶️ Resume", "cmd_resume"),
        ];
        let admin_row3 = vec![
            InlineKeyboardButton::callback("🔄 Reboot", "cmd_restart"),
            InlineKeyboardButton::callback("🗄️ DB Diag", "cmd_db_diag"),
            InlineKeyboardButton::callback("🚨 GDPR Wipe", "cmd_forget_all"),
        ];

        let mut rows = markup.inline_keyboard;
        rows.push(divider);
        rows.push(admin_row1);
        rows.push(admin_row2);
        rows.push(admin_row3);

        InlineKeyboardMarkup::new(rows)
    }

    pub fn models_menu_markup() -> InlineKeyboardMarkup {
        // 3 Columns: AI Models Configuration
        let row1 = vec![
            InlineKeyboardButton::callback("🦙 Llama 3 (Groq)", "cmd_set_model_llama"),
            InlineKeyboardButton::callback("🐋 DeepSeek V2", "cmd_set_model_deepseek"),
        ];
        let row2 = vec![
            InlineKeyboardButton::callback("🧠 GPT-4o (OpenAI)", "cmd_set_model_gpt4"),
            InlineKeyboardButton::callback("🔮 Claude 3.5", "cmd_set_model_claude"),
        ];
        let row3 = vec![
            InlineKeyboardButton::callback("✨ Gemini Pro", "cmd_set_model_gemini"),
            InlineKeyboardButton::callback("🔙 Close", "cmd_ignore"),
        ];
        InlineKeyboardMarkup::new(vec![row1, row2, row3])
    }
}
