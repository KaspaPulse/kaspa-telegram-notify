use anyhow::{Context, Result, bail};
use std::env;
use std::net::IpAddr;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct WebhookConfig {
    pub domain: String,
    pub port: u16,
    pub bind_ip: IpAddr,
    pub secret_token: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StartupConfig {
    pub database_url: String,
    pub node_url: String,
    pub app_env: String,
    pub db_max_connections: u32,
    pub verbose_logs: bool,
    pub allow_runtime_schema_ensure: bool,
    pub bot_token: String,
    pub telegram_api_url: Option<reqwest::Url>,
    pub admin_user_id: u64,
    pub admin_chat_id: i64,
    pub use_webhook: bool,
    pub webhook: Option<WebhookConfig>,
    pub shutdown_drain_secs: u64,
}

impl StartupConfig {
    pub fn from_env() -> Result<Self> {
        Self::from_lookup(|key| env::var(key).ok())
    }

    fn from_lookup<F>(mut lookup: F) -> Result<Self>
    where
        F: FnMut(&str) -> Option<String>,
    {
        let database_url = required(&mut lookup, "DATABASE_URL")?;
        let node_url = required(&mut lookup, "NODE_URL_01")?;
        let bot_token = required(&mut lookup, "BOT_TOKEN")?;
        let app_env = optional(&mut lookup, "APP_ENV").unwrap_or_else(|| "production".to_string());
        let telegram_api_url = optional(&mut lookup, "TELEGRAM_API_URL")
            .map(|raw| {
                raw.parse::<reqwest::Url>()
                    .with_context(|| "TELEGRAM_API_URL must be a valid absolute URL")
            })
            .transpose()?;

        if !app_env.eq_ignore_ascii_case("ci") && telegram_api_url.is_some() {
            bail!("TELEGRAM_API_URL is test-only and requires APP_ENV=ci");
        }

        if app_env.eq_ignore_ascii_case("ci") {
            let url = telegram_api_url
                .as_ref()
                .context("APP_ENV=ci requires TELEGRAM_API_URL")?;
            let host = url
                .host_str()
                .and_then(|value| value.parse::<IpAddr>().ok())
                .context("APP_ENV=ci requires TELEGRAM_API_URL to use a numeric loopback host")?;
            if url.scheme() != "http"
                || !host.is_loopback()
                || url.port().is_none()
                || !url.username().is_empty()
                || url.password().is_some()
            {
                bail!(
                    "APP_ENV=ci requires an explicit http loopback TELEGRAM_API_URL with no credentials"
                );
            }
            if !bot_token.ends_with(":TEST_PLACEHOLDER_TOKEN_NOT_REAL") {
                bail!("APP_ENV=ci requires the documented synthetic BOT_TOKEN fixture");
            }
        }

        let db_max_connections = parse_u32_range(
            optional(&mut lookup, "DB_MAX_CONNECTIONS").as_deref(),
            "DB_MAX_CONNECTIONS",
            10,
            2,
            50,
        )?;
        let verbose_logs = parse_bool(
            optional(&mut lookup, "ENABLE_VERBOSE_LOGS").as_deref(),
            "ENABLE_VERBOSE_LOGS",
            false,
        )?;
        let allow_runtime_schema_ensure = parse_bool(
            optional(&mut lookup, "ALLOW_RUNTIME_SCHEMA_ENSURE").as_deref(),
            "ALLOW_RUNTIME_SCHEMA_ENSURE",
            false,
        )?;
        let use_webhook = parse_bool(
            optional(&mut lookup, "USE_WEBHOOK").as_deref(),
            "USE_WEBHOOK",
            false,
        )?;
        let shutdown_drain_secs = parse_u64_range(
            optional(&mut lookup, "SHUTDOWN_DRAIN_SECS").as_deref(),
            "SHUTDOWN_DRAIN_SECS",
            3,
            0,
            30,
        )?;

        let legacy_admin_id = optional(&mut lookup, "ADMIN_ID");
        let admin_user_id_raw = optional(&mut lookup, "ADMIN_USER_ID")
            .or_else(|| legacy_admin_id.clone())
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "ADMIN_USER_ID must be set (or ADMIN_ID for backward compatibility)"
                )
            })?;
        let admin_chat_id_raw = optional(&mut lookup, "ADMIN_CHAT_ID")
            .or(legacy_admin_id)
            .ok_or_else(|| {
                anyhow::anyhow!(
                    "ADMIN_CHAT_ID must be set (or ADMIN_ID for backward compatibility)"
                )
            })?;

        let admin_user_id = admin_user_id_raw
            .parse::<u64>()
            .with_context(|| "ADMIN_USER_ID must be a positive Telegram user ID")?;
        let admin_chat_id = admin_chat_id_raw
            .parse::<i64>()
            .with_context(|| "ADMIN_CHAT_ID must be a numeric Telegram chat ID")?;
        let admin_private_chat_id = i64::try_from(admin_user_id).map_err(|_| {
            anyhow::anyhow!("ADMIN_USER_ID is outside the supported Telegram ID range")
        })?;

        if admin_user_id == 0 || admin_chat_id <= 0 || admin_chat_id != admin_private_chat_id {
            bail!("Admin commands require a private chat: ADMIN_CHAT_ID must equal ADMIN_USER_ID");
        }

        let webhook = if use_webhook {
            let domain = required(&mut lookup, "WEBHOOK_DOMAIN")?;
            let port = parse_u16_nonzero(
                optional(&mut lookup, "WEBHOOK_PORT").as_deref(),
                "WEBHOOK_PORT",
                8443,
            )?;
            let bind_raw =
                optional(&mut lookup, "WEBHOOK_BIND").unwrap_or_else(|| "127.0.0.1".to_string());
            let bind_ip = bind_raw
                .parse::<IpAddr>()
                .with_context(|| "WEBHOOK_BIND must be a valid IP address")?;
            let secret_token = required(&mut lookup, "WEBHOOK_SECRET_TOKEN")?;

            Some(WebhookConfig {
                domain,
                port,
                bind_ip,
                secret_token,
            })
        } else {
            None
        };

        Ok(Self {
            database_url,
            node_url,
            app_env,
            db_max_connections,
            verbose_logs,
            allow_runtime_schema_ensure,
            bot_token,
            telegram_api_url,
            admin_user_id,
            admin_chat_id,
            use_webhook,
            webhook,
            shutdown_drain_secs,
        })
    }
}

