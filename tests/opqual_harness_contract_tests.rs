use std::fs;
use std::path::Path;

const ROOT: &str = env!("CARGO_MANIFEST_DIR");

fn read(rel: &str) -> String {
    fs::read_to_string(Path::new(ROOT).join(rel)).unwrap_or_else(|e| panic!("{rel}: {e}"))
}

#[test]
fn opqual_entrypoint_and_phases_exist() {
    for rel in [
        "scripts/opqual/run.sh",
        "scripts/opqual/preflight.sh",
        "scripts/opqual/provision.sh",
        "scripts/opqual/migrate.sh",
        "scripts/opqual/fixtures.sh",
        "scripts/opqual/start-app.sh",
        "scripts/opqual/create-lock.sh",
        "scripts/opqual/exec01.sh",
        "scripts/opqual/verify-shutdown.sh",
        "scripts/opqual/restart.sh",
        "scripts/opqual/run-scenarios.sh",
        "scripts/opqual/webhook-cycle.sh",
        "scripts/opqual/reward-task-cycle.sh",
        "scripts/opqual/edge001-full-cycle.sh",
        "scripts/opqual/collect-evidence.sh",
        "scripts/opqual/cleanup.sh",
    ] {
        assert!(Path::new(ROOT).join(rel).is_file(), "missing {rel}");
    }
}

#[test]
fn deterministic_identity_and_isolation_contract_are_hard_coded() {
    let common = read("scripts/opqual/lib/common.sh");
    for expected in [
        "kaspa-telegram-opqual-v1",
        "kp-opqual-v1",
        "kp-opqual-postgres18",
        "kaspa_opqual_v1",
        "kaspa_pulse_app",
        "kaspa-opqual-v1",
    ] {
        assert!(
            common.contains(expected),
            "missing deterministic identity {expected}"
        );
    }
    let provision = read("scripts/opqual/provision.sh");
    assert!(provision.contains("docker network create --internal"));
    assert!(!provision.contains("--publish"));
    assert!(!provision.contains(" -p "));
}
#[test]
fn migration_contract_uses_repository_migrations_only() {
    let migrate = read("scripts/opqual/migrate.sh");
    let common = read("scripts/opqual/lib/common.sh");
    assert!(migrate.contains("$REPO_ROOT/migrations"));
    assert!(migrate.contains("sort"));
    assert!(
        common.contains("-v ON_ERROR_STOP=1"),
        "shared psql helper must fail at first SQL/migration error"
    );
    assert!(migrate.contains("migration-receipt.json"));
    let opqual = Path::new(ROOT).join("scripts/opqual");
    let mut sql_files = Vec::new();
    fn visit(dir: &Path, out: &mut Vec<String>) {
        for entry in fs::read_dir(dir).unwrap() {
            let path = entry.unwrap().path();
            if path.is_dir() {
                visit(&path, out);
            } else if path.extension().and_then(|x| x.to_str()) == Some("sql") {
                out.push(path.display().to_string());
            }
        }
    }
    visit(&opqual, &mut sql_files);
    assert!(
        sql_files.is_empty(),
        "duplicated migration SQL in harness: {sql_files:?}"
    );
}

#[test]
fn candidate_mismatch_and_cleanup_are_fail_closed() {
    let common = read("scripts/opqual/lib/common.sh");
    assert!(common.contains("qualified binary SHA mismatch"));
    assert!(common.contains("application source differs from expected candidate"));
    assert!(common.contains("assert_owned_container"));
    assert!(common.contains("assert_owned_network"));
    assert!(!common.contains("docker rm -f"));
    assert!(!common.contains("kill -9"));
    let cleanup = read("scripts/opqual/cleanup.sh");
    assert!(cleanup.contains("docker kill --signal=TERM"));
    assert!(cleanup.contains("unrelated_baseline_resources_preserved"));
    assert!(!cleanup.contains("--signal=KILL"));
}

