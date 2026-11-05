#!/usr/bin/env bash
set -Eeuo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/opqual/lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"
init_run
require_phase create_lock
state_is exec01.sigterm_sent true || die "SIGTERM has not been recorded"
record_op VERIFY_SHUTDOWN PLANNED "observe process exit, no-new-work, PostgreSQL application sessions/locks zero while external lock remains held"

APP_CONTAINER_ID="$(state_get app_container_id)"
[[ -n "$APP_CONTAINER_ID" ]] || die "missing app container identity"
[[ "$(sudo -n docker inspect "$APP_CONTAINER" --format '{{.Id}}')" == "$APP_CONTAINER_ID" ]] || die "app container identity changed"

touch "$RUN_DIR/postgres-activity.jsonl" "$RUN_DIR/shutdown-timeline.tsv"
if [[ ! -s "$RUN_DIR/shutdown-timeline.tsv" ]]; then
  printf 'utc\tapp_running\tapp_sessions\tapp_locks\texternal_granted\tpost_status\n' >> "$RUN_DIR/shutdown-timeline.tsv"
fi
EXIT_OBSERVED=0
for _ in $(seq 1 120); do
  TS="$(date -u -Ins)"
  RUNNING="$(sudo -n docker inspect "$APP_CONTAINER" --format '{{.State.Running}}')"
  COUNTS="$(pg_admin -At -F $'\t' -c "
SELECT
 (SELECT count(*) FROM pg_stat_activity WHERE application_name='$APPLICATION_NAME'),
 (SELECT count(*) FROM pg_locks l JOIN pg_stat_activity a ON a.pid=l.pid WHERE a.application_name='$APPLICATION_NAME'),
 (SELECT count(*) FROM pg_locks l JOIN pg_stat_activity a ON a.pid=l.pid WHERE a.application_name='$LOCK_APP_NAME' AND l.locktype='advisory' AND l.granted),
 COALESCE((SELECT status||':'||COALESCE(locked_by,'') FROM telegram_delivery_queue WHERE event_key='opqual-v1:post-sigterm'),'missing');")"
  printf '%s\t%s\t%s\n' "$TS" "$RUNNING" "$COUNTS" >> "$RUN_DIR/shutdown-timeline.tsv"
  IFS=$'\t' read -r SESS LOCKS EXT POST <<< "$COUNTS"
  OPQUAL_TS="$TS" OPQUAL_RUNNING="$RUNNING" OPQUAL_SESS="$SESS" OPQUAL_LOCKS="$LOCKS" OPQUAL_EXT="$EXT" OPQUAL_POST="$POST" python3 - <<'PY'
import json,os
with open(os.path.join(os.environ['RUN_DIR'],'postgres-activity.jsonl'),'a') as f:
 f.write(json.dumps({'utc':os.environ['OPQUAL_TS'],'app_running':os.environ['OPQUAL_RUNNING']=='true','application_sessions':int(os.environ['OPQUAL_SESS']),'application_locks':int(os.environ['OPQUAL_LOCKS']),'external_advisory_locks':int(os.environ['OPQUAL_EXT']),'post_sigterm_status':os.environ['OPQUAL_POST']})+'\n')
PY
  if [[ "$RUNNING" == false ]]; then EXIT_OBSERVED=1; break; fi
  sleep 0.25
done
[[ "$EXIT_OBSERVED" == 1 ]] || { state_set exec01.result FAIL; die "application did not exit within 30s; do not SIGKILL for success"; }

# PostgreSQL can retain a disconnected backend that is blocked on a lock briefly after
# the client process exits. Runtime connections set client_connection_check_interval=1s.
# Poll only post-exit DB convergence; do not extend or disguise the process shutdown gate.
ZERO_OBSERVED=0
SESS_FINAL=-1
LOCKS_FINAL=-1
EXT_FINAL=-1
POST_STATE=missing
for _ in $(seq 1 80); do
  TS="$(date -u -Ins)"
  COUNTS="$(pg_admin -At -F $'\t' -c "
SELECT
 (SELECT count(*) FROM pg_stat_activity WHERE application_name='$APPLICATION_NAME'),
 (SELECT count(*) FROM pg_locks l JOIN pg_stat_activity a ON a.pid=l.pid WHERE a.application_name='$APPLICATION_NAME'),
 (SELECT count(*) FROM pg_locks l JOIN pg_stat_activity a ON a.pid=l.pid WHERE a.application_name='$LOCK_APP_NAME' AND l.locktype='advisory' AND l.granted),
 COALESCE((SELECT status||':'||COALESCE(locked_by,'') FROM telegram_delivery_queue WHERE event_key='opqual-v1:post-sigterm'),'missing');")"
  IFS=$'\t' read -r SESS_FINAL LOCKS_FINAL EXT_FINAL POST_STATE <<< "$COUNTS"
  printf '%s\tfalse\t%s\n' "$TS" "$COUNTS" >> "$RUN_DIR/shutdown-timeline.tsv"
  OPQUAL_TS="$TS" OPQUAL_SESS="$SESS_FINAL" OPQUAL_LOCKS="$LOCKS_FINAL" OPQUAL_EXT="$EXT_FINAL" OPQUAL_POST="$POST_STATE" python3 - <<'PY2'
