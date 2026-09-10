fn main_source() -> String {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("src/main.rs");
    std::fs::read_to_string(path).expect("main source must be readable")
}

#[test]
fn startup_events_report_persistence_failures() {
    let source = main_source();

    assert!(source.contains("Failed to persist system start event"));
    assert!(source.contains("Failed to persist webhook start event"));
    assert!(!source.contains("let _ = db_repo.record_bot_event_record(system_start_event)"));
    assert!(!source.contains("let _ = db_repo.record_bot_event_record(webhook_start_event)"));
}

#[test]
fn node_preflight_reports_fast_failures_and_timeouts() {
    let source = main_source();

    assert!(source.contains("let mut failed_operations = Vec::new()"));
    assert!(source.contains("failed_operations.push(\"get_server_info\")"));
    assert!(source.contains("failed_operations.push(\"connect\")"));
    assert!(source.contains("Node pre-flight completed with failures"));
    assert!(source.contains("Node pre-flight timed out"));
}

#[test]
fn telegram_command_sync_does_not_claim_false_success() {
    let source = main_source();

    assert!(source.contains("telegram_command_sync_errors"));
    assert!(source.contains("Telegram command synchronization operation failed"));
    assert!(source.contains("if telegram_command_sync_errors == 0"));
    assert!(source.contains("Telegram command synchronization completed with errors"));
}

#[test]
fn rustls_crypto_provider_is_installed_before_tls_startup() {
    let source = main_source();
    let install = source
        .find("install_rustls_crypto_provider()?;")
        .expect("Rustls provider installation must be explicit");
    let preflight = source
        .find("Running node pre-flight diagnostic with safe timeouts")
        .expect("node preflight marker must exist");

    assert!(source.contains("rustls::crypto::ring::default_provider()"));
    assert!(source.contains(".install_default()"));
    assert!(
        install < preflight,
        "Rustls provider must be installed before TLS/network preflight"
    );
}
