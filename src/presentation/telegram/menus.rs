use teloxide::types::{InlineButtonStyle, InlineKeyboardButton, InlineKeyboardMarkup};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Language {
    En,
    Ar,
}

impl Language {
    pub fn from_db(value: &str) -> Self {
        if value == "ar" { Self::Ar } else { Self::En }
    }
    pub fn label<'a>(self, en: &'a str, ar: &'a str) -> &'a str {
        if self == Self::Ar { ar } else { en }
    }
}

pub async fn user_language(pool: &sqlx::PgPool, user_id: i64) -> Language {
    let result = sqlx::query_scalar::<_, String>(
        "SELECT language FROM public.telegram_user_language WHERE user_id=$1",
    )
    .bind(user_id)
    .fetch_optional(pool)
    .await;
    match result {
        Ok(Some(code)) => Language::from_db(&code),
        Ok(None) => Language::En,
        Err(err) => {
            tracing::warn!(error=%err, "Language preference read unavailable; using English");
            Language::En
        }
    }
}

pub const DONATION_KAS_ADDRESS: &str =
    "kaspa:qz0yqq8z3twwgg7lq2mjzg6w4edqys45w2wslz7tym2tc6s84580vvx9zr44g";
pub const DONATION_EXPLORER_URL: &str = "https://kaspa.stream/addresses/kaspa%3Aqz0yqq8z3twwgg7lq2mjzg6w4edqys45w2wslz7tym2tc6s84580vvx9zr44g";

pub struct TelegramMenus;

impl TelegramMenus {
    pub fn main_menu_markup() -> InlineKeyboardMarkup {
        Self::home_markup(Language::En, false)
    }

    pub fn admin_menu_markup() -> InlineKeyboardMarkup {
        Self::home_markup(Language::En, true)
    }

    pub fn home_markup(lang: Language, admin: bool) -> InlineKeyboardMarkup {
        // Five public destinations, including Donate directly on Home.
        // Balance and mined blocks belong inside the selected Wallet.
        let mut rows = vec![
            vec![
                Self::key(lang, "👛 Wallets", "👛 المحافظ", "cmd_wallets"),
                Self::key(lang, "💹 Market", "💹 السوق", "cmd_price"),
            ],
            vec![
                Self::key(lang, "🌐 Network", "🌐 الشبكة", "cmd_network"),
                Self::key(lang, "⚙️ More", "⚙️ المزيد", "ui:more"),
            ],
            vec![Self::key(lang, "❤️ Donate", "❤️ التبرع", "cmd_donate")],
        ];
        if admin {
            rows.push(vec![Self::key(lang, "🛡️ Admin", "🛡️ الإدارة", "ui:admin")]);
        }
        InlineKeyboardMarkup::new(rows)
    }

    fn key(lang: Language, en: &str, ar: &str, action: &str) -> InlineKeyboardButton {
        let button = InlineKeyboardButton::callback(lang.label(en, ar), action);
        // Use native styles sparingly: primary navigation, success and danger.
        match action {
            "ui:home" | "ui:network" | "cmd_wallets" | "cmd_price" | "ui:admin" | "ui:lang:ar"
            | "ui:lang:en" => button.with_style(InlineButtonStyle::Primary),
            "cmd_resume" | "cmd_unmute_alerts" => button.with_style(InlineButtonStyle::Success),
            "cmd_pause" | "cmd_mute_alerts" | "cmd_cleanup_events" => {
                button.with_style(InlineButtonStyle::Danger)
            }
            _ => button,
        }
    }

    pub fn mining_markup(lang: Language) -> InlineKeyboardMarkup {
        InlineKeyboardMarkup::new(vec![
            vec![
                Self::key(lang, "⛏️ Hashrate", "⛏️ معدل التعدين", "cmd_miner"),
                Self::key(lang, "🧱 Blocks", "🧱 الكتل", "cmd_blocks"),
            ],
            vec![Self::key(lang, "🏠 Home", "🏠 الرئيسية", "ui:home")],
        ])
    }

    // Keep ui:market legacy callback active, but present network only.
    pub fn market_markup(lang: Language) -> InlineKeyboardMarkup {
        InlineKeyboardMarkup::new(vec![
            vec![
                Self::key(lang, "🩺 Network Health", "🩺 حالة الشبكة", "cmd_network"),
                Self::key(lang, "📊 BlockDAG", "📊 BlockDAG", "cmd_dag"),
            ],
            vec![
                Self::key(lang, "⛽ Fees", "⛽ الرسوم", "cmd_fees"),
                Self::key(lang, "🪙 Supply", "🪙 المعروض", "cmd_supply"),
            ],
            vec![Self::key(lang, "🏠 Home", "🏠 الرئيسية", "ui:home")],
        ])
    }

