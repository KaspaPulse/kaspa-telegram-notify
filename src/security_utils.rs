use std::env;

/// Validates a secondary PIN for destructive or sensitive Admin actions
pub fn verify_admin_pin(provided_pin: &str) -> bool {
    let expected_pin = env::var("ADMIN_PIN").unwrap_or_else(|_| "UNSET_PIN_SECURE_ME".to_string());
    if expected_pin == "UNSET_PIN_SECURE_ME" {
        tracing::warn!("⚠️ CRITICAL: ADMIN_PIN is not set in .env!");
        return false;
    }
    provided_pin.trim() == expected_pin.trim()
}
