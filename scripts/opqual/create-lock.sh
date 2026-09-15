#!/usr/bin/env bash
# shellcheck disable=SC2024
set -Eeuo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/opqual/lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"
init_run
require_phase start_app
record_op CREATE_LOCK PLANNED "hold conflicting advisory lock, enqueue synthetic delivery, prove real app backend waits in acquire_delivery_claim_guard"

if is_dry_run; then info "DRY-RUN: would create application-owned advisory-lock wait"; exit 0; fi
assert_owned_container "$APP_CONTAINER"
[[ "$(sudo -n docker inspect "$APP_CONTAINER" --format '{{.State.Running}}')" == true ]] || die "application not running"

WALLET_TOKEN="$(python3 - <<PY
import hashlib
print(hashlib.sha256(('delivery-wallet-v1:'+'$SYNTH_WALLET').encode()).digest()[:16].hex())
PY
)"
WALLET_ID="v2:$WALLET_TOKEN"
printf 'chat_id=%s\nwallet=%s\nwallet_identity=%s\n' "$SYNTH_CHAT_ID" "$SYNTH_WALLET" "$WALLET_ID" \
  > "$RUN_DIR/synthetic-lock-identity.txt"

if container_exists "$LOCK_CONTAINER"; then
  assert_owned_container "$LOCK_CONTAINER"
  [[ "$(sudo -n docker inspect "$LOCK_CONTAINER" --format '{{.State.Running}}')" == true ]] \
    || die "stale stopped lock-holder requires manual evidence reconciliation"
else
  sudo -n docker run -d --name "$LOCK_CONTAINER" --network "$DOCKER_NETWORK" \
    --label "task=$TASK_ID" --label purpose=external-lock-holder \
    -e "PGAPPNAME=$LOCK_APP_NAME" --entrypoint psql "$POSTGRES_IMAGE" \
    -h "$POSTGRES_CONTAINER" -U "$OBSERVER_ROLE" -d "$DATABASE" -X -v ON_ERROR_STOP=1 \
    -c "SELECT pg_backend_pid(); SELECT pg_advisory_lock($SYNTH_CHAT_ID); SELECT pg_sleep(3600);" \
    > "$RECEIPTS_DIR/lock-holder-container-id.txt"
fi
assert_no_published_ports "$LOCK_CONTAINER"
for i in $(seq 1 80); do
  OBS_GRANTED="$(pg_admin -At -c "SELECT count(*) FROM pg_locks l JOIN pg_stat_activity a ON a.pid=l.pid WHERE l.locktype='advisory' AND l.granted AND a.application_name='$LOCK_APP_NAME';")"
  [[ "$OBS_GRANTED" -gt 0 ]] && break
  [[ "$i" -lt 80 ]] || die "external advisory lock not acquired"
  sleep 0.25
done

# Reconcile an interrupted run before inserting: never create a duplicate event key blindly.
EXISTING="$(pg_admin -At -c "SELECT count(*) FROM telegram_delivery_queue WHERE event_key='opqual-v1:blocked-lock';")"
if [[ "$EXISTING" == 0 ]]; then
  pg_admin -v chat="$SYNTH_CHAT_ID" -v wid="$WALLET_ID" <<'SQL'
INSERT INTO telegram_delivery_queue(chat_id,message_html,status,wallet_masked,event_key,next_attempt_at)
VALUES (:chat::bigint,'<b>OPQUAL synthetic lock proof</b>','pending',:'wid','opqual-v1:blocked-lock',NOW());
SQL
elif [[ "$EXISTING" != 1 ]]; then
  die "unexpected duplicate lock-proof queue rows: $EXISTING"
fi

MATCH=0
for i in $(seq 1 80); do
  MATCH="$(pg_admin -At -c "
WITH obs AS (
 SELECT l.database,l.classid,l.objid,l.objsubid,a.pid
 FROM pg_locks l JOIN pg_stat_activity a ON a.pid=l.pid
 WHERE l.locktype='advisory' AND l.granted AND a.application_name='$LOCK_APP_NAME'
), app AS (
 SELECT l.database,l.classid,l.objid,l.objsubid,a.pid,a.wait_event_type,a.wait_event,a.query
 FROM pg_locks l JOIN pg_stat_activity a ON a.pid=l.pid
 WHERE l.locktype='advisory' AND NOT l.granted AND a.application_name='$APPLICATION_NAME'
)
SELECT count(*) FROM obs JOIN app USING(database,classid,objid,objsubid)
WHERE app.wait_event_type='Lock' AND app.query LIKE '%pg_advisory_xact_lock%';")"
  [[ "$MATCH" -gt 0 ]] && break
  [[ "$i" -lt 80 ]] || die "real application backend did not enter advisory-lock wait"
  sleep 0.25
done
pg_admin -P pager=off -c "
SELECT q.id,q.chat_id,q.status,q.locked_by,q.locked_at,q.event_key
FROM telegram_delivery_queue q WHERE q.event_key='opqual-v1:blocked-lock';
SELECT a.pid,a.usename,a.application_name,a.state,a.wait_event_type,a.wait_event,
       l.granted,l.database,l.classid,l.objid,l.objsubid,left(a.query,180) AS query
FROM pg_stat_activity a JOIN pg_locks l ON l.pid=a.pid
WHERE l.locktype='advisory' AND a.application_name IN ('$APPLICATION_NAME','$LOCK_APP_NAME')
ORDER BY l.classid,l.objid,l.granted DESC,a.application_name;" > "$RUN_DIR/pre-sigterm-lock-proof.txt"

grep -q 'processing' "$RUN_DIR/pre-sigterm-lock-proof.txt" || die "queue row not processing"
grep -q "$APPLICATION_NAME" "$RUN_DIR/pre-sigterm-lock-proof.txt" || die "application lock waiter missing"
grep -q "$LOCK_APP_NAME" "$RUN_DIR/pre-sigterm-lock-proof.txt" || die "external lock holder missing"

python3 - <<'PY'
import json,os,subprocess,datetime
cmd=['sudo','-n','docker','exec','-i','-e','PGAPPNAME='+os.environ['OBSERVER_APP_NAME'],os.environ['POSTGRES_CONTAINER'],
     'psql','-X','-U',os.environ['PG_ADMIN'],'-d',os.environ['DATABASE'],'-At','-F','\t','-c',
     "SELECT a.pid,a.application_name,a.state,coalesce(a.wait_event_type,''),coalesce(a.wait_event,''),l.granted,l.classid,l.objid,l.objsubid FROM pg_stat_activity a JOIN pg_locks l ON l.pid=a.pid WHERE l.locktype='advisory' AND a.application_name IN ('%s','%s') ORDER BY a.application_name,l.granted DESC"%(os.environ['APPLICATION_NAME'],os.environ['LOCK_APP_NAME'])]
out=subprocess.check_output(cmd,text=True)
path=os.path.join(os.environ['RUN_DIR'],'postgres-locks.jsonl')
with open(path,'a') as f:
 for line in out.splitlines():
  p=line.split('\t'); f.write(json.dumps({'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'pid':int(p[0]),'application_name':p[1],'state':p[2],'wait_event_type':p[3],'wait_event':p[4],'granted':p[5]=='t','classid':p[6],'objid':p[7],'objsubid':p[8]})+'\n')
PY
mark_phase create_lock VERIFIED
state_set exec01.lock_wait_proven true
record_op CREATE_LOCK VERIFIED "real delivery worker is waiting on application-owned advisory lock while external task lock remains granted"
info "CREATE_LOCK=PASS"
