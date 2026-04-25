#[macro_export]
macro_rules! send_logged {
    ($bot_instance:expr, $msg_obj:expr, $msg:expr) => {{
        let text = $msg.to_string();
        $crate::utils::log_multiline(
            &format!("📤 [BOT OUT] Chat: {}", $msg_obj.chat.id),
            &text,
            true,
        );

        // 🔥 FORCE REPLY: Direct link between the request (button click) and the result
        let _ = $bot_instance
            .send_message($msg_obj.chat.id, text)
            .parse_mode(teloxide::types::ParseMode::Html)
            .reply_parameters(teloxide::types::ReplyParameters::new($msg_obj.id))
            .await;
    }};
}
