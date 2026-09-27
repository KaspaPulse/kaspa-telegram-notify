use anyhow::Context;
use governor::{Quota, RateLimiter, clock::DefaultClock, state::keyed::DefaultKeyedStateStore};
use std::num::NonZeroU32;
use std::sync::OnceLock;
use teloxide::{
    prelude::*,
    types::{ChatId, InlineKeyboardMarkup},
};

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

// 🔄 Unified Community Function for Sending or In-Place Editing
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
    let safe_header = sanitize_for_log(header);

    for line in safe_header.lines() {
        if !line.trim().is_empty() {
            tracing::info!("{}", line);
        }
    }

    let body_to_print = if is_html {
        clean_for_log(body)
    } else {
        body.to_string()
    };

    let safe_body = sanitize_for_log(&body_to_print);

    for line in safe_body.lines() {
        if !line.trim().is_empty() {
            tracing::info!("   | {}", line);
        }
    }
}

pub async fn send_reply_or_edit_log(
    bot: &teloxide::Bot,
    chat_id: teloxide::types::ChatId,
    reply_to: teloxide::types::MessageId,
    edit_msg_id: Option<teloxide::types::MessageId>,
    text: String,
    markup: Option<teloxide::types::InlineKeyboardMarkup>,
) {
    crate::utils::log_multiline(&format!("📤 [BOT OUT] Chat: {}", chat_id.0), &text, true);

    if let Some(id) = edit_msg_id {
        let mut req = bot
            .edit_message_text(chat_id, id, text)
            .parse_mode(teloxide::types::ParseMode::Html);

        if let Some(m) = markup {
            req = req.reply_markup(m);
        }

        match req.await {
            Ok(_) => {}
            Err(teloxide::RequestError::Api(teloxide::ApiError::MessageNotModified)) => {}
            Err(e) => tracing::error!("[TELEGRAM ERROR] Failed to edit logged response: {}", e),
        }
    } else {
        let mut req = bot
            .send_message(chat_id, text)
            .reply_parameters(teloxide::types::ReplyParameters::new(reply_to))
            .parse_mode(teloxide::types::ParseMode::Html);

        if let Some(m) = markup {
            req = req.reply_markup(m);
        }

        if let Err(e) = req.await {
            tracing::error!("[TELEGRAM ERROR] Failed to send logged response: {}", e);
        }
    }
}

pub async fn send_logged_message(
    bot: &teloxide::Bot,
    chat_id: teloxide::types::ChatId,
    reply_to: Option<teloxide::types::MessageId>,
    text: String,
    markup: Option<teloxide::types::InlineKeyboardMarkup>,
) -> anyhow::Result<()> {
    crate::utils::log_multiline(&format!("📤 [BOT OUT] Chat: {}", chat_id.0), &text, true);

    let mut req = bot
        .send_message(chat_id, text)
        .parse_mode(teloxide::types::ParseMode::Html);

    if let Some(reply_id) = reply_to {
        req = req.reply_parameters(teloxide::types::ReplyParameters::new(reply_id));
    }

    if let Some(markup) = markup {
        req = req.reply_markup(markup);
    }

    match req.await {
        Ok(_) => Ok(()),
        Err(error) => {
            tracing::error!("[TELEGRAM ERROR] Failed to send logged message: {}", error);
            Err(anyhow::anyhow!("Telegram send failed: {}", error))
        }
    }
}

pub async fn edit_logged_message(
    bot: &teloxide::Bot,
    chat_id: teloxide::types::ChatId,
    message_id: teloxide::types::MessageId,
    text: String,
    markup: Option<teloxide::types::InlineKeyboardMarkup>,
) -> anyhow::Result<()> {
    crate::utils::log_multiline(
        &format!(
            "📤 [BOT OUT] Chat: {} | Edit Message: {}",
            chat_id.0, message_id.0
        ),
        &text,
        true,
    );

    let mut req = bot
        .edit_message_text(chat_id, message_id, text)
        .parse_mode(teloxide::types::ParseMode::Html);

    if let Some(markup) = markup {
        req = req.reply_markup(markup);
    }

    match req.await {
        Ok(_) | Err(teloxide::RequestError::Api(teloxide::ApiError::MessageNotModified)) => Ok(()),
        Err(error) => {
            tracing::error!("[TELEGRAM ERROR] Failed to edit logged message: {}", error);
            Err(anyhow::anyhow!("Telegram edit failed: {}", error))
        }
    }
}

