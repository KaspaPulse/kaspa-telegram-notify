use crate::domain::models::{AppContext, PendingInputAction};
use crate::infrastructure::database::postgres_adapter::PostgresRepository;
use crate::wallet::wallet_use_cases::WalletManagementUseCase;
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

    if app_context.maintenance_mode.load(Ordering::Relaxed) && !is_admin {
        crate::send_logged!(bot, msg, MAINTENANCE_MESSAGE);
        return Ok(());
    }

    let pending_input = app_context
        .pending_input_sessions
        .remove(&identity.actor_chat_key())
        .map(|(_, action)| action);

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

        crate::presentation::telegram::handlers::wallet::handle_add(
            bot,
            msg,
            identity.chat_id,
            identity.actor_user_id,
            address,
            wallet_mgt,
        )
        .await?;

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
