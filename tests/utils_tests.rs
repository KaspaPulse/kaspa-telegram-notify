use kaspa_pulse::utils::{
    format_short_wallet, is_add_wallet_rate_limited, sanitize_callback_data_for_log,
    validate_raw_message_size, validate_wallet_address_size,
};

#[test]
fn masks_long_wallets() {
    let wallet = "kaspa:qq2avyvncscg5dtsk8u4uwjhlr3799dhaqj8k9y6q5y9hpwfxjy6u00pep7vg";
    let masked = format_short_wallet(wallet);
    assert!(masked.starts_with("kaspa:qq2avy"));
    assert!(masked.ends_with("pep7vg"));
    assert!(masked.contains("..."));
}

#[test]
fn validates_message_size() {
    assert!(validate_raw_message_size("short").is_ok());
    assert!(validate_raw_message_size(&"x".repeat(513)).is_err());
}

#[test]
fn validates_wallet_size() {
    assert!(validate_wallet_address_size("kaspa:short").is_ok());
    assert!(validate_wallet_address_size(&format!("kaspa:{}", "x".repeat(121))).is_err());
}

#[test]
fn add_wallet_rate_limit_blocks_burst() {
    let actor_user_id = 9988776655_u64;
    let outcomes = (0..=5)
        .map(|_| is_add_wallet_rate_limited(actor_user_id))
        .collect::<Vec<_>>();

    assert!(outcomes.iter().any(|limited| *limited));
}

#[test]
fn admin_confirmation_nonce_is_redacted_from_callback_logs() {
    let nonce = format!("{:032x}", std::process::id());
    let safe = sanitize_callback_data_for_log(&format!("admin_do:resume:{nonce}"));

    assert_eq!(safe, "admin_do:resume:[REDACTED]");
    assert!(!safe.contains(&nonce));
}

#[test]
fn malformed_admin_callback_is_still_redacted() {
    let safe = sanitize_callback_data_for_log("admin_do:RESUME!:sensitive-value");

    assert_eq!(safe, "admin_do:unknown:[REDACTED]");
    assert!(!safe.contains("sensitive-value"));
}

#[test]
fn ordinary_callback_data_is_preserved() {
    assert_eq!(sanitize_callback_data_for_log("cmd_network"), "cmd_network");
}
