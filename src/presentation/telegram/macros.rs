#[macro_export]
macro_rules! send_logged {
    ($bot_instance:expr, $msg_obj:expr, $msg:expr) => {{
        $crate::utils::send_logged_message(
            &$bot_instance,
            $msg_obj.chat.id,
            Some($msg_obj.id),
            $msg.to_string(),
            None,
        )
        .await?;
    }};
}
