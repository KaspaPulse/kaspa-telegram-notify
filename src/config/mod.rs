use anyhow::{Context, Result, bail};
use std::env;
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};

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
    pub app_env: String,
    pub db_max_connections: u32,
    pub verbose_logs: bool,
    pub allow_runtime_schema_ensure: bool,
    pub bot_token: String,
    pub admin_user_id: u64,
    pub admin_chat_id: i64,
    pub use_webhook: bool,
    pub webhook: Option<WebhookConfig>,
    pub shutdown_drain_secs: u64,
}

impl StartupConfig {
    pub fn from_env() -> Result<Self> {
        let require_files = parse_bool(
            env::var("REQUIRE_CREDENTIAL_FILES").ok().as_deref(),
            "REQUIRE_CREDENTIAL_FILES",
            false,
        )?;
        let credential_dir = env::var_os("CREDENTIALS_DIRECTORY").map(PathBuf::from);
        if require_files && credential_dir.is_none() {
            bail!("required systemd credential directory is unavailable");
        }
        // systemd LoadCredential exposes files, not environment variables.
        // Read only the three named secrets. No secret values enter logs.
        let loaded = load_systemd_credentials(credential_dir.as_deref(), require_files)?;
        Self::from_lookup(|key| loaded.get(key).cloned().or_else(|| env::var(key).ok()))
    }

    fn from_lookup<F>(mut lookup: F) -> Result<Self>
    where
        F: FnMut(&str) -> Option<String>,
    {
        let database_url = required(&mut lookup, "DATABASE_URL")?;
        // Production is PNN-only; never accept static local or tailnet node overrides.
        for key in [
            "NODE_URL_01",
            "UTXO_STANDBY_NODE_URL",
            "UTXO_STANDBY_NODE_URL_FILE",
        ] {
            if optional(&mut lookup, key).is_some() {
                bail!("{key} is forbidden: Kaspa Block Alert Bot uses public PNN only");
            }
        }
        let bot_token = required(&mut lookup, "BOT_TOKEN")?;
        let app_env = optional(&mut lookup, "APP_ENV").unwrap_or_else(|| "production".to_string());

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
            app_env,
            db_max_connections,
            verbose_logs,
            allow_runtime_schema_ensure,
            bot_token,
            admin_user_id,
            admin_chat_id,
            use_webhook,
            webhook,
            shutdown_drain_secs,
        })
    }
}

fn load_systemd_credentials(
    directory: Option<&Path>,
    require_files: bool,
) -> Result<std::collections::HashMap<&'static str, String>> {
    if require_files && directory.is_none() {
        bail!("required systemd credential directory is unavailable");
    }
    let mut loaded = std::collections::HashMap::new();
    for (key, file) in [
        ("DATABASE_URL", "database_url"),
        ("BOT_TOKEN", "bot_token"),
        ("WEBHOOK_SECRET_TOKEN", "webhook_secret_token"),
    ] {
        let value = match directory {
            Some(dir) => read_credential(dir, file)?,
            None => None,
        };
        if let Some(value) = value {
            loaded.insert(key, value);
        } else if require_files {
            bail!("required systemd credential {file} unavailable or empty");
        }
    }
    Ok(loaded)
}

