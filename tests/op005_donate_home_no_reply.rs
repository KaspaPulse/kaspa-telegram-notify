use kaspa_pulse::presentation::telegram::menus::{Language, TelegramMenus};

#[test]
fn donate_is_on_the_start_menu_once_and_never_under_more() {
    for lang in [Language::En, Language::Ar] {
        for admin in [false, true] {
            let home = TelegramMenus::home_markup(lang, admin);
            let count = home
                .inline_keyboard
                .iter()
                .flatten()
                .filter(|button| {
                    serde_json::to_value(button).unwrap()["callback_data"] == "cmd_donate"
                })
                .count();
            assert_eq!(count, 1);
            assert_eq!(
                home.inline_keyboard[2][0].text,
                lang.label("❤️ Donate", "❤️ التبرع")
            );
        }
        let more = serde_json::to_string(&TelegramMenus::more_markup(lang)).unwrap();
        assert!(!more.contains("cmd_donate"));
        let support = TelegramMenus::support_markup(lang);
        assert_eq!(
            support.inline_keyboard[1][0].text,
            lang.label("🏠 Home", "🏠 الرئيسية")
        );
        let buttons_json = serde_json::to_string(&support).unwrap();
        assert!(buttons_json.contains("kaspa.stream"));
        assert!(buttons_json.contains("ui:home"));
    }
}

#[test]
fn no_bot_messages_ever_attach_reply_parameters_to_old_buttons() {
    let utils = include_str!("../src/utils.rs");
    let admin = include_str!("../src/presentation/telegram/handlers/admin.rs");
    let callback = include_str!("../src/presentation/telegram/handlers/mod.rs");
    for source in [utils, admin, callback] {
        assert!(!source.contains(".reply_parameters("));
        assert!(!source.contains(".reply_to_message_id("));
    }
    assert!(
        utils.contains(".edit_message_text("),
        "button navigation edits existing panel"
    );
    assert!(
        callback.contains("edit_callback_state"),
        "button state still edits original panel"
    );
}
