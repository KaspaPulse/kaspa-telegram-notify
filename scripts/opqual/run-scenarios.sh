#!/usr/bin/env bash
set -Eeuo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/opqual/lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"
init_run
require_phase restart
record_op SCENARIOS PLANNED "execute current scenario-map BLOCKED rows through real dispatcher/providers/DB contracts; preserve prior qualified rows without inferred pass"

if is_dry_run; then
  python3 - <<'PY'
import csv,os,collections
with open(os.environ['SCENARIO_MATRIX'],encoding='utf-8-sig',newline='') as f: rows=list(csv.DictReader(f))
blocked=[r for r in rows if r.get('outcome')=='BLOCKED']
print(f'DRY-RUN scenarios_total={len(rows)} blocked_inputs={len(blocked)} not_tested=0')
print(collections.Counter(r['feature_ids'].split('-',1)[0] for r in blocked))
PY
  exit 0
fi

set +e
python3 "$SCRIPT_DIR/scenario_runner.py" | tee "$RUN_DIR/scenario-runner.log"
RC=${PIPESTATUS[0]}
set -e
if [[ "$RC" -ne 0 ]]; then
  mark_phase scenarios FAILED
  record_op SCENARIOS FAILED "scenario runner reported one or more VERIFIED_FAIL rows; rc=$RC"
  exit "$RC"
fi
mark_phase scenarios VERIFIED
record_op SCENARIOS VERIFIED "scenario runner completed; see scenario-results.csv for PASS/BLOCKED classifications"
info "SCENARIO_RUNNER=PASS"
