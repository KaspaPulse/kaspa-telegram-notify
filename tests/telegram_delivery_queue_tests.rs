use kaspa_pulse::infrastructure::telegram_delivery_queue::{
    retry_after_seconds, retry_delay_seconds,
};

#[test]
fn retry_after_is_extracted_from_telegram_error() {
    assert_eq!(
        retry_after_seconds("Too Many Requests: retry_after 42"),
        Some(42)
    );
}

#[test]
fn retry_delay_uses_backoff_when_no_retry_after() {
    assert_eq!(retry_delay_seconds(0, "network error"), 5);
    assert_eq!(retry_delay_seconds(1, "network error"), 15);
    assert_eq!(retry_delay_seconds(2, "network error"), 60);
    assert_eq!(retry_delay_seconds(3, "network error"), 300);
}