#[test]
fn resume_and_signal_replay_guards_are_explicit() {
    let run = read("scripts/opqual/run.sh");
    assert!(run.contains("--resume"));
    assert!(run.contains("phase receipt and current side effects agree; skipped replay"));
    assert!(run.contains("refusing blind replay"));
    let exec01 = read("scripts/opqual/exec01.sh");
    assert!(exec01.contains("SIGTERM already recorded; verifying prior effect without resending"));
    assert!(exec01.contains("state_set exec01.sigterm_sent true"));
}
#[test]
fn fixtures_support_required_failure_modes_without_public_transport() {
    let telegram = read("scripts/opqual/fixtures/telegram_bot_api.py");
    let http = read("scripts/opqual/fixtures/http_providers.py");
    let kaspa = read("scripts/opqual/fixtures/kaspa_wrpc.py");
    for mode in [
        "success",
        "malformed",
        "timeout",
        "4xx",
        "5xx",
        "rate_limit",
        "invalid",
    ] {
        assert!(
            telegram.contains(mode) || http.contains(mode) || kaspa.contains(mode),
            "missing fixture mode {mode}"
        );
    }
    let fixtures = read("scripts/opqual/fixtures.sh");
    assert!(fixtures.contains("--network-alias api.telegram.org"));
    assert!(fixtures.contains("--network-alias api.kaspa.org"));
    assert!(fixtures.contains("--network-alias api.coingecko.com"));
    assert!(!fixtures.contains("--publish"));
}

#[test]
fn exec01_requires_real_application_lock_and_zero_db_state() {
    let lock = read("scripts/opqual/create-lock.sh");
    assert!(lock.contains("telegram_delivery_queue"));
    assert!(lock.contains("pg_advisory_lock"));
    assert!(lock.contains("pg_advisory_xact_lock"));
    assert!(lock.contains("real application backend did not enter advisory-lock wait"));
    let verify = read("scripts/opqual/verify-shutdown.sh");
    assert!(
        verify.contains("PostgreSQL app backend/locks did not converge to zero after process exit")
    );
    assert!(verify.contains("\"$SESS_FINAL\" == 0"));
    assert!(verify.contains("\"$LOCKS_FINAL\" == 0"));
    assert!(verify.contains("external conflicting lock was released too early"));
    assert!(verify.contains("new work was admitted after SIGTERM"));
}

#[test]
fn evidence_pipeline_keeps_runtime_and_audit_results_distinct() {
    let collect = read("scripts/opqual/collect-evidence.sh");
    for name in [
        "FINAL_RESULT.json",
        "SHA256SUMS.txt",
        "shutdown-verification.json",
        "restart-verification.json",
        "scenario-results.csv",
        "cleanup-receipt.json",
    ] {
        assert!(
            collect.contains(name),
            "evidence output not required: {name}"
        );
    }
    assert!(collect.contains("'exec_01':exec01") || collect.contains("'exec_01': exec01"));
    assert!(collect.contains("'audit_status':audit") || collect.contains("'audit_status': audit"));
    assert!(collect.contains("full_bot_journeys_completed"));
    assert!(
        collect.contains(">= 150"),
        "COMPLETE must require all 150 dependent full journeys"
    );
    assert!(collect.contains("cumulative-scenario-summary.json"));
    assert!(collect.contains("scenario_summary_source"));
}

#[test]
fn scenario_mapping_is_input_not_inferred_pass() {
    let runner = read("scripts/opqual/scenario_runner.py");
    assert!(runner.contains("VERIFIED_PASS"));
    assert!(runner.contains("VERIFIED_FAIL"));
    assert!(runner.contains("BLOCKED"));
    assert!(runner.contains("NOT_APPLICABLE"));
    assert!(!runner.contains("inferred PASS"));
    let map = read("scripts/opqual/fixtures/scenario-map.csv");
    assert!(map.contains("EDGE-002"));
    assert!(map.contains("FLOW-LIFE-shutdown"));
}
#[test]
fn edge001_full_cycle_is_full_dispatcher_and_fail_closed() {
    let edge = read("scripts/opqual/edge001-full-cycle.sh");
    assert!(edge.contains("require_phase start_app"));
    assert!(edge.contains("message --user-id"));
    assert!(edge.contains("--text /fees"));
    assert!(edge.contains("run_case missing missing"));
    assert!(edge.contains("run_case negative invalid"));
    assert!(edge.contains("Fee estimates are temporarily unavailable"));
    assert!(edge.contains("invalid fee values rendered as live estimates"));
    assert!(edge.contains("FULL_MAIN_DISPATCHER"));
}
