use kaspa_pulse::domain::models::{ActorChatKey, PendingInputAction, RequestIdentity};
use kaspa_pulse::presentation::telegram::commands::Command;

fn read_source(relative_path: &str) -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join(relative_path);
    std::fs::read_to_string(path).expect("source file must be readable")
}

#[test]
fn actor_and_chat_identity_are_distinct_types() {
    let identity = RequestIdentity {
        actor_user_id: 123,
        chat_id: -100456,
        message_id: 77,
        is_private: false,
    };

    assert_eq!(identity.actor_chat_key(), ActorChatKey::new(123, -100456));
    assert!(!identity.is_private_admin(123, 123));
}

#[test]
fn pending_wallet_input_is_scoped_to_actor_and_chat() {
    let first = ActorChatKey::new(1, -100);
    let second = ActorChatKey::new(2, -100);

    assert_ne!(first, second);
    assert_eq!(PendingInputAction::AddWallet, PendingInputAction::AddWallet);
}

#[test]
fn every_admin_command_is_classified_as_admin_only() {
    assert!(Command::Health.is_admin_only());
    assert!(Command::Pause.is_admin_only());
    assert!(Command::Toggle("SYNC".to_string()).is_admin_only());
    assert!(Command::RestartInfo.is_admin_only());
    assert!(!Command::Start.is_admin_only());
    assert!(!Command::Add("kaspa:test".to_string()).is_admin_only());
}

#[test]
fn command_authorization_uses_private_actor_identity() {
    let source = read_source("src/presentation/telegram/handlers/mod.rs");

    assert!(source.contains("identity.is_private_admin"));
    assert!(source.contains("cmd.is_admin_only()"));
    assert!(!source.contains("let is_admin = cid =="));
}

#[test]
fn command_and_callback_rate_limits_are_keyed_by_actor() {
    let source = read_source("src/presentation/telegram/handlers/mod.rs");
    let utils = read_source("src/utils.rs");

    assert!(source.contains("is_command_rate_limited(actor_user_id)"));
    assert!(source.contains("is_callback_rate_limited(identity.actor_user_id)"));
    assert!(utils.contains("RateLimiter<u64"));
    assert!(!utils.contains("pub fn is_spam("));
}

#[test]
fn confirmation_nonce_is_csprng_and_hash_indexed() {
    let source = read_source("src/presentation/telegram/handlers/admin_confirm.rs");

    assert!(source.contains("SysRng"));
    assert!(source.contains("try_fill_bytes"));
    assert!(source.contains("Sha256"));
    assert!(source.contains("NONCE_BYTES: usize = 16"));
    assert!(!source.contains("token_seed"));
}

#[test]
fn confirmation_is_bound_to_actor_chat_and_message() {
    let source = read_source("src/presentation/telegram/handlers/admin_confirm.rs");

    assert!(source.contains("stored.actor_user_id != identity.actor_user_id"));
    assert!(source.contains("stored.chat_id != identity.chat_id"));
    assert!(source.contains("stored.message_id != identity.message_id"));
    assert!(source.contains("entry.remove()"));
}

#[test]
fn admin_configuration_separates_user_and_chat_with_legacy_fallback() {
    let config_source = read_source("src/config/mod.rs");
    let main_source = read_source("src/main.rs");

    assert!(config_source.contains("ADMIN_USER_ID"));
    assert!(config_source.contains("ADMIN_CHAT_ID"));
    assert!(config_source.contains("ADMIN_ID"));
    assert!(config_source.contains("ADMIN_CHAT_ID must equal ADMIN_USER_ID"));
    assert!(main_source.contains("startup.admin_user_id"));
    assert!(main_source.contains("startup.admin_chat_id"));
}

#[test]
fn admin_audit_records_both_actor_and_chat() {
    let source = read_source("src/infrastructure/admin_audit.rs");
    let migration = read_source("migrations/0010_admin_actor_identity.sql");

    assert!(source.contains("admin_actor_user_id"));
    assert!(source.contains("admin_chat_id"));
    assert!(migration.contains("ADD COLUMN IF NOT EXISTS admin_actor_user_id"));
}

#[test]
fn admin_and_pending_input_sessions_are_separate() {
    let context = read_source("src/domain/models/context.rs");
    let raw_handler = read_source("src/presentation/telegram/handlers/raw_message.rs");

    assert!(context.contains("admin_confirmations"));
    assert!(context.contains("pending_input_sessions"));
    assert!(!context.contains("admin_sessions"));
    assert!(!raw_handler.contains("admin_confirmations.remove"));
}

#[test]
fn admin_confirmation_nonce_is_redacted_before_callback_logging() {
    let source = read_source("src/presentation/telegram/handlers/mod.rs");
    let utils = read_source("src/utils.rs");

    let normalized_source = source.replace("\r\n", "\n");

    assert!(normalized_source.contains("sanitize_callback_data_for_log(&data)"));
    assert!(utils.contains("admin_do:{}:[REDACTED]"));
    assert!(!normalized_source.contains("&data,\n        false,"));
}
