use kaspa_pulse::domain::models::{RequestIdentity, SensitiveAction};
use kaspa_pulse::presentation::telegram::handlers::admin_confirm::{
    action_from_admin_do_callback, confirmation_callback, sensitive_action_from_callback,
    sensitive_action_from_toggle_flag,
};

#[test]
fn sensitive_admin_callbacks_are_detected() {
    assert_eq!(
        sensitive_action_from_callback("cmd_pause"),
        Some(SensitiveAction::Pause)
    );
    assert_eq!(
        sensitive_action_from_callback("cmd_resume"),
        Some(SensitiveAction::Resume)
    );
    assert_eq!(sensitive_action_from_callback("cmd_restart_info"), None);
    assert_eq!(
        sensitive_action_from_callback("cmd_cleanup_events"),
        Some(SensitiveAction::CleanupEvents)
    );
    assert_eq!(
        sensitive_action_from_callback("confirm_forget_all"),
        Some(SensitiveAction::ForgetAll)
    );
}

#[test]
fn sensitive_toggle_flags_are_detected() {
    assert_eq!(
        sensitive_action_from_toggle_flag("MAINTENANCE"),
        Some(SensitiveAction::ToggleMaintenance)
    );
    assert_eq!(
        sensitive_action_from_toggle_flag("SYNC"),
        Some(SensitiveAction::ToggleLiveSync)
    );
    assert_eq!(
        sensitive_action_from_toggle_flag("MEMORY"),
        Some(SensitiveAction::ToggleMemoryCleaner)
    );
}

#[test]
fn confirmed_actions_rewrite_to_original_callbacks() {
    assert_eq!(SensitiveAction::Pause.execute_callback(), "do_pause");
    assert_eq!(
        SensitiveAction::ToggleMaintenance.execute_callback(),
        "btn_toggle_MAINTENANCE_MODE"
    );
    assert_eq!(
        SensitiveAction::ForgetAll.execute_callback(),
        "do_forget_all"
    );
    assert_ne!(SensitiveAction::Pause.execute_callback(), "cmd_pause");
    assert_ne!(
        SensitiveAction::CleanupEvents.execute_callback(),
        "cmd_cleanup_events"
    );
}

#[test]
fn admin_and_user_destructive_actions_have_separate_scopes() {
    assert!(SensitiveAction::Pause.requires_admin());
    assert!(SensitiveAction::ToggleMaintenance.requires_admin());
    assert!(!SensitiveAction::ClearWallets.requires_admin());
    assert!(!SensitiveAction::ForgetAll.requires_admin());
}

#[test]
fn request_identity_requires_private_actor_and_chat_match_for_admin() {
    let private_admin = RequestIdentity {
        actor_user_id: 42,
        chat_id: 42,
        message_id: 1,
        is_private: true,
    };
    let group_admin_actor = RequestIdentity {
        actor_user_id: 42,
        chat_id: -100,
        message_id: 1,
        is_private: false,
    };
    let wrong_actor = RequestIdentity {
        actor_user_id: 99,
        chat_id: 42,
        message_id: 1,
        is_private: true,
    };

    assert!(private_admin.is_private_admin(42, 42));
    assert!(!group_admin_actor.is_private_admin(42, 42));
    assert!(!wrong_actor.is_private_admin(42, 42));
}

#[test]
fn confirmation_callback_fits_telegram_limit() {
    let nonce = format!("{:032x}", std::process::id());
    let callback = confirmation_callback(SensitiveAction::ToggleMaintenance, &nonce);

    assert!(callback.len() <= 64);
}

#[test]
fn invalid_admin_do_callbacks_are_rejected() {
    assert!(action_from_admin_do_callback("admin_do:bad:token").is_err());
    assert!(action_from_admin_do_callback("bad").is_err());
}
