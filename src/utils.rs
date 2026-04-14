use anyhow::Context;
use governor::{clock::DefaultClock, state::keyed::DefaultKeyedStateStore, Quota, RateLimiter};
use std::num::NonZeroU32;
use std::sync::OnceLock;
use teloxide::{
    prelude::*,
    types::{ChatId, InlineKeyboardMarkup},
};

type SpamLimiter = RateLimiter<i64, DefaultKeyedStateStore<i64>, DefaultClock>;

pub fn f_num(n: f64) -> String {
    let s = format!("{:.0}", n);
    let mut result = String::new();
    let len = s.len();
    for (i, c) in s.chars().enumerate() {
        result.push(c);
        if (len - i - 1) % 3 == 0 && i != len - 1 {
            result.push(',');
        }
    }
    result
}

// 🔄 Unified Enterprise Function for Sending or In-Place Editing
pub async fn send_or_edit_log<T: AsRef<str>>(
    bot: &Bot,
    chat_id: ChatId,
    msg_id: Option<teloxide::types::MessageId>,
    text: T,
    markup: Option<InlineKeyboardMarkup>,
) -> anyhow::Result<()> {
    let text_ref = text.as_ref();
    crate::utils::log_multiline(
        &format!("📤 [BOT OUT] Chat: {}\n[RESPONSE]:", chat_id.0),
        text_ref,
        true,
    );

    let preview_opts = teloxide::types::LinkPreviewOptions {
        is_disabled: true,
        url: None,
        prefer_small_media: false,
        prefer_large_media: false,
        show_above_text: false,
    };

    if let Some(id) = msg_id {
        let mut req = bot
            .edit_message_text(chat_id, id, text_ref.to_string())
            .parse_mode(teloxide::types::ParseMode::Html)
            .link_preview_options(preview_opts);
        if let Some(ref m) = markup {
            req = req.reply_markup(m.clone());
        }

        match req.await {
            Ok(_) => Ok(()),
            Err(teloxide::RequestError::Api(teloxide::ApiError::MessageNotModified)) => Ok(()), // Gracefully ignore unchanged text
            Err(e) => Err(anyhow::anyhow!("API Error: {}", e)),
        }
    } else {
        let mut req = bot
            .send_message(chat_id, text_ref.to_string())
            .parse_mode(teloxide::types::ParseMode::Html)
            .link_preview_options(preview_opts);
        if let Some(ref m) = markup {
            req = req.reply_markup(m.clone());
        }
        req.await.context("API Error")?;
        Ok(())
    }
}

// 🔄 Helper to generate the Refresh Button
pub fn refresh_markup(cmd_callback: &str) -> InlineKeyboardMarkup {
    InlineKeyboardMarkup::new(vec![vec![teloxide::types::InlineKeyboardButton::callback(
        "🔄 Refresh",
        cmd_callback,
    )]])
}

pub fn format_short_wallet(w: &str) -> String {
    let chars: Vec<char> = w.chars().collect();
    if chars.len() > 18 {
        let start: String = chars[0..12].iter().collect();
        let end: String = chars[chars.len() - 6..].iter().collect();
        format!("{}...{}", start, end)
    } else {
        w.to_string()
    }
}

pub fn format_hash(hash: &str, link_type: &str) -> String {
    format!(
        "<a href=\"https://kaspa.stream/{}/{}\">{}</a>",
        link_type,
        hash,
        format_short_wallet(hash)
    )
}

pub fn is_spam(chat_id: i64) -> bool {
    static LIMITER: OnceLock<SpamLimiter> = OnceLock::new();
    let limiter =
        LIMITER.get_or_init(|| RateLimiter::keyed(Quota::per_second(NonZeroU32::new(1).unwrap())));

    limiter.check_key(&chat_id).is_err()
}

pub fn clean_for_log(s: &str) -> String {
    let mut result = String::with_capacity(s.len());
    let mut in_tag = false;
    for c in s.chars() {
        if c == '<' {
            in_tag = true;
        } else if c == '>' {
            in_tag = false;
        } else if !in_tag {
            result.push(c);
        }
    }
    result
}

pub fn log_multiline(header: &str, body: &str, is_html: bool) {
    for line in header.lines() {
        if !line.trim().is_empty() {
            tracing::info!("{}", line);
        }
    }
    let body_to_print = if is_html {
        clean_for_log(body)
    } else {
        body.to_string()
    };
    for line in body_to_print.lines() {
        if !line.trim().is_empty() {
            tracing::info!("   | {}", line);
        }
    }
}
