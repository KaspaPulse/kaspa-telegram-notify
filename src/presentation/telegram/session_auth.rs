use teloxide::dispatching::dialogue::InMemStorage;
use teloxide::prelude::*;

// Enterprise Standard: Secure Session State
#[derive(Clone, Default)]
pub enum State {
    #[default]
    Idle,
    AwaitingAdminPin { command: String },
}

pub type BotDialogue = Dialogue<State, InMemStorage<State>>;

pub async fn handle_admin_auth(
    bot: Bot,
    msg: Message,
    dialogue: BotDialogue,
    command: String,
) -> anyhow::Result<()> {
    bot.send_message(msg.chat.id, "🔐 Enter Admin PIN securely (This will be intercepted and deleted):").await?;
    dialogue.update(State::AwaitingAdminPin { command }).await?;
    Ok(())
}

pub async fn verify_pin_and_execute(
    bot: Bot,
    msg: Message,
    dialogue: BotDialogue,
    command: String,
) -> anyhow::Result<()> {
    let pin = msg.text().unwrap_or_default();
    
    // Safety: Delete the user's message containing the PIN immediately
    let _ = bot.delete_message(msg.chat.id, msg.id).await;

    if crate::infrastructure::security::utils::verify_admin_pin(pin) {
        bot.send_message(msg.chat.id, format!("✅ PIN Verified. Executing: {}", command)).await?;
        // TODO: Route to actual execution logic here based on command
        dialogue.exit().await?;
    } else {
        bot.send_message(msg.chat.id, "⛔ Security Alert: Invalid PIN.").await?;
        dialogue.exit().await?;
    }
    Ok(())
}