// === Telegram request protection helpers (Stage 3) ===

type TelegramLimiter = RateLimiter<u64, DefaultKeyedStateStore<u64>, DefaultClock>;

fn env_u32(key: &str, default_value: u32) -> u32 {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<u32>().ok())
        .unwrap_or(default_value)
}

fn safe_nonzero(value: u32, default_value: u32) -> NonZeroU32 {
    NonZeroU32::new(value)
        .or_else(|| NonZeroU32::new(default_value))
        .unwrap_or(NonZeroU32::MIN)
}

fn per_second_quota(key: &str, default_value: u32) -> Quota {
    Quota::per_second(safe_nonzero(env_u32(key, default_value), default_value))
}

fn per_minute_quota(key: &str, default_value: u32) -> Quota {
    Quota::per_minute(safe_nonzero(env_u32(key, default_value), default_value))
}

pub fn is_command_rate_limited(actor_user_id: u64) -> bool {
    static LIMITER: OnceLock<TelegramLimiter> = OnceLock::new();

    let limiter = LIMITER
        .get_or_init(|| RateLimiter::keyed(per_second_quota("RATE_LIMIT_COMMANDS_PER_SECOND", 1)));

    limiter.check_key(&actor_user_id).is_err()
}

pub fn is_callback_rate_limited(actor_user_id: u64) -> bool {
    static LIMITER: OnceLock<TelegramLimiter> = OnceLock::new();

    let limiter = LIMITER
        .get_or_init(|| RateLimiter::keyed(per_second_quota("RATE_LIMIT_CALLBACKS_PER_SECOND", 3)));

    limiter.check_key(&actor_user_id).is_err()
}

pub fn is_add_wallet_rate_limited(actor_user_id: u64) -> bool {
    static LIMITER: OnceLock<TelegramLimiter> = OnceLock::new();

    let limiter = LIMITER.get_or_init(|| {
        RateLimiter::keyed(per_minute_quota("RATE_LIMIT_ADD_WALLET_PER_MINUTE", 5))
    });

    limiter.check_key(&actor_user_id).is_err()
}

pub fn max_wallets_per_user() -> i64 {
    std::env::var("MAX_WALLETS_PER_USER")
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(10)
}

pub fn rate_limit_message() -> &'static str {
    "⏳ <b>Too many requests.</b>\nPlease slow down and try again shortly."
}

// === End Telegram request protection helpers (Stage 3) ===

// === Logging privacy helpers (Stage 4) ===

pub fn env_bool(key: &str, default_value: bool) -> bool {
    std::env::var(key)
        .ok()
        .map(|value| {
            let value = value.trim().to_ascii_lowercase();
            value == "true" || value == "1" || value == "yes" || value == "on"
        })
        .unwrap_or(default_value)
}

pub fn env_usize(key: &str, default_value: usize) -> usize {
    std::env::var(key)
        .ok()
        .and_then(|value| value.parse::<usize>().ok())
        .filter(|value| *value > 0)
        .unwrap_or(default_value)
}

pub fn verbose_logs_enabled() -> bool {
    env_bool("ENABLE_VERBOSE_LOGS", false)
}

pub fn max_raw_message_chars() -> usize {
    env_usize("MAX_RAW_MESSAGE_CHARS", 512)
}

pub fn max_wallet_address_chars() -> usize {
    env_usize("MAX_WALLET_ADDRESS_CHARS", 120)
}

pub fn max_log_chars() -> usize {
    env_usize("LOG_MAX_CHARS", 5000)
}

pub fn sanitize_for_log(input: &str) -> String {
    let mut text = input.to_string();

    if !verbose_logs_enabled() {
        text = mask_kaspa_addresses(&text);
        text = mask_common_secret_values(&text);
        text = mask_hex_hash_like_tokens(&text);
        text = mask_telegram_usernames(&text);
    }

    truncate_for_log(&text)
}

pub fn sanitize_callback_data_for_log(data: &str) -> String {
    if let Some(rest) = data.strip_prefix("admin_do:") {
        let safe_action = rest
            .split(':')
            .next()
            .unwrap_or_default()
            .chars()
            .filter(|value| value.is_ascii_lowercase() || *value == '_')
            .take(48)
            .collect::<String>();

        let action = if safe_action.is_empty() {
            "unknown"
        } else {
            safe_action.as_str()
        };

        return format!("admin_do:{}:[REDACTED]", action);
    }

    sanitize_for_log(data)
}