fn read_credential(directory: &Path, name: &str) -> Result<Option<String>> {
    if !directory.is_absolute() {
        bail!("systemd credential directory must be absolute");
    }
    let dir_meta =
        fs::symlink_metadata(directory).context("systemd credential directory unavailable")?;
    if !dir_meta.is_dir() || dir_meta.file_type().is_symlink() {
        bail!("systemd credential directory is not a real directory");
    }
    // 'name' comes only from the three compile-time literals above.
    let path = directory.join(name);
    let meta = match fs::symlink_metadata(&path) {
        Ok(meta) => meta,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err).context("systemd credential metadata unavailable"),
    };
    if !meta.is_file() || meta.file_type().is_symlink() || meta.len() > 65_536 {
        bail!("systemd credential {name} has invalid file type or size");
    }
    let raw = fs::read_to_string(&path)
        .with_context(|| format!("systemd credential {name} unreadable"))?;
    let value = raw.trim_end_matches(['\r', '\n']).to_string();
    if value.trim().is_empty() {
        bail!("systemd credential {name} unavailable or empty");
    }
    Ok(Some(value))
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
            ("BOT_TOKEN", "123456:test"),
            ("ADMIN_ID", "484901117"),
        ]
    }

    struct CredentialFixture(std::path::PathBuf);

    impl CredentialFixture {
        fn new() -> Self {
            let unique = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock")
                .as_nanos();
            let dir = std::env::temp_dir().join(format!(
                "kaspa-pulse-credential-unit-{}-{unique}",
                std::process::id()
            ));
            std::fs::create_dir(&dir).expect("create isolated test directory");
            Self(dir)
        }

        fn put(&self, name: &str, value: &str) {
            std::fs::write(self.0.join(name), value).expect("write isolated fixture");
        }
    }

    impl Drop for CredentialFixture {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn systemd_credentials_three_names_and_trailing_newline() {
        let fixture = CredentialFixture::new();
        fixture.put("database_url", "postgres://localhost/fixture\n");
        fixture.put("bot_token", "123456:fixture\n");
        fixture.put("webhook_secret_token", "fixture-webhook\r\n");
        let credentials =
            load_systemd_credentials(Some(&fixture.0), true).expect("required credentials");
        assert_eq!(credentials["DATABASE_URL"], "postgres://localhost/fixture");
        assert_eq!(credentials["BOT_TOKEN"], "123456:fixture");
        assert_eq!(credentials["WEBHOOK_SECRET_TOKEN"], "fixture-webhook");
    }

    #[test]
    fn systemd_credentials_missing_and_empty_fail_closed() {
        let fixture = CredentialFixture::new();
        assert!(load_systemd_credentials(Some(&fixture.0), true).is_err());
        fixture.put("database_url", "postgres://localhost/fixture");
        fixture.put("bot_token", "123456:fixture");
        fixture.put("webhook_secret_token", "  \n");
        assert!(load_systemd_credentials(Some(&fixture.0), true).is_err());
        assert!(load_systemd_credentials(None, true).is_err());
    }

    #[test]
    fn systemd_credentials_optional_without_directory() {
        assert!(
            load_systemd_credentials(None, false)
                .expect("legacy mode")
                .is_empty()
        );
    }

    #[cfg(unix)]
    #[test]
    fn systemd_credentials_refuse_symlink() {
        let fixture = CredentialFixture::new();
        fixture.put("database_url", "postgres://localhost/fixture");
        std::os::unix::fs::symlink(fixture.0.join("database_url"), fixture.0.join("bot_token"))
            .expect("symlink fixture");
        assert!(load_systemd_credentials(Some(&fixture.0), true).is_err());
    }

    #[test]
    fn legacy_admin_id_populates_private_admin_identity() {
        let config = load(&required_values()).expect("valid config");

        assert_eq!(config.admin_user_id, 484_901_117);
        assert_eq!(config.admin_chat_id, 484_901_117);
        assert_eq!(config.db_max_connections, 10);
        assert!(!config.use_webhook);
        assert!(config.webhook.is_none());
    }

    #[test]
    fn pnn_only_accepts_no_static_node() {
        let config = load(&required_values()).expect("public PNN needs no static node");
        assert_eq!(config.app_env, "production");
    }

    #[test]
    fn pnn_only_rejects_static_or_standby_node() {
        for key in [
            "NODE_URL_01",
            "UTXO_STANDBY_NODE_URL",
            "UTXO_STANDBY_NODE_URL_FILE",
        ] {
            let mut settings = required_values();
            settings.push((key, "wss://example.invalid"));
            let error = load(&settings).expect_err("static node is forbidden");
            assert!(error.to_string().contains("forbidden"));
        }
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
