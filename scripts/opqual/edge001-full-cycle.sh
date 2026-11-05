#!/usr/bin/env bash
set -Eeuo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/opqual/lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"
init_run
require_phase start_app
record_op EDGE001_FULL PLANNED "full-main /fees invalid-payload journey for missing and negative HTTP-200 fee responses"

if is_dry_run; then
  info "DRY-RUN: would execute EDGE-001 through exact full dispatcher twice"
  exit 0
fi
assert_owned_container "$APP_CONTAINER"
[[ "$(sudo -n docker inspect "$APP_CONTAINER" --format '{{.State.Running}}')" == true ]] \
  || die "application is not running"

DRIVER="$OPQUAL_DIR/fixtures/scenario_driver.py"
HTTP_EVENTS="$EVENTS_DIR/http-provider-events.jsonl"
TELEGRAM_EVENTS="$EVENTS_DIR/telegram-events.jsonl"
RESULT_DIR="$RUN_DIR/edge001-full"
export HTTP_EVENTS TELEGRAM_EVENTS RESULT_DIR
mkdir -p "$RESULT_DIR"
restore_control() {
  printf '{}\n' > "$CONTROL_DIR/http.json"
}
trap restore_control EXIT

event_count_file() {
  local path="$1"
  OPQUAL_EVENT_PATH="$path" python3 - <<'PY'
import os
from pathlib import Path
p=Path(os.environ['OPQUAL_EVENT_PATH'])
print(sum(1 for line in p.read_text(encoding='utf-8').splitlines() if line.strip()) if p.exists() else 0)
PY
}

run_case() {
  local label="$1" mode="$2"
  local http_start telegram_start update_id
  python3 - "$CONTROL_DIR/http.json" "$mode" <<'PY'
import json,sys
path,mode=sys.argv[1:]
with open(path,'w',encoding='utf-8') as f:
    json.dump({'api.kaspa.org/info/fee-estimate':{'mode':mode}},f,sort_keys=True)
    f.write('\n')
PY
  http_start="$(event_count_file "$HTTP_EVENTS")"
  telegram_start="$(event_count_file "$TELEGRAM_EVENTS")"
  update_id="$(python3 "$DRIVER" --control-dir "$CONTROL_DIR" --event-dir "$EVENTS_DIR" \
    message --user-id "$SYNTH_USER_ID" --chat-id "$SYNTH_CHAT_ID" --text /fees)"
  python3 "$DRIVER" --control-dir "$CONTROL_DIR" --event-dir "$EVENTS_DIR" \
    wait-text --start "$telegram_start" --contains 'Fee estimates are temporarily unavailable' --timeout 15 \
    > "$RESULT_DIR/$label-telegram.json"

  OPQUAL_HTTP_START="$http_start" OPQUAL_MODE="$mode" OPQUAL_LABEL="$label" python3 - <<'PY'
import json,os
from pathlib import Path
p=Path(os.environ['HTTP_EVENTS'])
rows=[]
if p.exists():
    for line in p.read_text(encoding='utf-8').splitlines()[int(os.environ['OPQUAL_HTTP_START']):]:
        if line.strip(): rows.append(json.loads(line))
matched=[r for r in rows if r.get('host')=='api.kaspa.org' and r.get('path')=='/info/fee-estimate']
if not matched: raise SystemExit('fee provider request not observed')
r=matched[-1]
if r.get('mode')!=os.environ['OPQUAL_MODE'] or r.get('status')!=200:
    raise SystemExit(f'unexpected fee provider event: {r}')
out=Path(os.environ['RESULT_DIR'])/(os.environ['OPQUAL_LABEL']+'-http.json')
out.write_text(json.dumps(r,indent=2,sort_keys=True)+'\n')
PY

  OPQUAL_TG_START="$telegram_start" OPQUAL_LABEL="$label" OPQUAL_UPDATE_ID="$update_id" python3 - <<'PY'
import json,os
from pathlib import Path
p=Path(os.environ['TELEGRAM_EVENTS'])
rows=[]
if p.exists():
    for line in p.read_text(encoding='utf-8').splitlines()[int(os.environ['OPQUAL_TG_START']):]:
        if line.strip(): rows.append(json.loads(line))
text='\n'.join(str(r.get('text','')) for r in rows)
if 'Fee estimates are temporarily unavailable' not in text:
    raise SystemExit('fallback Telegram output missing')
if '<b>Priority:</b>' in text or 'sompi/gram' in text:
    raise SystemExit('invalid fee values rendered as live estimates')
out={'update_id':int(os.environ['OPQUAL_UPDATE_ID']),'telegram_events':rows}
(Path(os.environ['RESULT_DIR'])/(os.environ['OPQUAL_LABEL']+'-proof.json')).write_text(json.dumps(out,indent=2,sort_keys=True)+'\n')
PY
}
run_case missing missing
run_case negative invalid
restore_control
trap - EXIT

python3 - <<'PY'
import json,os
from pathlib import Path
root=Path(os.environ['RESULT_DIR'])
out={
  'scenario':'EDGE-001',
  'execution_level':'FULL_MAIN_DISPATCHER',
  'source_head':os.environ['EXPECTED_SOURCE_HEAD'],
  'binary_sha256':os.environ['EXPECTED_BINARY_SHA256'],
  'missing_case':'PASS',
  'negative_case':'PASS',
  'real_provider_traffic':False,
  'synthetic_only':True,
}
(root/'EDGE-001-full-dispatcher.json').write_text(json.dumps(out,indent=2,sort_keys=True)+'\n')
PY
mark_phase edge001_full VERIFIED
record_op EDGE001_FULL VERIFIED "full main /fees dispatcher produced fallback only for missing and negative HTTP-200 fee payloads"
info "EDGE001_FULL_DISPATCHER=PASS"
