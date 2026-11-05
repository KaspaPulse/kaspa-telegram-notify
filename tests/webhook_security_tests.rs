use kaspa_pulse::infrastructure::webhook_security::{
    validate_webhook_bind_policy_value, validate_webhook_domain, validate_webhook_secret,
};
use std::net::IpAddr;

#[test]
fn rejects_short_webhook_secret() {
    assert!(validate_webhook_secret("short").is_err());
}

#[test]
fn accepts_strong_webhook_secret() {
    let secret = "L6jqmXyz0elohPuGwV7u4gwGjRf16kfBwNG9q-oHDAE";
    assert!(validate_webhook_secret(secret).is_ok());
}

#[test]
fn rejects_domain_with_scheme_or_path() {
    assert!(validate_webhook_domain("https://api.example.com").is_err());
    assert!(validate_webhook_domain("api.example.com/webhook").is_err());
}

#[test]
fn production_rejects_public_webhook_bind_by_default() {
    let public_ip: IpAddr = "0.0.0.0".parse().unwrap();
    assert!(validate_webhook_bind_policy_value("production", public_ip, false).is_err());

    let loopback: IpAddr = "127.0.0.1".parse().unwrap();
    assert!(validate_webhook_bind_policy_value("production", loopback, false).is_ok());
}

#[test]
fn production_allows_explicit_public_webhook_bind_override() {
    let public_ip: IpAddr = "0.0.0.0".parse().unwrap();
    assert!(validate_webhook_bind_policy_value("production", public_ip, true).is_ok());
}