import json,os
with open(os.path.join(os.environ['RUN_DIR'],'postgres-activity.jsonl'),'a') as f:
 f.write(json.dumps({'utc':os.environ['OPQUAL_TS'],'app_running':False,'application_sessions':int(os.environ['OPQUAL_SESS']),'application_locks':int(os.environ['OPQUAL_LOCKS']),'external_advisory_locks':int(os.environ['OPQUAL_EXT']),'post_sigterm_status':os.environ['OPQUAL_POST'],'phase':'post_exit_db_convergence'})+'\n')
PY2
  [[ "$EXT_FINAL" -gt 0 ]] || die "external conflicting lock was released too early"
  [[ "$POST_STATE" == "pending:" ]] || die "new work was admitted after SIGTERM: $POST_STATE"
  if [[ "$SESS_FINAL" == 0 && "$LOCKS_FINAL" == 0 ]]; then ZERO_OBSERVED=1; break; fi
  sleep 0.25
done
[[ "$ZERO_OBSERVED" == 1 ]] || die "PostgreSQL app backend/locks did not converge to zero after process exit: sessions=$SESS_FINAL locks=$LOCKS_FINAL"
state_set exec01.zero_proof_utc "$(date -u -Ins)"

EXIT_CODE="$(sudo -n docker inspect "$APP_CONTAINER" --format '{{.State.ExitCode}}')"
OOM="$(sudo -n docker inspect "$APP_CONTAINER" --format '{{.State.OOMKilled}}')"
[[ "$EXIT_CODE" == 0 ]] || die "application exit code=$EXIT_CODE"
[[ "$OOM" == false ]] || die "application OOM-killed"
for marker in 'Starting graceful shutdown' \
  'All owned background workers and request tasks joined' \
  'Database connections closed safely'; do
  grep -q "$marker" "$RUN_DIR/app.stdout.log" "$RUN_DIR/app.stderr.log" \
    || die "shutdown marker missing: $marker"
done

T0_NS="$(state_get exec01.sigterm_monotonic_ns)"
[[ "$T0_NS" =~ ^[0-9]+$ ]] || die "missing SIGTERM monotonic timestamp"
FINISHED_AT="$(sudo -n docker inspect "$APP_CONTAINER" --format '{{.State.FinishedAt}}')"
SIGTERM_UTC="$(state_get exec01.sigterm_utc)"
SHUTDOWN_MS="$(OPQUAL_FINISHED_AT="$FINISHED_AT" OPQUAL_SIGTERM_UTC="$SIGTERM_UTC" python3 - <<'PY3'
import datetime,os
def parse(value):
    value=value.replace(',', '.')
    if value.endswith('Z'): value=value[:-1]+'+00:00'
    return datetime.datetime.fromisoformat(value)
start=parse(os.environ['OPQUAL_SIGTERM_UTC'])
end=parse(os.environ['OPQUAL_FINISHED_AT'])
print(max(0, int((end-start).total_seconds()*1000)))
PY3
)"
state_set exec01.exit_code "$EXIT_CODE"
state_set exec01.process_finished_at "$FINISHED_AT"
state_set exec01.shutdown_duration_ms "$SHUTDOWN_MS"
state_set exec01.zero_sessions true
state_set exec01.zero_locks true
state_set exec01.clean_exit true
state_set exec01.no_new_work true
grep -Eq 'shutdown requested|Cancellation requested|Shutdown worker joins complete' \
  "$RUN_DIR/app.stdout.log" "$RUN_DIR/app.stderr.log" \
  || die "cancellation/worker-drain marker missing"

python3 - <<'PY'
import json,os
state=json.load(open(os.environ['STATE_FILE']))
out={
 'application_long_sql_lock_shutdown':'PASS',
 'zero_app_db_sessions_after_shutdown':'PASS' if state.get('exec01.zero_sessions')=='true' else 'FAIL',
 'zero_app_locks_after_shutdown':'PASS' if state.get('exec01.zero_locks')=='true' else 'FAIL',
 'no_new_work_after_shutdown':'PASS' if state.get('exec01.no_new_work')=='true' else 'FAIL',
 'clean_exit':'PASS' if state.get('exec01.clean_exit')=='true' else 'FAIL',
 'forced_kill_used':'NO',
 'exit_code':int(state.get('exec01.exit_code','-1')),
 'shutdown_duration_ms':int(state.get('exec01.shutdown_duration_ms','-1')),
 'external_lock_still_held':True,
}
with open(os.path.join(os.environ['RUN_DIR'],'shutdown-verification.json'),'w') as f:
 json.dump(out,f,indent=2,sort_keys=True); f.write('\n')
PY
mark_phase verify_shutdown VERIFIED
state_set exec01.shutdown_pass true
state_set exec01.forced_kill_used false
record_op VERIFY_SHUTDOWN VERIFIED "SIGTERM shutdown closed app sessions/locks cleanly while external lock remained held"
info "VERIFY_SHUTDOWN=PASS duration_ms=$SHUTDOWN_MS"
