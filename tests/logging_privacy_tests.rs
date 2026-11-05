use kaspa_pulse::utils::{sanitize_event_text_for_storage, sanitize_for_log};

#[test]
fn log_sanitizer_masks_kaspa_wallets_by_default() {
    let raw = "wallet kaspa:qabcdefghijklmnopqrstuvwxyz1234567890abcdef";
    let clean = sanitize_for_log(raw);

    assert!(!clean.contains("abcdefghijklmnopqrstuvwxyz1234567890abcdef"));
}

#[test]
fn event_storage_sanitizer_masks_usernames_and_hashes_by_default() {
    let raw = "@username mined 0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef";
    let clean = sanitize_event_text_for_storage(raw);

    assert!(!clean.contains("@username"));
    assert!(!clean.contains("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"));
}
