use reqwest::{Client, StatusCode};
use serde_json::{Value, json};
use std::fs::{self, File};
use std::io::Write;
use std::path::PathBuf;
use std::time::Duration;

const MAX_ATTEMPTS: u32 = 5;

fn telegram_test_method_url(token: &str, method: &str) -> String {
    format!("https://api.telegram.org/bot{token}/test/{method}")
}

fn redact_secret(text: &str, secret: &str) -> String {
    if secret.is_empty() {
        return text.to_owned();
    }
    text.replace(secret, "<redacted>")
}

fn retry_delay_seconds(body: &Value, attempt: u32) -> u64 {
    body.pointer("/parameters/retry_after")
        .and_then(Value::as_u64)
        .unwrap_or_else(|| 1_u64 << attempt.saturating_sub(1))
        .clamp(1, 30)
}

async fn telegram_call(
    client: &Client,
    token: &str,
    method: &str,
    payload: &Value,
) -> Result<Value, String> {
    let url = telegram_test_method_url(token, method);

    for attempt in 1..=MAX_ATTEMPTS {
        let response = match client.post(&url).json(payload).send().await {
            Ok(response) => response,
            Err(_) if attempt < MAX_ATTEMPTS => {
                tokio::time::sleep(Duration::from_secs(1_u64 << (attempt - 1))).await;
                continue;
            }
            Err(_) => {
                return Err(
                    "Telegram Test Environment transport failed after bounded retries".to_owned(),
                );
            }
        };

        let status = response.status();
        let bytes = response
            .bytes()
            .await
            .map_err(|_| "Telegram Test Environment response body could not be read".to_owned())?;
        let envelope: Value = serde_json::from_slice(&bytes)
            .map_err(|_| "Telegram Test Environment returned non-JSON data".to_owned())?;

        let error_code = envelope.get("error_code").and_then(Value::as_u64);
        let transient = status == StatusCode::TOO_MANY_REQUESTS
            || status.is_server_error()
            || error_code == Some(429)
            || error_code.is_some_and(|code| code >= 500);

        if transient && attempt < MAX_ATTEMPTS {
            tokio::time::sleep(Duration::from_secs(retry_delay_seconds(&envelope, attempt))).await;
            continue;
        }

        if !status.is_success() || envelope.get("ok").and_then(Value::as_bool) != Some(true) {
            let description = envelope
                .get("description")
                .and_then(Value::as_str)
                .unwrap_or("provider rejected the request");
            return Err(format!(
                "Telegram Test Environment method {method} failed with HTTP {}: {}",
                status.as_u16(),
                redact_secret(description, token)
            ));
        }

        return envelope
            .get("result")
            .cloned()
            .ok_or_else(|| format!("Telegram Test Environment method {method} omitted result"));
    }

    Err("Telegram Test Environment exhausted its bounded retry policy".to_owned())
}

fn valid_bot_command(command: &str) -> bool {
    (1..=32).contains(&command.len())
        && command
            .bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
}

fn write_external_evidence(value: &Value) {
    let Ok(path) = std::env::var("EXTERNAL_CONTRACT_EVIDENCE_PATH") else {
        return;
    };
    let path = PathBuf::from(path);
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent).expect("external evidence directory must be creatable");
    }
    let tmp = path.with_extension("json.tmp");
    let bytes = serde_json::to_vec_pretty(value).expect("external evidence must serialize");
    let mut file = File::create(&tmp).expect("external evidence temp file must be creatable");
    file.write_all(&bytes)
        .expect("external evidence temp file must be writable");
    file.write_all(b"\n")
        .expect("external evidence newline must be writable");
    file.sync_all()
        .expect("external evidence temp file must fsync");
    fs::rename(&tmp, &path).expect("external evidence rename must succeed");
}

#[test]
fn telegram_test_url_uses_official_test_path_order() {
    assert_eq!(
        telegram_test_method_url("123456:TEST_TOKEN", "getMe"),
        "https://api.telegram.org/bot123456:TEST_TOKEN/test/getMe"
    );
}