    pub fn more_markup(lang: Language) -> InlineKeyboardMarkup {
        // Wallet changes live within the Wallets section, not in More.
        InlineKeyboardMarkup::new(vec![
            vec![
                Self::key(lang, "🌍 Language", "🌍 اللغة", "ui:language"),
                Self::key(lang, "❔ Help", "❔ المساعدة", "cmd_help"),
            ],
            vec![Self::key(lang, "🏠 Home", "🏠 الرئيسية", "ui:home")],
        ])
    }

    /// Explorer button is visible only on the Support page, not on generic More.
    pub fn support_markup(lang: Language) -> InlineKeyboardMarkup {
        // Donation is opened directly from Home; return there, not to More.
        let mut markup = InlineKeyboardMarkup::new(vec![vec![Self::key(
            lang,
            "🏠 Home",
            "🏠 الرئيسية",
            "ui:home",
        )]]);
        let explorer = reqwest::Url::parse(DONATION_EXPLORER_URL)
            .expect("fixed Kaspa.stream explorer URL is valid");
        markup.inline_keyboard.insert(
            0,
            vec![InlineKeyboardButton::url(
                lang.label(
                    "🔎 Open donation address on Kaspa.stream",
                    "🔎 عرض عنوان التبرع في Kaspa.stream",
                ),
                explorer,
            )],
        );
        markup
    }

    pub fn owner_markup(lang: Language) -> InlineKeyboardMarkup {
        InlineKeyboardMarkup::new(vec![
            vec![
                Self::key(lang, "🩺 Health", "🩺 الصحة", "cmd_health"),
                Self::key(lang, "📊 Stats", "📊 الإحصاءات", "cmd_stats"),
            ],
            vec![
                Self::key(lang, "⚙️ Settings", "⚙️ الإعدادات", "cmd_settings"),
                Self::key(lang, "📬 Delivery", "📬 التنبيهات", "cmd_delivery"),
            ],
            vec![
                Self::key(lang, "🗄️ DB", "🗄️ قاعدة البيانات", "cmd_db_diag"),
                Self::key(lang, "📜 Logs", "📜 السجلات", "cmd_logs"),
            ],
            vec![Self::key(
                lang,
                "🛠️ Operations",
                "🛠️ العمليات",
                "ui:admin_ops",
            )],
            vec![Self::key(
                lang,
                "👥 Subscribers",
                "👥 المشتركون",
                "ui:admin_subs:0",
            )],
            vec![Self::key(lang, "🏠 Home", "🏠 الرئيسية", "ui:home")],
        ])
    }

    pub fn owner_ops_markup(lang: Language) -> InlineKeyboardMarkup {
        InlineKeyboardMarkup::new(vec![
            vec![
                Self::key(lang, "⏸️ Pause", "⏸️ إيقاف", "cmd_pause"),
                Self::key(lang, "▶️ Resume", "▶️ استئناف", "cmd_resume"),
            ],
            vec![
                Self::key(
                    lang,
                    "🔕 Mute Alerts",
                    "🔕 كتم التنبيهات",
                    "cmd_mute_alerts",
                ),
                Self::key(lang, "🔔 Unmute", "🔔 تشغيل التنبيهات", "cmd_unmute_alerts"),
            ],
            vec![
                Self::key(lang, "🚨 Errors", "🚨 الأخطاء", "cmd_errors"),
                Self::key(lang, "📜 Events", "📜 الأحداث", "cmd_events"),
            ],
            vec![
                Self::key(lang, "⚙️ System", "⚙️ النظام", "cmd_sys"),
                Self::key(
                    lang,
                    "📣 Alert Status",
                    "📣 حالة التنبيهات",
                    "cmd_alerts_status",
                ),
            ],
            vec![Self::key(
                lang,
                "🧹 Cleanup",
                "🧹 تنظيف",
                "cmd_cleanup_events",
            )],
            vec![Self::key(lang, "🔙 Admin", "🔙 الإدارة", "ui:admin")],
        ])
    }

    pub fn language_markup(lang: Language) -> InlineKeyboardMarkup {
        InlineKeyboardMarkup::new(vec![
            vec![
                Self::key(lang, "🇬🇧 English", "🇬🇧 الإنجليزية", "ui:lang:en"),
                Self::key(lang, "🇸🇦 Arabic", "🇸🇦 العربية", "ui:lang:ar"),
            ],
            vec![Self::key(lang, "🔙 More", "🔙 المزيد", "ui:more")],
        ])
    }

