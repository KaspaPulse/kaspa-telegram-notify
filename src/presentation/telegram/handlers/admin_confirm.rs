use crate::domain::models::{AppContext, ConfirmationSession, RequestIdentity, SensitiveAction};
use dashmap::DashMap;
use dashmap::mapref::entry::Entry;
use rand::TryRng;
use rand::rngs::SysRng;
use sha2::{Digest, Sha256};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use teloxide::prelude::*;
use teloxide::types::{InlineKeyboardButton, InlineKeyboardMarkup, ParseMode};

const CONFIRM_TTL_SECS: u64 = 60;
const NONCE_BYTES: usize = 16;

impl SensitiveAction {
    pub const fn label(self) -> &'static str {
        match self {
            Self::Pause => "Pause live monitoring",
            Self::Resume => "Resume live monitoring",
            Self::CleanupEvents => "Purge old event logs",
            Self::MuteAlerts => "Stop mining alert delivery",
            Self::UnmuteAlerts => "Resume mining alert delivery",
            Self::ClearWallets => "Clear all tracked wallets",
            Self::ForgetAll => "Delete all user data",
            Self::ToggleMemoryCleaner => "Toggle memory cleaner",
            Self::ToggleLiveSync => "Toggle live monitoring setting",
            Self::ToggleMaintenance => "Toggle maintenance mode",
        }
    }

    pub const fn risk_text(self) -> &'static str {
        match self {
            Self::Pause => "This will stop live monitoring until it is resumed.",
            Self::Resume => "This will enable live monitoring again.",
            Self::CleanupEvents => {
                "This will purge old event records according to the configured cleanup policy."
            }
            Self::ClearWallets => "This will remove all tracked wallets for this chat.",
            Self::ForgetAll => "This will remove all wallets and user data linked to this chat.",
            Self::ToggleMemoryCleaner => "This will change the memory cleaner runtime state.",
            Self::ToggleLiveSync => "This will change live monitoring runtime state.",
            Self::ToggleMaintenance => "This will change maintenance mode.",
            Self::MuteAlerts => {
                "This will stop Telegram mining alert delivery only. Block detection, DAG analysis, and database logging will continue."
            }
            Self::UnmuteAlerts => "This will resume Telegram mining alert delivery for new alerts.",
        }
    }
}

pub fn now_unix_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|value| value.as_secs())
        .unwrap_or(0)
}

fn encode_hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut output = String::with_capacity(bytes.len() * 2);

    for byte in bytes {
        output.push(HEX[(byte >> 4) as usize] as char);
        output.push(HEX[(byte & 0x0f) as usize] as char);
    }

    output
}

fn nonce_hash(nonce: &str) -> Result<String, String> {
    if nonce.len() != NONCE_BYTES * 2 || !nonce.bytes().all(|value| value.is_ascii_hexdigit()) {
        return Err("Invalid confirmation token.".to_string());
    }

    Ok(encode_hex(&Sha256::digest(nonce.as_bytes())))
}

fn generate_nonce() -> anyhow::Result<String> {
    let mut bytes = [0u8; NONCE_BYTES];
    SysRng.try_fill_bytes(&mut bytes).map_err(|error| {
        anyhow::anyhow!("failed to generate secure confirmation nonce: {error}")
    })?;
    Ok(encode_hex(&bytes))
}

fn register_confirmation(
    ctx: &Arc<AppContext>,
    identity: RequestIdentity,
    action: SensitiveAction,
    nonce: &str,
) -> Result<(), String> {
    cleanup_expired(ctx);

    if action.requires_admin() && !identity.is_private_admin(ctx.admin_user_id, ctx.admin_chat_id) {
        return Err(
            "Admin actions are allowed only in the configured private admin chat.".to_string(),
        );
    }

    let key = nonce_hash(nonce)?;

    ctx.admin_confirmations.retain(|_, session| {
        !(session.actor_user_id == identity.actor_user_id
            && session.chat_id == identity.chat_id
            && session.message_id == identity.message_id
            && session.action == action)
    });

    match ctx.admin_confirmations.entry(key) {
        Entry::Vacant(entry) => {
            entry.insert(ConfirmationSession {
                actor_user_id: identity.actor_user_id,
                chat_id: identity.chat_id,
                message_id: identity.message_id,
                action,
                expires_at_unix_secs: now_unix_secs().saturating_add(CONFIRM_TTL_SECS),
            });
            Ok(())
        }
        Entry::Occupied(_) => Err("Confirmation nonce collision. Please retry.".to_string()),
    }
}

