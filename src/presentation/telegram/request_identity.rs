use crate::domain::models::RequestIdentity;
use teloxide::types::{CallbackQuery, Message};

pub fn from_message(message: &Message) -> Result<RequestIdentity, &'static str> {
    let actor = message
        .from
        .as_ref()
        .ok_or("Telegram message does not contain an actor identity.")?;

    Ok(RequestIdentity {
        actor_user_id: actor.id.0,
        chat_id: message.chat.id.0,
        message_id: message.id.0,
        is_private: message.chat.is_private(),
    })
}

pub fn from_callback(callback: &CallbackQuery) -> Result<RequestIdentity, &'static str> {
    let message = callback
        .message
        .as_ref()
        .ok_or("Callback message is no longer available.")?;

    Ok(RequestIdentity {
        actor_user_id: callback.from.id.0,
        chat_id: message.chat().id.0,
        message_id: message.id().0,
        is_private: message.chat().is_private(),
    })
}