fn optional<F>(lookup: &mut F, key: &str) -> Option<String>
where
    F: FnMut(&str) -> Option<String>,
{
    lookup(key)
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}

fn required<F>(lookup: &mut F, key: &str) -> Result<String>
where
    F: FnMut(&str) -> Option<String>,
{
    optional(lookup, key).ok_or_else(|| anyhow::anyhow!("{key} must be set and non-empty"))
}

fn parse_bool(value: Option<&str>, key: &str, default: bool) -> Result<bool> {
    let Some(value) = value else {
        return Ok(default);
    };

    match value.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Ok(true),
        "false" | "0" | "no" | "off" => Ok(false),
        _ => bail!("{key} must be one of: true, false, 1, 0, yes, no, on, off"),
    }
}

fn parse_u32_range(
    value: Option<&str>,
    key: &str,
    default: u32,
    min: u32,
    max: u32,
) -> Result<u32> {
    let value = match value {
        Some(raw) => raw
            .parse::<u32>()
            .with_context(|| format!("{key} must be an unsigned integer"))?,
        None => default,
    };

    if !(min..=max).contains(&value) {
        bail!("{key} must be between {min} and {max}");
    }

    Ok(value)
}

fn parse_u64_range(
    value: Option<&str>,
    key: &str,
    default: u64,
    min: u64,
    max: u64,
) -> Result<u64> {
    let value = match value {
        Some(raw) => raw
            .parse::<u64>()
            .with_context(|| format!("{key} must be an unsigned integer"))?,
        None => default,
    };

    if !(min..=max).contains(&value) {
        bail!("{key} must be between {min} and {max}");
    }

    Ok(value)
}

