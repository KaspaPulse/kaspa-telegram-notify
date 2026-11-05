#![no_main]

use kaspa_pulse::utils::{
    clean_for_log, contains_dangerous_invisible_chars, extract_single_wallet_from_message,
    html_escape, normalize_user_text, normalize_wallet_input, sanitize_callback_data_for_log,
    sanitize_event_text_for_storage, sanitize_for_log, sanitize_user_text,
    validate_raw_message_size, validate_wallet_address_size, validate_wallet_security,
};
use libfuzzer_sys::fuzz_target;

fuzz_target!(|data: &[u8]| {
    let input = String::from_utf8_lossy(data);
    let input = input.as_ref();

    let sanitized = sanitize_user_text(input);
    assert!(!contains_dangerous_invisible_chars(&sanitized));
    assert!(!sanitized.contains('\r'));

    let normalized = normalize_user_text(input);
    assert!(!contains_dangerous_invisible_chars(&normalized));
    assert!(!normalized.contains('\r'));

    let escaped = html_escape(input);
    assert!(!escaped.contains('<'));
    assert!(!escaped.contains('>'));
    assert!(!escaped.contains('"'));
    assert!(!escaped.contains('\''));

    let wallet = normalize_wallet_input(input);
    assert!(!contains_dangerous_invisible_chars(&wallet));

    let _ = clean_for_log(input);
    let _ = sanitize_for_log(input);
    let _ = sanitize_callback_data_for_log(input);
    let _ = sanitize_event_text_for_storage(input);
    let _ = extract_single_wallet_from_message(input);
    let _ = validate_raw_message_size(input);
    let _ = validate_wallet_address_size(input);
    let _ = validate_wallet_security(&wallet);
});