    /// Shared navigation for data pages: refresh, contextual back, home.
    pub fn navigation_markup(lang: Language, refresh: &str, back: &str) -> InlineKeyboardMarkup {
        let mut row = vec![Self::key(lang, "🔄 Refresh", "🔄 تحديث", refresh)];
        if back != "ui:home" {
            row.push(Self::key(lang, "↩️ Back", "↩️ رجوع", back));
        }
        row.push(Self::key(lang, "🏠 Home", "🏠 الرئيسية", "ui:home"));
        InlineKeyboardMarkup::new(vec![row])
    }

    pub fn wallet_menu_markup_lang(lang: Language) -> InlineKeyboardMarkup {
        InlineKeyboardMarkup::new(vec![
            vec![
                Self::key(lang, "➕ Add Wallet", "➕ إضافة محفظة", "cmd_add_wallet"),
                Self::key(
                    lang,
                    "➖ Remove Wallet",
                    "➖ حذف محفظة",
                    "cmd_remove_wallets",
                ),
            ],
            vec![Self::key(
                lang,
                "🗑️ Clear Wallets",
                "🗑️ مسح المحافظ",
                "confirm_forget_wallets",
            )],
            vec![Self::key(lang, "🏠 Home", "🏠 الرئيسية", "ui:home")],
        ])
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
            vec![InlineKeyboardButton::callback("🔙 Main Menu", "ui:home")],
        ])
    }

    pub fn confirm_wallet_clear_markup() -> InlineKeyboardMarkup {
        InlineKeyboardMarkup::new(vec![
            vec![
                InlineKeyboardButton::callback("✅ Yes, clear wallets", "do_forget_wallets"),
                InlineKeyboardButton::callback("❌ Cancel", "cancel_action"),
            ],
            vec![InlineKeyboardButton::callback("🔙 Main Menu", "ui:home")],
        ])
    }

    pub fn confirm_full_delete_markup() -> InlineKeyboardMarkup {
        InlineKeyboardMarkup::new(vec![
            vec![
                InlineKeyboardButton::callback("🚨 Yes, delete my data", "do_forget_all"),
                InlineKeyboardButton::callback("❌ Cancel", "cancel_action"),
            ],
            vec![InlineKeyboardButton::callback("🔙 Main Menu", "ui:home")],
        ])
    }
}

#[cfg(test)]
mod compact_menu_tests {
    use super::{Language, TelegramMenus};
    #[test]
    fn main_menu_has_five_actions_and_separate_owner() {
        let ordinary = TelegramMenus::home_markup(Language::En, false);
        assert_eq!(
            ordinary.inline_keyboard.iter().map(Vec::len).sum::<usize>(),
            5
        );
        let owner = TelegramMenus::home_markup(Language::Ar, true);
        assert_eq!(owner.inline_keyboard.iter().map(Vec::len).sum::<usize>(), 6);
        assert!(
            owner
                .inline_keyboard
                .iter()
                .flatten()
                .any(|b| b.text.contains("إدارة"))
        );
    }
    #[test]
    fn wallet_first_home_has_exact_destinations() {
        let labels: Vec<String> = TelegramMenus::home_markup(Language::En, false)
            .inline_keyboard
            .into_iter()
            .flatten()
            .map(|button| button.text)
            .collect();
        assert_eq!(
            labels,
            [
                "👛 Wallets",
                "💹 Market",
                "🌐 Network",
                "⚙️ More",
                "❤️ Donate"
            ]
        );
    }

    #[test]
    fn more_menu_keeps_wallet_actions_out() {
        let labels: Vec<String> = TelegramMenus::more_markup(Language::En)
            .inline_keyboard
            .into_iter()
            .flatten()
            .map(|button| button.text)
            .collect();
        assert!(!labels.iter().any(|value| value.contains("Wallet")));
        assert!(labels.contains(&"🌍 Language".to_string()));
        assert!(!labels.iter().any(|value| value.contains("Donate")));
    }

    #[test]
    fn language_defaults_to_english() {
        assert_eq!(Language::from_db("unknown"), Language::En);
        assert_eq!(Language::from_db("ar"), Language::Ar);
    }
}

#[cfg(test)]
mod native_telegram_styles_tests {
    use super::*;
    #[test]
    fn native_styles_are_serialized_only_when_present() {
        let markup = TelegramMenus::home_markup(Language::Ar, true);
        let value = serde_json::to_value(&markup).unwrap();
        assert_eq!(value["inline_keyboard"][0][0]["style"], "primary");
        assert_eq!(
            value["inline_keyboard"][1][1]["style"],
            serde_json::Value::Null
        );
        assert_eq!(
            value["inline_keyboard"][2][0]["style"],
            serde_json::Value::Null
        );
        assert_eq!(value["inline_keyboard"][3][0]["style"], "primary");
        assert_eq!(
            value["inline_keyboard"][0][0]["callback_data"],
            "cmd_wallets"
        );
    }
}

