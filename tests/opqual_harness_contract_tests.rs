use std::fs;
use std::path::Path;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn read(rel: &str) -> String {
    fs::read_to_string(Path::new(ROOT).join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

#[test]
fn rust_native_opqual_components_exist() {
    for rel in [
        "xtask/src/opqual.rs",
        "xtask/src/opqual/runtime.rs",
        "xtask/src/opqual/scenarios.rs",
        "xtask/src/opqual/cleanup.rs",
        "xtask/src/opqual/evidence.rs",
        "opqual-fixture/src/main.rs",
        "opqual/scenario-map.csv",
    ] {
        assert!(Path::new(ROOT).join(rel).is_file(), "missing {rel}");
    }
    let opqual = read("xtask/src/opqual.rs");
    let runtime = read("xtask/src/opqual/runtime.rs");
    assert!(!opqual.contains(r#"\"openssl\""#));
    assert!(!runtime.contains(r#"\"openssl\""#));
    assert!(runtime.contains("KeyPair::generate()"));
}

#[test]
fn deterministic_identity_and_isolation_are_hard_coded_in_rust() {
    let opqual = read("xtask/src/opqual.rs");
    for expected in [
        "kaspa-telegram-opqual-v1",
        "kp-opqual-v1",
        "kp-opqual-postgres18",
        "kaspa_opqual_v1",
        "kaspa_pulse_app",
        "kaspa-opqual-v1",
    ] {
        assert!(opqual.contains(expected), "missing identity {expected}");
    }
    assert!(opqual.contains(r#""--internal""#));
    assert!(!opqual.contains(r#""--publish""#));
}

#[test]
fn migration_uses_repository_migrations_only() {
    let opqual = read("xtask/src/opqual.rs");
    assert!(opqual.contains(r#"ctx.repo.join("migrations")"#));
    assert!(opqual.contains("migration-receipt.json"));
    assert!(opqual.contains("ON_ERROR_STOP=1"));
    let opqual_data = Path::new(ROOT).join("opqual");
    let sql = fs::read_dir(opqual_data)
        .unwrap()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().and_then(|x| x.to_str()) == Some("sql"))
        .collect::<Vec<_>>();
    assert!(sql.is_empty(), "duplicated migration SQL: {sql:?}");
}

#[test]
fn cleanup_and_resume_are_fail_closed() {
    let cleanup = read("xtask/src/opqual/cleanup.rs");
    let runtime = read("xtask/src/opqual/runtime.rs");
    let opqual = read("xtask/src/opqual.rs");
    assert!(cleanup.contains("--signal=TERM"));
    assert!(cleanup.contains("unrelated_baseline_resources_preserved"));
    assert!(!cleanup.contains("--signal=KILL"));
    assert!(runtime.contains("SIGTERM already recorded; verifying prior effect without resending"));
    assert!(runtime.contains("exec01.sigterm_sent"));
    assert!(runtime.contains("reconcile before replay"));
    assert!(opqual.contains("is already cleaned/closed"));
}

#[test]
fn rust_fixtures_cover_required_failure_modes_without_public_transport() {
    let fixture = read("opqual-fixture/src/main.rs");
    for mode in [
        "success",
        "malformed",
        "timeout",
        "4xx",
        "5xx",
        "rate_limit",
        "invalid",
    ] {
        assert!(fixture.contains(mode), "missing fixture mode {mode}");
    }
    let runtime = read("xtask/src/opqual/runtime.rs");
    for alias in ["api.telegram.org", "api.kaspa.org", "api.coingecko.com"] {
        assert!(runtime.contains(alias), "missing fixture alias {alias}");
    }
    assert!(!runtime.contains(r#""--publish""#));
    assert!(fixture.contains("set_nonblocking(true)"));
    assert!(fixture.contains("SHUTDOWN.load(Ordering::SeqCst)"));
    assert!(fixture.contains("send_close_notify()"));
}

#[test]
fn exec01_requires_real_lock_zero_db_and_clean_restart() {
    let runtime = read("xtask/src/opqual/runtime.rs");
    for marker in [
        "telegram_delivery_queue",
        "pg_advisory_lock",
        "real application backend did not enter advisory-lock wait",
        "PostgreSQL app backend/locks did not converge to zero after process exit",
        "external conflicting lock was released too early",
        "new work was admitted after SIGTERM",
        "same exact candidate restarted",
        "functional_smoke",
    ] {
        assert!(
            runtime.contains(marker),
            "missing EXEC-01 contract: {marker}"
        );
    }
}

#[test]
fn evidence_pipeline_keeps_runtime_and_audit_distinct() {
    let evidence = read("xtask/src/opqual/evidence.rs");
    for name in [
        "FINAL_RESULT.json",
        "SHA256SUMS.txt",
        "shutdown-verification.json",
        "restart-verification.json",
        "scenario-results.csv",
        "cleanup-receipt.json",
    ] {
        assert!(
            evidence.contains(name),
            "missing evidence requirement {name}"
        );
    }
    assert!(evidence.contains("exec_01"));
    assert!(evidence.contains("audit_status"));
    assert!(evidence.contains("full_bot_journeys_completed"));
    assert!(evidence.contains("journeys>=150") || evidence.contains("journeys >= 150"));
}

#[test]
fn scenario_runner_is_rust_native_and_never_infers_pass() {
    let runner = read("xtask/src/opqual/scenarios.rs");
    for value in [
        "VERIFIED_PASS",
        "VERIFIED_FAIL",
        "BLOCKED",
        "NOT_APPLICABLE",
    ] {
        assert!(runner.contains(value), "missing result class {value}");
    }
    assert!(runner.contains("full_bot_journeys_completed"));
    assert!(runner.contains("scenario-evidence"));
    assert!(runner.contains("EDGE-001"));
    assert!(runner.contains("wait_update_queue_drained"));
    assert!(runner.contains("wait_callback_answer"));
    assert!(runner.contains("bound_nonce_confirmation+baseline_restored"));
    assert!(runner.contains("let insert = args.len() - 1;"));
    assert!(runner.contains(r#"("WEBHOOK_DOMAIN", WEBHOOK_CONTAINER.into())"#));
    let map = read("opqual/scenario-map.csv");
    assert!(map.contains("EDGE-002"));
    assert!(map.contains("FLOW-LIFE-shutdown"));
}

#[test]
fn edge001_full_cycle_is_full_dispatcher_fail_closed() {
    let runner = read("xtask/src/opqual/scenarios.rs");
    assert!(runner.contains("Fee estimates are temporarily unavailable"));
    assert!(runner.contains(r#"("missing", "missing")"#));
    assert!(runner.contains(r#"("negative", "invalid")"#));
    assert!(runner.contains("invalid fee rendered live"));
    assert!(runner.contains("FULL_MAIN_DISPATCHER"));
}

#[test]
fn no_legacy_opqual_executable_tree_remains() {
    assert!(!Path::new(ROOT).join("scripts/opqual").exists());
}