pub fn confirmation_callback(action: SensitiveAction, nonce: &str) -> String {
    format!("admin_do:{}:{}", action.as_str(), nonce)
}

pub fn confirmation_markup(action: SensitiveAction, nonce: &str) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![
        vec![
            InlineKeyboardButton::callback(
                format!("✅ Confirm {}", action.label()),
                confirmation_callback(action, nonce),
            ),
            InlineKeyboardButton::callback("❌ Cancel", "cancel_action"),
        ],
        vec![InlineKeyboardButton::callback("🔙 Main Menu", "cmd_start")],
    ])
}

pub fn confirmation_text(action: SensitiveAction) -> String {
    format!(
        "⚠️ <b>Confirmation Required</b>\n━━━━━━━━━━━━━━━━━━\n<b>Action:</b> <code>{}</code>\n<b>Risk:</b> {}\n\nThis confirmation expires in <code>{}</code> seconds and can be used only once.",
        action.label(),
        action.risk_text(),
        CONFIRM_TTL_SECS
    )
}

pub async fn send_command_confirmation(
    bot: &Bot,
    ctx: &Arc<AppContext>,
    identity: RequestIdentity,
    action: SensitiveAction,
) -> anyhow::Result<()> {
    if action.requires_admin() && !identity.is_private_admin(ctx.admin_user_id, ctx.admin_chat_id) {
        anyhow::bail!("Admin actions are allowed only in the configured private admin chat.");
    }

    let nonce = generate_nonce()?;
    let confirmation_message = bot
        .send_message(
            teloxide::types::ChatId(identity.chat_id),
            confirmation_text(action),
        )
        .parse_mode(ParseMode::Html)
        .reply_markup(confirmation_markup(action, &nonce))
        .await?;

    let confirmation_identity = RequestIdentity {
        message_id: confirmation_message.id.0,
        ..identity
    };

    register_confirmation(ctx, confirmation_identity, action, &nonce)
        .map_err(anyhow::Error::msg)?;

    Ok(())
}

pub async fn edit_callback_confirmation(
    bot: &Bot,
    msg: &teloxide::types::MaybeInaccessibleMessage,
    ctx: &Arc<AppContext>,
    identity: RequestIdentity,
    action: SensitiveAction,
) -> anyhow::Result<()> {
    let nonce = generate_nonce()?;
    register_confirmation(ctx, identity, action, &nonce).map_err(anyhow::Error::msg)?;

    let result = bot
        .edit_message_text(msg.chat().id, msg.id(), confirmation_text(action))
        .parse_mode(ParseMode::Html)
        .reply_markup(confirmation_markup(action, &nonce))
        .await;

    if let Err(error) = result {
        if let Ok(key) = nonce_hash(&nonce) {
            ctx.admin_confirmations.remove(&key);
        }
        return Err(error.into());
    }

    Ok(())
}

pub fn cleanup_expired(ctx: &Arc<AppContext>) {
    let now = now_unix_secs();
    ctx.admin_confirmations
        .retain(|_, session| session.expires_at_unix_secs > now);
}

pub fn cancel_for_identity(ctx: &Arc<AppContext>, identity: RequestIdentity) {
    ctx.admin_confirmations.retain(|_, session| {
        !(session.actor_user_id == identity.actor_user_id
            && session.chat_id == identity.chat_id
            && session.message_id == identity.message_id)
    });

    ctx.pending_input_sessions
        .remove(&identity.actor_chat_key());
}

pub fn clear_all_runtime_state_for_identity(ctx: &Arc<AppContext>, identity: RequestIdentity) {
    ctx.admin_confirmations.retain(|_, session| {
        session.actor_user_id != identity.actor_user_id && session.chat_id != identity.chat_id
    });
    ctx.pending_input_sessions
        .remove(&identity.actor_chat_key());
}

