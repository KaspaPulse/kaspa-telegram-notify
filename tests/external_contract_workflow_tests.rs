use std::fs;

fn workflow_source() -> String {
    fs::read_to_string(".github/workflows/external-contract-e2e.yml")
        .expect("external contract workflow must exist")
}

#[test]
fn external_contract_workflow_never_runs_on_pull_request_code() {
    let source = workflow_source();

    assert!(source.contains("schedule:"));
    assert!(source.contains("workflow_dispatch:"));
    assert!(!source.contains("pull_request:"));
    assert!(!source.contains("pull_request_target:"));
    assert!(source.contains("permissions:\n  contents: read"));
    assert!(source.contains("persist-credentials: false"));
}

#[test]
fn telegram_external_contract_is_manual_main_only_and_environment_protected() {
    let source = workflow_source();

    assert!(source.contains("github.event_name == 'workflow_dispatch'"));
    assert!(source.contains("inputs.run_telegram == true"));
    assert!(source.contains("github.ref == 'refs/heads/main'"));
    assert!(source.contains("environment: external-contract-e2e"));
    assert!(source.contains("secrets.TELEGRAM_TEST_BOT_TOKEN"));
    assert!(source.contains("secrets.TELEGRAM_TEST_CHAT_ID"));
    assert!(source.contains("Dedicated Telegram Test Environment credentials are not configured"));
    assert!(source.contains("telegram_test_environment_contract -- --ignored --exact --nocapture"));
}

#[test]
fn kaspa_external_contract_is_scheduled_read_only_and_supports_explicit_override() {
    let source = workflow_source();

    assert!(source.contains("Live Kaspa provider contract"));
    assert!(source.contains("KASPA_LIVE_WRPC_URL"));
    assert!(source.contains("vars.KASPA_LIVE_WRPC_URL"));
    assert!(source.contains("KASPA_EXPECTED_SERVER_VERSION"));
    assert!(source.contains("live_latest_kaspa_wrpc_contract -- --ignored --exact --nocapture"));
    assert!(source.contains("target/external-contract-evidence/kaspa-live.json"));
}

#[test]
fn external_contract_workflow_has_bounded_runtime_and_non_canceling_concurrency() {
    let source = workflow_source();

    assert!(source.contains("cancel-in-progress: false"));
    assert!(source.contains("timeout-minutes: 20"));
    assert!(source.contains("timeout-minutes: 15"));
}