#[test]
fn telegram_test_error_redaction_never_exposes_token() {
    let token = "123456:THIS_MUST_NEVER_APPEAR";
    let text = format!("provider URL contained {token}");
    let redacted = redact_secret(&text, token);
    assert!(!redacted.contains(token));
    assert!(redacted.contains("<redacted>"));
}

#[test]
fn telegram_command_shape_matches_bot_api_contract() {
    assert!(valid_bot_command("network"));
    assert!(valid_bot_command("wallets_2"));
    assert!(!valid_bot_command(""));
    assert!(!valid_bot_command("Not-Lowercase"));
    assert!(!valid_bot_command(&"a".repeat(33)));
}

#[tokio::test]
#[ignore = "requires dedicated Telegram Test Environment bot and private test-user chat"]
async fn telegram_test_environment_contract() {
    let token = std::env::var("TELEGRAM_TEST_BOT_TOKEN")
        .expect("TELEGRAM_TEST_BOT_TOKEN is required for the trusted external contract gate");
    let chat_id: i64 = std::env::var("TELEGRAM_TEST_CHAT_ID")
        .expect("TELEGRAM_TEST_CHAT_ID is required for the trusted external contract gate")
        .parse()
        .expect("TELEGRAM_TEST_CHAT_ID must be a signed integer");

    let client = Client::builder()
        .connect_timeout(Duration::from_secs(5))
        .timeout(Duration::from_secs(20))
        .build()
        .expect("Telegram test HTTP client must build");

    let me = telegram_call(&client, &token, "getMe", &json!({}))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        me.get("is_bot").and_then(Value::as_bool),
        Some(true),
        "Telegram Test Environment getMe must identify a bot"
    );
    let bot_id = me
        .get("id")
        .and_then(Value::as_i64)
        .expect("Telegram Test Environment bot id must be present");
    let bot_username = me
        .get("username")
        .and_then(Value::as_str)
        .expect("Telegram Test Environment bot username must be present");
    assert!(
        !bot_username.trim().is_empty(),
        "Telegram Test Environment bot username must not be empty"
    );

    let commands = telegram_call(&client, &token, "getMyCommands", &json!({}))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    let commands = commands
        .as_array()
        .expect("Telegram Test Environment getMyCommands must return an array");
    for command in commands {
        let name = command
            .get("command")
            .and_then(Value::as_str)
            .expect("Telegram command must include command");
        let description = command
            .get("description")
            .and_then(Value::as_str)
            .expect("Telegram command must include description");
        assert!(valid_bot_command(name), "Telegram command shape is invalid");
        assert!(
            !description.trim().is_empty() && description.chars().count() <= 256,
            "Telegram command description is outside the Bot API contract"
        );
    }

    let webhook = telegram_call(&client, &token, "getWebhookInfo", &json!({}))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert!(
        webhook.is_object() && webhook.get("url").and_then(Value::as_str).is_some(),
        "Telegram Test Environment getWebhookInfo must return the documented object shape"
    );

    let chat = telegram_call(&client, &token, "getChat", &json!({"chat_id": chat_id}))
        .await
        .unwrap_or_else(|error| panic!("{error}"));
    assert_eq!(
        chat.get("id").and_then(Value::as_i64),
        Some(chat_id),
        "Telegram Test Environment getChat must resolve the configured private test chat"
    );
    assert_eq!(
        chat.get("type").and_then(Value::as_str),
        Some("private"),
        "Telegram Test Environment chat must belong to the dedicated private test user"
    );

    println!(
        "TELEGRAM_TEST_ENVIRONMENT_CONTRACT=PASS bot_id={bot_id} commands={}",
        commands.len()
    );

    write_external_evidence(&json!({
        "schema_version": "1.0.0",
        "provider": "telegram-test-environment",
        "result": "PASS",
        "tested_sha": std::env::var("GITHUB_SHA").unwrap_or_else(|_| "local".to_owned()),
        "bot_id": bot_id,
        "bot_username": bot_username,
        "commands_count": commands.len(),
        "chat_verified": true,
        "chat_id_recorded": false,
        "webhook_info_verified": true,
        "methods": ["getMe", "getMyCommands", "getWebhookInfo", "getChat"]
    }));
}