pub fn sensitive_action_from_toggle_flag(flag: &str) -> Option<SensitiveAction> {
    match flag.trim().to_uppercase().as_str() {
        "ENABLE_MEMORY_CLEANER" | "MEMORY" | "MEM" => Some(SensitiveAction::ToggleMemoryCleaner),
        "ENABLE_LIVE_SYNC" | "LIVE" | "SYNC" => Some(SensitiveAction::ToggleLiveSync),
        "MAINTENANCE_MODE" | "MAINTENANCE" => Some(SensitiveAction::ToggleMaintenance),
        _ => None,
    }
}

pub fn sensitive_action_from_callback(data: &str) -> Option<SensitiveAction> {
    match data {
        "cmd_pause" => Some(SensitiveAction::Pause),
        "cmd_resume" => Some(SensitiveAction::Resume),
        "cmd_cleanup_events" => Some(SensitiveAction::CleanupEvents),
        "cmd_mute_alerts" => Some(SensitiveAction::MuteAlerts),
        "cmd_unmute_alerts" => Some(SensitiveAction::UnmuteAlerts),
        "confirm_forget_wallets" => Some(SensitiveAction::ClearWallets),
        "confirm_forget_all" => Some(SensitiveAction::ForgetAll),
        "btn_toggle_ENABLE_MEMORY_CLEANER" => Some(SensitiveAction::ToggleMemoryCleaner),
        "btn_toggle_ENABLE_LIVE_SYNC" => Some(SensitiveAction::ToggleLiveSync),
        "btn_toggle_MAINTENANCE_MODE" => Some(SensitiveAction::ToggleMaintenance),
        _ => None,
    }
}

fn parse_admin_do_callback(data: &str) -> Result<(SensitiveAction, &str), String> {
    let mut parts = data.split(':');
    let prefix = parts.next();
    let action = parts.next();
    let nonce = parts.next();

    if prefix != Some("admin_do") || parts.next().is_some() {
        return Err("Invalid confirmation callback.".to_string());
    }

    let action = action
        .and_then(SensitiveAction::parse)
        .ok_or_else(|| "Unknown sensitive action.".to_string())?;
    let nonce = nonce.ok_or_else(|| "Invalid confirmation token.".to_string())?;

    nonce_hash(nonce)?;
    Ok((action, nonce))
}

pub fn action_from_admin_do_callback(data: &str) -> Result<SensitiveAction, String> {
    parse_admin_do_callback(data).map(|(action, _)| action)
}

fn consume_confirmation(
    confirmations: &DashMap<String, ConfirmationSession>,
    identity: RequestIdentity,
    requested_action: SensitiveAction,
    requested_nonce: &str,
    now: u64,
    admin_user_id: u64,
    admin_chat_id: i64,
) -> Result<SensitiveAction, String> {
    let key = nonce_hash(requested_nonce)?;

    match confirmations.entry(key) {
        Entry::Vacant(_) => Err("Confirmation expired, missing, or already used.".to_string()),
        Entry::Occupied(entry) => {
            let stored = entry.get().clone();

            if stored.expires_at_unix_secs <= now {
                entry.remove();
                return Err("Confirmation expired. Please try again.".to_string());
            }

            if stored.actor_user_id != identity.actor_user_id {
                return Err("Confirmation belongs to a different Telegram user.".to_string());
            }

            if stored.chat_id != identity.chat_id {
                return Err("Confirmation belongs to a different chat.".to_string());
            }

            if stored.message_id != identity.message_id {
                return Err("Confirmation belongs to a different message.".to_string());
            }

            if stored.action != requested_action {
                return Err("Confirmation action mismatch.".to_string());
            }

            if requested_action.requires_admin()
                && !identity.is_private_admin(admin_user_id, admin_chat_id)
            {
                return Err(
                    "Admin actions are allowed only in the configured private admin chat."
                        .to_string(),
                );
            }

            entry.remove();
            Ok(requested_action)
        }
    }
}