pub fn sanitize_user_text(input: &str) -> String {
    input
        .replace('\r', "\n")
        .chars()
        .filter(|c| !is_dangerous_invisible_char(*c))
        .collect::<String>()
        .trim()
        .to_string()
}

fn truncate_for_log(input: &str) -> String {
    let max_chars = max_log_chars();

    if input.chars().count() <= max_chars {
        return input.to_string();
    }

    let short = input.chars().take(max_chars).collect::<String>();

    format!(
        "{}\n...[log truncated: original {} chars, limit {} chars]",
        short,
        input.chars().count(),
        max_chars
    )
}

fn mask_kaspa_addresses(input: &str) -> String {
    let mut output = String::new();

    for token in input.split_whitespace() {
        let cleaned = token
            .trim_matches(|c: char| {
                c == '<'
                    || c == '>'
                    || c == ','
                    || c == '.'
                    || c == ')'
                    || c == '('
                    || c == '['
                    || c == ']'
                    || c == '"'
                    || c == '\''
            })
            .to_string();

        if cleaned.starts_with("kaspa:") || cleaned.starts_with("kaspatest:") {
            output.push_str(&token.replace(&cleaned, &mask_identifier(&cleaned)));
        } else {
            output.push_str(token);
        }

        output.push(' ');
    }

    output.trim_end().to_string()
}

fn mask_identifier(value: &str) -> String {
    let chars = value.chars().collect::<Vec<_>>();

    if chars.len() <= 18 {
        return "***".to_string();
    }

    let start = chars.iter().take(12).collect::<String>();
    let end = chars
        .iter()
        .rev()
        .take(6)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect::<String>();

    format!("{}...{}", start, end)
}

fn mask_common_secret_values(input: &str) -> String {
    let sensitive_keys = [
        "BOT_TOKEN",
        "DATABASE_URL",
        "POSTGRES_URL",
        "DB_URL",
        "NODE_URL_01",
        "NODE_URL",
        "ADMIN_PIN",
        "WEBHOOK_SECRET_TOKEN",
        "COINGECKO_API_URL",
    ];

    let mut lines = Vec::new();

    for line in input.lines() {
        let mut masked = line.to_string();

        for key in sensitive_keys {
            let upper = masked.to_ascii_uppercase();

            if upper.contains(key) && (upper.contains('=') || upper.contains(':')) {
                if let Some(pos) = masked.find('=') {
                    masked = format!("{}=***REDACTED***", masked[..pos].trim_end());
                } else if let Some(pos) = masked.find(':') {
                    masked = format!("{}: ***REDACTED***", masked[..pos].trim_end());
                }
            }
        }

        lines.push(masked);
    }

    lines.join("\n")
}

pub fn html_escape(input: &str) -> String {
    let mut escaped = String::with_capacity(input.len());

    for c in input.chars() {
        match c {
            '&' => escaped.push_str("&amp;"),
            '<' => escaped.push_str("&lt;"),
            '>' => escaped.push_str("&gt;"),
            '"' => escaped.push_str("&quot;"),
            '\'' => escaped.push_str("&#39;"),
            _ => escaped.push(c),
        }
    }

    escaped
}

pub fn is_dangerous_invisible_char(c: char) -> bool {
    matches!(
        c,
        '\u{0000}'
            | '\u{00AD}'
            | '\u{034F}'
            | '\u{061C}'
            | '\u{115F}'
            | '\u{1160}'
            | '\u{17B4}'
            | '\u{17B5}'
            | '\u{180E}'
            | '\u{200B}'..='\u{200F}'
            | '\u{202A}'..='\u{202E}'
            | '\u{2060}'..='\u{206F}'
            | '\u{3164}'
            | '\u{FEFF}'
            | '\u{FFA0}'
    )
}

pub fn contains_dangerous_invisible_chars(input: &str) -> bool {
    input.chars().any(is_dangerous_invisible_char)
}

pub fn contains_html_sensitive_chars(input: &str) -> bool {
    input
        .chars()
        .any(|c| matches!(c, '<' | '>' | '&' | '"' | '\'' | '`'))
}

pub fn normalize_user_text(input: &str) -> String {
    sanitize_user_text(input)
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
}

pub fn normalize_wallet_input(input: &str) -> String {
    sanitize_user_text(input).trim().to_ascii_lowercase()
}

