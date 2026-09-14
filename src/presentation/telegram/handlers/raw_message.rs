use super::wallet::WalletAddOutcome;
use crate::domain::models::{ActorChatKey, AppContext, PendingInputAction, PendingInputSession};
use crate::infrastructure::database::postgres_adapter::PostgresRepository;
use crate::wallet::wallet_use_cases::WalletManagementUseCase;
use dashmap::DashMap;
use std::sync::Arc;
use std::sync::atomic::Ordering;
use teloxide::prelude::*;

pub const MAINTENANCE_MESSAGE: &str =
    "🚧 <b>Maintenance Mode</b>\nThe bot is currently under maintenance.";
pub const UNKNOWN_COMMAND_MESSAGE: &str =
    "❓ <b>Unknown command.</b>\nUse /help to see the available commands.";

pub fn is_unknown_command_text(text: &str) -> bool {
    text.trim_start().starts_with('/')
}

fn maintenance_allows_raw_message(_text: Option<&str>) -> bool {
    // Raw-message fallback can add a wallet but has no read/delete operation.
    // Valid commands are dispatched before this handler.
    false
}

fn pending_input_for_message(
    sessions: &DashMap<ActorChatKey, PendingInputSession>,
    key: ActorChatKey,
) -> Option<PendingInputAction> {
    // Copy the action so no DashMap guard is kept across any handler await.
    sessions.get(&key).map(|entry| entry.value().action)
}

fn complete_pending_input(
    sessions: &DashMap<ActorChatKey, PendingInputSession>,
    key: ActorChatKey,
    outcome: WalletAddOutcome,
) {
    if outcome == WalletAddOutcome::Added {
        sessions.remove(&key);
    }
}

pub async fn handle_raw_message(
    bot: Bot,
    msg: Message,
    app_context: Arc<AppContext>,
) -> anyhow::Result<()> {
    let identity = match crate::presentation::telegram::request_identity::from_message(&msg) {
        Ok(identity) => identity,
        Err(_) => return Ok(()),
    };

    let is_admin = identity.is_private_admin(app_context.admin_user_id, app_context.admin_chat_id);

    if app_context.maintenance_mode.load(Ordering::Relaxed)
        && !is_admin
        && !maintenance_allows_raw_message(msg.text())
    {
        crate::send_logged!(bot, msg, MAINTENANCE_MESSAGE);
        return Ok(());
    }

    let pending_input = pending_input_for_message(
        &app_context.pending_input_sessions,
        identity.actor_chat_key(),
    );

    let raw_text = match msg.text() {
        Some(text) => text,
        None => return Ok(()),
    };

    if let Err(reason) = crate::utils::validate_raw_message_size(raw_text) {
        crate::send_logged!(bot, msg, format!("🚫 <b>Message rejected.</b>\n{}", reason));
        return Ok(());
    }

    if is_unknown_command_text(raw_text) {
        crate::send_logged!(bot, msg, UNKNOWN_COMMAND_MESSAGE);
        return Ok(());
    }

    let wallet_address = match crate::utils::extract_single_wallet_from_message(raw_text) {
        Ok(wallet) => wallet,
        Err(reason) => {
            crate::send_logged!(
                bot,
                msg,
                format!(
                    "🚫 <b>Message rejected.</b>\n{}",
                    crate::utils::html_escape(&reason)
                )
            );
            return Ok(());
        }
    };

    if let Some(address) = wallet_address {
        let db = Arc::new(PostgresRepository::new(app_context.pool.clone()));
        let wallet_mgt = Arc::new(WalletManagementUseCase::new(db));

        let outcome = crate::presentation::telegram::handlers::wallet::handle_add(
            bot,
            msg,
            identity.chat_id,
            identity.actor_user_id,
            address,
            wallet_mgt,
        )
        .await?;
        complete_pending_input(
            &app_context.pending_input_sessions,
            identity.actor_chat_key(),
            outcome,
        );

        return Ok(());
    }

    if matches!(pending_input, Some(PendingInputAction::AddWallet)) {
        crate::send_logged!(
            bot,
            msg,
            "⚠️ <b>No Kaspa wallet found.</b>\nSend one <code>kaspa:...</code> address or press Cancel."
        );
    }

    Ok(())
}

#[cfg(test)]
mod pending_input_regression_tests {
    use super::*;

    #[test]
    fn maintenance_blocks_raw_add_and_fallback_text() {
        assert!(!maintenance_allows_raw_message(Some("kaspa:qptest")));
        assert!(!maintenance_allows_raw_message(Some("plain fallback text")));
        assert!(!maintenance_allows_raw_message(None));
    }