pub fn validate_admin_do_callback(
    ctx: &Arc<AppContext>,
    identity: RequestIdentity,
    data: &str,
) -> Result<SensitiveAction, String> {
    cleanup_expired(ctx);

    let requested_action = action_from_admin_do_callback(data)?;
    let (_, requested_nonce) = parse_admin_do_callback(data)?;

    consume_confirmation(
        &ctx.admin_confirmations,
        identity,
        requested_action,
        requested_nonce,
        now_unix_secs(),
        ctx.admin_user_id,
        ctx.admin_chat_id,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn callback_mapping_for_sensitive_actions_is_stable() {
        assert_eq!(
            sensitive_action_from_callback("cmd_pause"),
            Some(SensitiveAction::Pause)
        );
        assert_eq!(
            sensitive_action_from_callback("btn_toggle_MAINTENANCE_MODE"),
            Some(SensitiveAction::ToggleMaintenance)
        );
        assert_eq!(SensitiveAction::Pause.execute_callback(), "do_pause");
        assert_eq!(
            SensitiveAction::ForgetAll.execute_callback(),
            "do_forget_all"
        );
    }

    #[test]
    fn generated_nonce_is_128_bit_lower_hex() {
        let nonce = generate_nonce().expect("system RNG should be available");
        assert_eq!(nonce.len(), 32);
        assert!(nonce.bytes().all(|value| value.is_ascii_hexdigit()));
    }

    #[test]
    fn callback_data_stays_within_telegram_limit() {
        let nonce = generate_nonce().expect("system RNG should be available");
        let callback = confirmation_callback(SensitiveAction::ToggleMaintenance, &nonce);
        assert!(callback.len() <= 64);
    }

    #[test]
    fn invalid_admin_do_callback_is_rejected() {
        assert!(action_from_admin_do_callback("bad").is_err());
        assert!(
            action_from_admin_do_callback(&format!(
                "admin_do:unknown:{}",
                generate_nonce().expect("system RNG should be available")
            ))
            .is_err()
        );
        assert!(action_from_admin_do_callback("admin_do:pause:short").is_err());
    }

    #[test]
    fn confirmation_is_bound_to_actor_chat_message_and_is_single_use() {
        let nonce = generate_nonce().expect("system RNG should be available");
        let key = nonce_hash(&nonce).unwrap();
        let confirmations = DashMap::new();
        let identity = RequestIdentity {
            actor_user_id: 42,
            chat_id: 42,
            message_id: 7,
            is_private: true,
        };

        confirmations.insert(
            key,
            ConfirmationSession {
                actor_user_id: 42,
                chat_id: 42,
                message_id: 7,
                action: SensitiveAction::Pause,
                expires_at_unix_secs: 200,
            },
        );

        assert_eq!(
            consume_confirmation(
                &confirmations,
                identity,
                SensitiveAction::Pause,
                &nonce,
                100,
                42,
                42,
            ),
            Ok(SensitiveAction::Pause)
        );

        assert!(
            consume_confirmation(
                &confirmations,
                identity,
                SensitiveAction::Pause,
                &nonce,
                100,
                42,
                42,
            )
            .is_err()
        );
    }

    #[test]
    fn another_actor_cannot_consume_the_confirmation() {
        let nonce = generate_nonce().expect("system RNG should be available");
        let key = nonce_hash(&nonce).unwrap();
        let confirmations = DashMap::new();

        confirmations.insert(
            key,
            ConfirmationSession {
                actor_user_id: 42,
                chat_id: 42,
                message_id: 7,
                action: SensitiveAction::Pause,
                expires_at_unix_secs: 200,
            },
        );

        let attacker = RequestIdentity {
            actor_user_id: 99,
            chat_id: 42,
            message_id: 7,
            is_private: true,
        };

        assert!(
            consume_confirmation(
                &confirmations,
                attacker,
                SensitiveAction::Pause,
                &nonce,
                100,
                42,
                42,
            )
            .is_err()
        );
        assert_eq!(confirmations.len(), 1);
    }

    #[test]
    fn admin_confirmation_is_rejected_outside_private_chat() {
        let nonce = generate_nonce().expect("system RNG should be available");
        let key = nonce_hash(&nonce).unwrap();
        let confirmations = DashMap::new();

        confirmations.insert(
            key,
            ConfirmationSession {
                actor_user_id: 42,
                chat_id: -100,
                message_id: 7,
                action: SensitiveAction::Pause,
                expires_at_unix_secs: 200,
            },
        );

        let group_identity = RequestIdentity {
            actor_user_id: 42,
            chat_id: -100,
            message_id: 7,
            is_private: false,
        };

        assert!(
            consume_confirmation(
                &confirmations,
                group_identity,
                SensitiveAction::Pause,
                &nonce,
                100,
                42,
                42,
            )
            .is_err()
        );
        assert_eq!(confirmations.len(), 1);
    }
}
