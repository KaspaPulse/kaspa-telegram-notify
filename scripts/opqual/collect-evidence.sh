#!/usr/bin/env bash
set -Eeuo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/opqual/lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"
init_run
record_op COLLECT_EVIDENCE PLANNED "finalize evidence, classify harness/EXEC-01/audit independently, write SHA256 manifest"

capture_docker_inventory
python3 - <<'PY'
import json, os
run = os.environ['RUN_DIR']
with open(os.environ['STATE_FILE']) as f:
    state = json.load(f)
summary = {
    'total': 195, 'verified_pass': 40, 'verified_fail': 0,
    'blocked': 149, 'not_applicable': 6, 'not_tested': 0,
    'full_bot_journeys_completed': 0,
}
cumulative_sp = os.path.join(run, 'cumulative-scenario-summary.json')
sp = cumulative_sp if os.path.exists(cumulative_sp) else os.path.join(run, 'scenario-summary.json')
if os.path.exists(sp):
    with open(sp) as f:
        summary.update(json.load(f))
exec01 = ('PASS' if state.get('exec01.result') == 'PASS' and
          state.get('exec01.clean_restart') == 'true' else
          'FAIL' if state.get('exec01.result') == 'FAIL' else 'NOT_COMPLETE')
audit = ('COMPLETE' if exec01 == 'PASS' and summary.get('blocked', 0) == 0 and
         summary.get('verified_fail', 0) == 0 and summary.get('not_tested', 0) == 0 and
         summary.get('full_bot_journeys_completed', 0) >= 150
         else 'PARTIAL')
core = ['preflight','provision','migrate','fixtures','start_app','create_lock',
        'verify_shutdown','restart','scenarios']
harness_runtime = 'PASS' if all(state.get('phase.' + x) == 'VERIFIED' for x in core) else 'PARTIAL'
required_success = [
    'environment.json','source-identity.json','binary-identity.json','docker-inventory.json',
    'network-map.json','migration-receipt.json','synthetic-identities.json','fixture-hashes.json',
    'app.stdout.log','app.stderr.log','postgres-activity.jsonl','postgres-locks.jsonl',
    'signal-timeline.jsonl','shutdown-verification.json','restart-verification.json',
    'scenario-results.csv','cleanup-receipt.json',
]
missing = [name for name in required_success if not os.path.exists(os.path.join(run, name))]
result = {
    'task': os.environ['TASK_ID'], 'run_id': os.environ['RUN_ID'],
    'harness_runtime_execution': harness_runtime, 'exec_01': exec01,
    'audit_status': audit, 'scenario_summary': summary,
    'cleanup': 'PASS' if state.get('phase.cleanup') == 'VERIFIED' else 'NOT_COMPLETE',
    'expected_source_head': os.environ['EXPECTED_SOURCE_HEAD'],
    'expected_binary_sha256': os.environ['EXPECTED_BINARY_SHA256'],
    'forced_kill_used': 'YES' if state.get('exec01.forced_kill_used') == 'true' else 'NO',
    'missing_success_evidence': missing,
    'success_evidence_complete': (not missing) if harness_runtime == 'PASS' else False,
    'scenario_summary_source': os.path.basename(sp),
    'supplemental_runs': summary.get('supplemental_runs', []),
}
with open(os.path.join(run, 'FINAL_RESULT.json'), 'w') as f:
    json.dump(result, f, indent=2, sort_keys=True)
    f.write('\n')
PY

find "$RUN_DIR" -type f ! -name SHA256SUMS.txt -print0 | sort -z | xargs -0 sha256sum > "$RUN_DIR/SHA256SUMS.txt"
mark_phase collect_evidence VERIFIED
record_op COLLECT_EVIDENCE VERIFIED "FINAL_RESULT.json and SHA256SUMS.txt written"
info "EVIDENCE_FINALIZED=$RUN_DIR"