fn parse_u16_nonzero(value: Option<&str>, key: &str, default: u16) -> Result<u16> {
    let value = match value {
        Some(raw) => raw
            .parse::<u16>()
            .with_context(|| format!("{key} must be a valid u16 port"))?,
        None => default,
    };

    if value == 0 {
        bail!("{key} must be greater than zero");
    }

    Ok(value)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn load(values: &[(&str, &str)]) -> Result<StartupConfig> {
        let values = values
            .iter()
            .map(|(key, value)| ((*key).to_string(), (*value).to_string()))
            .collect::<HashMap<_, _>>();

        StartupConfig::from_lookup(|key| values.get(key).cloned())
    }

    fn required_values() -> Vec<(&'static str, &'static str)> {
        vec![
            ("DATABASE_URL", "postgres://localhost/kaspa"),
            ("NODE_URL_01", "wss://node.example.invalid/json"),
            ("BOT_TOKEN", "123456:test"),
            ("ADMIN_USER_ID", "42"),
            ("ADMIN_CHAT_ID", "42"),
        ]
    }

    #[test]
    fn explicit_private_admin_identity_is_loaded() {
        let config = load(&required_values()).expect("valid config");

        assert_eq!(config.admin_user_id, 42);
        assert_eq!(config.admin_chat_id, 42);
        assert_eq!(config.db_max_connections, 10);
        assert!(!config.use_webhook);
        assert!(config.webhook.is_none());
    }

    #[test]
    fn legacy_admin_id_still_populates_private_admin_identity() {
        let values = [
            ("DATABASE_URL", "postgres://localhost/kaspa"),
            ("NODE_URL_01", "wss://node.example.invalid/json"),
            ("BOT_TOKEN", "123456:test"),
            ("ADMIN_ID", "42"),
        ];
        let config = load(&values).expect("valid legacy config");
        assert_eq!(config.admin_user_id, 42);
        assert_eq!(config.admin_chat_id, 42);
    }

    #[test]
    fn ci_requires_loopback_telegram_api_and_synthetic_token() {
        let mut values = required_values();
        values.retain(|(key, _)| *key != "BOT_TOKEN");
        values.extend([
            ("BOT_TOKEN", "1234567890:TEST_PLACEHOLDER_TOKEN_NOT_REAL"),
            ("APP_ENV", "ci"),
        ]);

        let missing = load(&values).expect_err("CI without an API override must fail");
        assert!(missing.to_string().contains("TELEGRAM_API_URL"));

        values.push(("TELEGRAM_API_URL", "http://203.0.113.1:9/"));
        let non_loopback = load(&values).expect_err("CI must reject non-loopback API hosts");
        assert!(non_loopback.to_string().contains("loopback"));

        values.pop();
        values.push(("TELEGRAM_API_URL", "http://127.0.0.1:9/"));
        let config = load(&values).expect("loopback-only CI Telegram API is valid");
        assert!(config.telegram_api_url.is_some());
    }

    #[test]
    fn telegram_api_override_is_forbidden_outside_ci() {
        let mut values = required_values();
        values.push(("TELEGRAM_API_URL", "http://127.0.0.1:9/"));

        let error = load(&values).expect_err("non-CI API override must fail closed");
        assert!(
            error
                .to_string()
                .contains("TELEGRAM_API_URL is test-only and requires APP_ENV=ci")
        );
    }

    #[test]
    fn invalid_boolean_fails_closed() {
        let mut values = required_values();
        values.push(("USE_WEBHOOK", "sometimes"));

        let error = load(&values).expect_err("invalid boolean must fail");
        assert!(error.to_string().contains("USE_WEBHOOK"));
    }

    #[test]
    fn invalid_database_pool_size_is_rejected() {
        let mut values = required_values();
        values.push(("DB_MAX_CONNECTIONS", "500"));

        let error = load(&values).expect_err("out-of-range pool size must fail");
        assert!(error.to_string().contains("DB_MAX_CONNECTIONS"));
    }

    #[test]
    fn webhook_mode_requires_complete_typed_settings() {
        let mut values = required_values();
        values.push(("USE_WEBHOOK", "true"));
        values.push(("WEBHOOK_DOMAIN", "bot.example.invalid"));

        let error = load(&values).expect_err("webhook secret is required");
        assert!(error.to_string().contains("WEBHOOK_SECRET_TOKEN"));
    }

    #[test]
    fn webhook_settings_parse_into_typed_values() {
        let mut values = required_values();
        values.extend([
            ("USE_WEBHOOK", "true"),
            ("WEBHOOK_DOMAIN", "bot.example.invalid"),
            ("WEBHOOK_PORT", "9443"),
            ("WEBHOOK_BIND", "127.0.0.2"),
            ("WEBHOOK_SECRET_TOKEN", "secret-token-value"),
            ("SHUTDOWN_DRAIN_SECS", "7"),
        ]);

        let config = load(&values).expect("valid webhook config");
        let webhook = config.webhook.expect("webhook config");

        assert_eq!(webhook.port, 9443);
        assert_eq!(webhook.bind_ip.to_string(), "127.0.0.2");
        assert_eq!(config.shutdown_drain_secs, 7);
    }
}