#[cfg(test)]
mod op036_unified_navigation_tests {
    use super::{Language, TelegramMenus};

    #[test]
    fn data_pages_avoid_duplicate_back_home_actions() {
        for refresh in ["refresh_network", "refresh_market"] {
            let rows =
                TelegramMenus::navigation_markup(Language::Ar, refresh, "ui:home").inline_keyboard;
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0].len(), 2);
            assert_eq!(rows[0][0].text, "🔄 تحديث");
            assert_eq!(rows[0][1].text, "🏠 الرئيسية");
        }
        let nested = TelegramMenus::navigation_markup(Language::Ar, "refresh_dag", "cmd_network");
        assert_eq!(nested.inline_keyboard[0].len(), 3);
        assert_eq!(nested.inline_keyboard[0][1].text, "↩️ رجوع");
    }

    #[test]
    fn wallet_management_uses_user_selected_language() {
        let ar = TelegramMenus::wallet_menu_markup_lang(Language::Ar);
        assert!(ar.inline_keyboard[0][0].text.contains("إضافة محفظة"));
        assert!(ar.inline_keyboard[0][1].text.contains("حذف محفظة"));
        assert!(ar.inline_keyboard[2][0].text.contains("الرئيسية"));
        let en = TelegramMenus::wallet_menu_markup_lang(Language::En);
        assert!(en.inline_keyboard[0][0].text.contains("Add Wallet"));
    }
}

#[cfg(test)]
mod op044_donation_explorer_tests {
    use super::{DONATION_EXPLORER_URL, DONATION_KAS_ADDRESS, Language, TelegramMenus};
    #[test]
    fn donation_explorer_button_targets_exact_donation_wallet() {
        let suffix = DONATION_KAS_ADDRESS.strip_prefix("kaspa:").unwrap();
        assert_eq!(
            DONATION_EXPLORER_URL,
            format!("https://kaspa.stream/addresses/kaspa%3A{suffix}")
        );
        for lang in [Language::En, Language::Ar] {
            let support = TelegramMenus::support_markup(lang);
            let buttons = support.inline_keyboard;
            assert!(buttons[0][0].text.contains("Kaspa.stream"));
            let json = serde_json::to_string(&buttons[0][0]).unwrap();
            assert!(json.contains("https://kaspa.stream/addresses/kaspa%3A"));
            let plain = TelegramMenus::more_markup(lang);
            assert!(
                !serde_json::to_string(&plain)
                    .unwrap()
                    .contains("kaspa.stream")
            );
        }
    }
}

#[cfg(test)]
mod op005_donate_home_tests {
    use super::{Language, TelegramMenus};

    #[test]
    fn donation_is_present_exactly_once_on_home_for_en_ar_and_admin() {
        for lang in [Language::En, Language::Ar] {
            for admin in [false, true] {
                let home = TelegramMenus::home_markup(lang, admin);
                let donate: Vec<_> = home
                    .inline_keyboard
                    .iter()
                    .flatten()
                    .filter(|b| {
                        serde_json::to_value(b)
                            .ok()
                            .and_then(|v| v["callback_data"].as_str().map(|s| s == "cmd_donate"))
                            .unwrap_or(false)
                    })
                    .collect();
                assert_eq!(donate.len(), 1, "Donate must be unique on Home");
                assert_eq!(donate[0].text, lang.label("❤️ Donate", "❤️ التبرع"));
                assert_eq!(home.inline_keyboard[2].len(), 1);
                assert_eq!(home.inline_keyboard[2][0].text, donate[0].text);
                assert_eq!(home.inline_keyboard.len(), if admin { 4 } else { 3 });
            }
        }
    }

    #[test]
    fn more_has_no_donate_and_support_returns_to_home_with_explorer() {
        for lang in [Language::En, Language::Ar] {
            let more = serde_json::to_value(TelegramMenus::more_markup(lang)).unwrap();
            assert!(!more.to_string().contains("cmd_donate"));
            let support = serde_json::to_value(TelegramMenus::support_markup(lang)).unwrap();
            let rows = support["inline_keyboard"].as_array().unwrap();
            assert_eq!(rows.len(), 2);
            assert!(
                rows[0][0]["url"]
                    .as_str()
                    .unwrap()
                    .starts_with("https://kaspa.stream/addresses/kaspa%3A")
            );
            assert_eq!(rows[1][0]["callback_data"], "ui:home");
        }
    }
}
