#!/usr/bin/env bash
# shellcheck disable=SC2024
set -Eeuo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/opqual/lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"
init_run
require_phase verify_shutdown
record_op RESTART PLANNED "release external test lock only after zero proof; restart exact same container and perform readiness/DB/Telegram smoke"

if is_dry_run; then info "DRY-RUN: would release test lock and restart exact same candidate"; exit 0; fi
assert_owned_container "$APP_CONTAINER"
[[ "$(sudo -n docker inspect "$APP_CONTAINER" --format '{{.State.Running}}')" == false ]] || die "application still running before restart"

LOCK_PID="$(pg_admin -At -c "SELECT pid FROM pg_stat_activity WHERE application_name='$LOCK_APP_NAME' ORDER BY pid LIMIT 1;")"
[[ -n "$LOCK_PID" ]] || die "external lock backend missing before controlled release"
pg_admin -c "SELECT pg_terminate_backend($LOCK_PID);" > "$RUN_DIR/observer-lock-release.txt"
for i in $(seq 1 40); do
  LEFT="$(pg_admin -At -c "SELECT count(*) FROM pg_stat_activity WHERE application_name='$LOCK_APP_NAME';")"
  [[ "$LEFT" == 0 ]] && break
  [[ "$i" -lt 40 ]] || die "external lock session did not release"
  sleep 0.25
done

if container_exists "$LOCK_CONTAINER"; then
  assert_owned_container "$LOCK_CONTAINER"
  for i in $(seq 1 40); do
    [[ "$(sudo -n docker inspect "$LOCK_CONTAINER" --format '{{.State.Running}}')" == false ]] && break
    [[ "$i" -lt 40 ]] || die "lock-holder container did not stop after backend termination"
    sleep 0.25
  done
  sudo -n docker rm "$LOCK_CONTAINER" >/dev/null
fi
pg_admin -c "DELETE FROM telegram_delivery_queue WHERE event_key IN ('opqual-v1:blocked-lock','opqual-v1:post-sigterm');" \
  > "$RUN_DIR/restart-fixture-reset.txt"

RESTART_START_NS="$(mono_ns)"
state_set exec01.restart_start_monotonic_ns "$RESTART_START_NS"
PRE_EVENTS="$(python3 "$OPQUAL_DIR/fixtures/scenario_driver.py" --control-dir "$CONTROL_DIR" --event-dir "$EVENTS_DIR" event-count)"
sudo -n docker start "$APP_CONTAINER" > "$RECEIPTS_DIR/restart-container.txt"
RESTART_PID="$(sudo -n docker inspect "$APP_CONTAINER" --format '{{.State.Pid}}')"
[[ "$RESTART_PID" -gt 1 ]] || die "restart PID invalid"
state_set exec01.restart_host_pid "$RESTART_PID"

READY=""
for i in $(seq 1 60); do
  READY="$(sudo -n docker run --rm -i --network "$DOCKER_NETWORK" "$PYTHON_IMAGE" python - <<PY 2>/dev/null || true
import urllib.request
try: print(urllib.request.urlopen('http://$APP_CONTAINER:$HEALTH_PORT/readyz',timeout=2).read().decode().strip())
except Exception: pass
PY
)"
  [[ "$READY" == ready ]] && break
  [[ "$i" -lt 60 ]] || die "clean restart readiness failed"
  sleep 1
done

sudo -n docker run --rm -i --network "$DOCKER_NETWORK" "$PYTHON_IMAGE" python - <<PY > "$RUN_DIR/restart-readiness-response.txt"
import urllib.request
for p in ('healthz','readyz'):
 with urllib.request.urlopen('http://$APP_CONTAINER:$HEALTH_PORT/'+p,timeout=3) as r:
  print(p,r.status,r.read().decode().strip())
PY
grep -qx 'healthz 200 ok' "$RUN_DIR/restart-readiness-response.txt" || die "restart health failed"
grep -qx 'readyz 200 ready' "$RUN_DIR/restart-readiness-response.txt" || die "restart readiness failed"
SESSION_COUNT="$(pg_admin -At -c "SELECT count(*) FROM pg_stat_activity WHERE application_name='$APPLICATION_NAME';")"
[[ "$SESSION_COUNT" -gt 0 ]] || die "restart DB connectivity/application_name missing"

python3 "$OPQUAL_DIR/fixtures/scenario_driver.py" --control-dir "$CONTROL_DIR" --event-dir "$EVENTS_DIR" \
  message --user-id "$SYNTH_USER_ID" --chat-id "$SYNTH_CHAT_ID" --text /help \
  > "$RUN_DIR/restart-smoke-update-id.txt"
python3 "$OPQUAL_DIR/fixtures/scenario_driver.py" --control-dir "$CONTROL_DIR" --event-dir "$EVENTS_DIR" \
  wait-text --start "$PRE_EVENTS" --contains 'Kaspa Pulse Help' --timeout 15 \
  > "$RUN_DIR/restart-functional-smoke.json"

RESTART_END_NS="$(mono_ns)"
RESTART_MS="$(( (RESTART_END_NS-RESTART_START_NS)/1000000 ))"
state_set exec01.restart_end_monotonic_ns "$RESTART_END_NS"
state_set exec01.restart_duration_ms "$RESTART_MS"
state_set exec01.clean_restart true
state_set exec01.result PASS

python3 - <<'PY'
import json,os
state=json.load(open(os.environ['STATE_FILE']))
out={'clean_restart':'PASS','health':'PASS','readiness':'PASS','database_connectivity':'PASS','functional_smoke':'PASS',
     'application_name':os.environ['APPLICATION_NAME'],'restart_host_pid':int(state['exec01.restart_host_pid']),
     'restart_duration_ms':int(state['exec01.restart_duration_ms']),'same_candidate_sha256':os.environ['EXPECTED_BINARY_SHA256']}
with open(os.path.join(os.environ['RUN_DIR'],'restart-verification.json'),'w') as f: json.dump(out,f,indent=2,sort_keys=True); f.write('\n')
PY
mark_phase restart VERIFIED
mark_phase exec01 VERIFIED
record_op RESTART VERIFIED "same exact candidate restarted; health/readiness/DB/Telegram smoke pass"
record_op EXEC01 VERIFIED "long-lock SIGTERM shutdown plus zero sessions/locks, clean exit, no SIGKILL, clean restart all pass"
info "CLEAN_RESTART=PASS EXEC_01=PASS"