pub fn extract_single_wallet_from_message(input: &str) -> Result<Option<String>, String> {
    let clean = normalize_user_text(input);

    if contains_dangerous_invisible_chars(&clean) {
        return Err("Message contains hidden or unsafe characters.".to_string());
    }

    if clean.starts_with('/') {
        return Ok(None);
    }

    let mut wallets = Vec::new();

    for part in clean.split_whitespace() {
        let candidate = part.trim_matches(|c: char| {
            matches!(c, ',' | '.' | ';' | ':' | ')' | '(' | '[' | ']' | '{' | '}')
        });

        if candidate.starts_with("kaspa:") || candidate.starts_with("kaspatest:") {
            wallets.push(candidate.to_string());
        }
    }

    wallets.sort();
    wallets.dedup();

    if wallets.len() > 1 {
        return Err("Please send only one wallet address per message.".to_string());
    }

    Ok(wallets.into_iter().next())
}

pub fn validate_wallet_security(address: &str) -> Result<(), String> {
    if address.is_empty() {
        return Err("Wallet address is empty.".to_string());
    }

    if address.trim() != address {
        return Err("Wallet address has leading or trailing whitespace.".to_string());
    }

    if address.chars().any(|c| c.is_whitespace()) {
        return Err("Wallet address must not contain spaces or new lines.".to_string());
    }

    if contains_dangerous_invisible_chars(address) {
        return Err("Wallet address contains hidden or unsafe characters.".to_string());
    }

    if contains_html_sensitive_chars(address) {
        return Err("Wallet address contains unsafe HTML-sensitive characters.".to_string());
    }

    if !(address.starts_with("kaspa:") || address.starts_with("kaspatest:")) {
        return Err("Wallet address must start with kaspa: or kaspatest:.".to_string());
    }

    kaspa_addresses::Address::try_from(address)
        .map_err(|_| "Invalid Kaspa wallet address.".to_string())?;

    Ok(())
}

pub fn sanitize_event_text_for_storage(input: &str) -> String {
    let mut text = sanitize_for_log(input);

    if !verbose_logs_enabled() {
        text = mask_telegram_usernames(&text);
        text = mask_hex_hash_like_tokens(&text);
    }

    truncate_for_log(&text)
}

fn mask_telegram_usernames(input: &str) -> String {
    let mut output = String::new();

    for token in input.split_whitespace() {
        let cleaned = token.trim_matches(|c: char| {
            matches!(
                c,
                ',' | '.' | ':' | ';' | ')' | '(' | '[' | ']' | '{' | '}' | '"' | '\''
            )
        });

        if cleaned.starts_with('@') && cleaned.len() > 2 {
            output.push_str(&token.replace(cleaned, "@***"));
        } else {
            output.push_str(token);
        }

        output.push(' ');
    }

    output.trim_end().to_string()
}

fn mask_hex_hash_like_tokens(input: &str) -> String {
    let mut output = String::new();

    for token in input.split_whitespace() {
        let cleaned = token.trim_matches(|c: char| {
            matches!(
                c,
                ',' | '.' | ':' | ';' | ')' | '(' | '[' | ']' | '{' | '}' | '"' | '\''
            )
        });

        let is_hash_like = cleaned.len() >= 48
            && cleaned.len() <= 128
            && cleaned.chars().all(|c| c.is_ascii_hexdigit());

        if is_hash_like {
            output.push_str(&token.replace(cleaned, &mask_identifier(cleaned)));
        } else {
            output.push_str(token);
        }

        output.push(' ');
    }

    output.trim_end().to_string()
}

pub fn validate_raw_message_size(text: &str) -> Result<(), String> {
    let max_chars = max_raw_message_chars();
    let actual_chars = text.chars().count();

    if actual_chars > max_chars {
        return Err(format!(
            "Message is too long. Limit: {} chars. Received: {} chars.",
            max_chars, actual_chars
        ));
    }

    Ok(())
}

pub fn validate_wallet_address_size(address: &str) -> Result<(), String> {
    let max_chars = max_wallet_address_chars();
    let actual_chars = address.chars().count();

    if actual_chars > max_chars {
        return Err(format!(
            "Wallet address is too long. Limit: {} chars. Received: {} chars.",
            max_chars, actual_chars
        ));
    }

    Ok(())
}

// === End logging privacy helpers (Stage 4) ===
