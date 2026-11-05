#!/usr/bin/env bash
# shellcheck disable=SC2024
set -Eeuo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/opqual/lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"
init_run
require_phase create_lock
record_op EXEC01 PLANNED "SIGTERM exact owned candidate while real app advisory-lock wait remains blocked; prove no new work and clean DB/task drain"

if is_dry_run; then
  info "DRY-RUN: would send SIGTERM only to recorded task-owned app PID/container"
  exit 0
fi

APP_ID_EXPECTED="$(state_get app_container_id)"
APP_PID_EXPECTED="$(state_get app_host_pid)"
[[ -n "$APP_ID_EXPECTED" && -n "$APP_PID_EXPECTED" ]] || die "missing app ownership receipt"
assert_owned_container "$APP_CONTAINER"
[[ "$(sudo -n docker inspect "$APP_CONTAINER" --format '{{.Id}}')" == "$APP_ID_EXPECTED" ]] || die "app container identity mismatch"
APP_PID_NOW="$(sudo -n docker inspect "$APP_CONTAINER" --format '{{.State.Pid}}')"
APP_RUNNING="$(sudo -n docker inspect "$APP_CONTAINER" --format '{{.State.Running}}')"

if state_is exec01.sigterm_sent true; then
  info "SIGTERM already recorded; verifying prior effect without resending"
  "$SCRIPT_DIR/verify-shutdown.sh"
  exit 0
fi

if [[ "$APP_RUNNING" != true || "$APP_PID_NOW" != "$APP_PID_EXPECTED" ]]; then
  record_op EXEC01 UNKNOWN "app not running or PID changed before SIGTERM receipt; refusing to replay signal"
  state_set exec01.result UNKNOWN
  die "cannot establish signal was never sent; reconcile evidence before retry"
fi
SIGTERM_UTC="$(date -u -Ins)"
SIGTERM_NS="$(mono_ns)"
record_op EXEC01_SIGNAL PLANNED "signal=SIGTERM container=$APP_CONTAINER host_pid=$APP_PID_NOW monotonic_ns=$SIGTERM_NS"
printf '{"utc":"%s","monotonic_ns":%s,"signal":"SIGTERM","container":"%s","host_pid":%s,"phase":"PLANNED"}\n' \
  "$SIGTERM_UTC" "$SIGTERM_NS" "$APP_CONTAINER" "$APP_PID_NOW" >> "$RUN_DIR/signal-timeline.jsonl"

sudo -n docker kill --signal=TERM "$APP_CONTAINER" > "$RECEIPTS_DIR/sigterm-container.txt"
state_set exec01.sigterm_sent true
state_set exec01.sigterm_monotonic_ns "$SIGTERM_NS"
state_set exec01.sigterm_utc "$SIGTERM_UTC"
record_op EXEC01_SIGNAL VERIFIED "SIGTERM accepted by Docker for exact task-owned app container"
printf '{"utc":"%s","monotonic_ns":%s,"signal":"SIGTERM","container":"%s","host_pid":%s,"phase":"VERIFIED"}\n' \
  "$SIGTERM_UTC" "$SIGTERM_NS" "$APP_CONTAINER" "$APP_PID_NOW" >> "$RUN_DIR/signal-timeline.jsonl"

# Admission proof: create work only after the signal receipt. It must never be claimed.
EXISTING_POST="$(pg_admin -At -c "SELECT count(*) FROM telegram_delivery_queue WHERE event_key='opqual-v1:post-sigterm';")"
if [[ "$EXISTING_POST" == 0 ]]; then
  WALLET_ID="$(awk -F= '/^wallet_identity=/{print $2}' "$RUN_DIR/synthetic-lock-identity.txt")"
  pg_admin -v chat="$SYNTH_CHAT_ID" -v wid="$WALLET_ID" <<'SQL'
INSERT INTO telegram_delivery_queue(chat_id,message_html,status,wallet_masked,event_key,next_attempt_at)
VALUES (:chat::bigint,'<b>OPQUAL post-SIGTERM admission proof</b>','pending',:'wid','opqual-v1:post-sigterm',NOW());
SQL
elif [[ "$EXISTING_POST" != 1 ]]; then
  state_set exec01.result FAIL
  die "unexpected duplicate post-SIGTERM rows: $EXISTING_POST"
fi

"$SCRIPT_DIR/verify-shutdown.sh"
mark_phase exec01_shutdown VERIFIED
record_op EXEC01 VERIFIED "shutdown half of EXEC-01 passed; clean restart remains required"
info "EXEC01_SHUTDOWN=PASS"