    #[test]
    fn rejected_message_lookup_keeps_pending_add_for_another_attempt() {
        let sessions = DashMap::new();
        let key = ActorChatKey::new(1001, 1001);
        sessions.insert(
            key,
            PendingInputSession {
                action: PendingInputAction::AddWallet,
                message_id: 7,
            },
        );
        for attempt in 1..=2 {
            assert_eq!(
                pending_input_for_message(&sessions, key),
                Some(PendingInputAction::AddWallet),
                "retry {attempt} lost the Add Wallet context"
            );
            assert!(
                sessions.contains_key(&key),
                "reading input consumed its retry state"
            );
        }
    }

    #[test]
    fn lookup_never_consumes_another_actor_or_chat() {
        let sessions = DashMap::new();
        let original = ActorChatKey::new(1001, 1001);
        sessions.insert(
            original,
            PendingInputSession {
                action: PendingInputAction::AddWallet,
                message_id: 7,
            },
        );
        assert_eq!(
            pending_input_for_message(&sessions, ActorChatKey::new(1002, 1001)),
            None
        );
        assert_eq!(
            pending_input_for_message(&sessions, ActorChatKey::new(1001, 1002)),
            None
        );
        assert!(sessions.contains_key(&original));
    }

    #[test]
    fn rejected_add_keeps_retry_but_success_consumes_only_its_own_session() {
        let sessions = DashMap::new();
        let key = ActorChatKey::new(1001, 1001);
        let other_actor = ActorChatKey::new(1002, 1001);
        let other_chat = ActorChatKey::new(1001, 1002);
        for identity in [key, other_actor, other_chat] {
            sessions.insert(
                identity,
                PendingInputSession {
                    action: PendingInputAction::AddWallet,
                    message_id: 7,
                },
            );
        }
        complete_pending_input(&sessions, key, WalletAddOutcome::Rejected);
        assert_eq!(
            pending_input_for_message(&sessions, key),
            Some(PendingInputAction::AddWallet)
        );
        complete_pending_input(&sessions, key, WalletAddOutcome::Added);
        complete_pending_input(&sessions, key, WalletAddOutcome::Added);
        assert!(!sessions.contains_key(&key));
        assert!(sessions.contains_key(&other_actor));
        assert!(sessions.contains_key(&other_chat));
    }

    #[test]
    fn two_no_address_inputs_keep_the_existing_retry_prompt_condition() {
        let sessions = DashMap::new();
        let key = ActorChatKey::new(1001, 1001);
        sessions.insert(
            key,
            PendingInputSession {
                action: PendingInputAction::AddWallet,
                message_id: 7,
            },
        );
        for text in ["not a wallet", "still not a wallet"] {
            let pending = pending_input_for_message(&sessions, key);
            assert_eq!(
                crate::utils::extract_single_wallet_from_message(text).unwrap(),
                None
            );
            assert!(matches!(pending, Some(PendingInputAction::AddWallet)));
        }
        assert!(sessions.contains_key(&key));
        let address = kaspa_addresses::Address::new(
            kaspa_addresses::Prefix::Mainnet,
            kaspa_addresses::Version::PubKey,
            &[42; 32],
        )
        .to_string();
        assert!(crate::utils::validate_wallet_security(&address).is_ok());
        assert_eq!(
            crate::utils::extract_single_wallet_from_message(&address).unwrap(),
            Some(address)
        );
        // Supply the application outcome explicitly: this tests the state transition,
        // not persistence or Telegram delivery, which require operational qualification.
        complete_pending_input(&sessions, key, WalletAddOutcome::Added);
        assert!(sessions.is_empty());
    }

    #[test]
    fn pending_lookup_does_not_hold_a_shard_lock() {
        let sessions = DashMap::new();
        let key = ActorChatKey::new(1001, 1001);
        sessions.insert(
            key,
            PendingInputSession {
                action: PendingInputAction::AddWallet,
                message_id: 7,
            },
        );
        let pending = pending_input_for_message(&sessions, key);
        assert!(sessions.try_get_mut(&key).is_present());
        assert_eq!(pending, Some(PendingInputAction::AddWallet));
    }

    #[test]
    fn absent_pending_session_is_not_created_by_lookup() {
        let sessions = DashMap::new();
        assert_eq!(
            pending_input_for_message(&sessions, ActorChatKey::new(1001, 1001)),
            None
        );
        assert!(sessions.is_empty());
    }
}
