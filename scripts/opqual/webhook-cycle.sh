#!/usr/bin/env bash
# shellcheck disable=SC2024
set -Eeuo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/opqual/lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"
init_run
require_phase fixtures
require_phase migrate
RECEIPT="$RUN_DIR/webhook-cycle.json"
SECRET='OPQUAL_WEBHOOK_SECRET_20260914_0123456789'

if [[ -f "$RECEIPT" ]]; then
  python3 -c 'import json,sys; d=json.load(open(sys.argv[1])); assert d.get("result")=="PASS"' "$RECEIPT"
  info "WEBHOOK_CYCLE=PASS_REUSED"
  exit 0
fi

record_op WEBHOOK_CYCLE PLANNED "launch exact candidate in isolated webhook mode, post synthetic update, prove webhook server task and dispatcher response, stop owned container"
container_exists "$WEBHOOK_CONTAINER" && die "unexpected existing webhook container $WEBHOOK_CONTAINER"

sudo -n docker run -d --name "$WEBHOOK_CONTAINER" \
  --network "$DOCKER_NETWORK" --network-alias "$WEBHOOK_CONTAINER" \
  --label "task=$TASK_ID" --label purpose=webhook-qualification \
  --read-only --security-opt no-new-privileges:true --tmpfs /tmp:rw,noexec,nosuid,size=16m \
  -e APP_ENV=development -e RUST_LOG=info -e ENABLE_VERBOSE_LOGS=true \
  -e "DATABASE_URL=postgresql://$RUNTIME_ROLE@$POSTGRES_CONTAINER:5432/$DATABASE?sslmode=disable&application_name=$WEBHOOK_APPLICATION_NAME" \
  -e DB_MAX_CONNECTIONS=4 -e ALLOW_RUNTIME_SCHEMA_ENSURE=false \
  -e BOT_TOKEN=1234567890:TEST_TOKEN -e ADMIN_ID="$SYNTH_ADMIN_ID" \
  -e ADMIN_USER_ID="$SYNTH_ADMIN_ID" -e ADMIN_CHAT_ID="$SYNTH_ADMIN_ID" \
  -e USE_WEBHOOK=true -e WEBHOOK_DOMAIN="$WEBHOOK_DOMAIN" -e WEBHOOK_PORT="$WEBHOOK_PORT" \
  -e WEBHOOK_BIND=0.0.0.0 -e WEBHOOK_ALLOW_PUBLIC_BIND=true -e WEBHOOK_MAX_CONNECTIONS=5 \
  -e WEBHOOK_SECRET_TOKEN="$SECRET" \
  -e "NODE_URL_01=ws://$NODE_CONTAINER:$NODE_PORT" -e KASPA_MONITOR_MODE=polling_only \
  -e KASPA_MONITOR_POLL_INTERVAL_SECS=2 -e READINESS_REQUIRE_NODE=true -e READINESS_REQUIRE_SUBSCRIPTION=false \
  -e HEALTH_ENDPOINT_ENABLED=true -e HEALTH_BIND=0.0.0.0 -e HEALTH_PORT="$WEBHOOK_HEALTH_PORT" -e HEALTH_ALLOW_PUBLIC_BIND=true \
  -e ENABLE_TELEGRAM_DELIVERY_QUEUE=false -e KAS_PRICE_HISTORY_ENABLED=false \
  -e RPC_TIMEOUT_SECS=5 -e HTTP_TIMEOUT_SECS=5 -e HTTP_CONNECT_TIMEOUT_SECS=2 \
  -e SSL_CERT_FILE=/task/certs/ca.crt \
  --mount "type=bind,src=$BINARY_PATH,dst=/task/kaspa-pulse,readonly" \
  --mount "type=bind,src=$CERTS_DIR,dst=/task/certs,readonly" \
  --entrypoint /task/kaspa-pulse "$POSTGRES_IMAGE" > "$RECEIPTS_DIR/webhook-container-id.txt"
assert_owned_container "$WEBHOOK_CONTAINER"
assert_no_published_ports "$WEBHOOK_CONTAINER"
for i in $(seq 1 60); do
  LOGS="$(sudo -n docker logs "$WEBHOOK_CONTAINER" 2>&1 || true)"
  if grep -q '\[WEBHOOK\] Listening' <<<"$LOGS"; then break; fi
  [[ "$i" -lt 60 ]] || { sudo -n docker logs "$WEBHOOK_CONTAINER" > "$LOGS_DIR/webhook-start-failure.log" 2>&1; die "webhook server did not become ready"; }
  sleep 0.25
done
sudo -n docker logs "$WEBHOOK_CONTAINER" > "$LOGS_DIR/webhook-app.log" 2>&1

event_before="$(wc -l < "$EVENTS_DIR/telegram-events.jsonl")"
export OPQUAL_WEBHOOK_EVENT_BEFORE="$event_before" OPQUAL_WEBHOOK_SECRET="$SECRET"
sudo -n docker run --rm -i --network "$DOCKER_NETWORK" \
  -e OPQUAL_WEBHOOK_SECRET="$SECRET" -e OPQUAL_WEBHOOK_HOST="$WEBHOOK_CONTAINER" -e OPQUAL_WEBHOOK_PORT="$WEBHOOK_PORT" \
  -e OPQUAL_USER_ID="$SYNTH_USER_ID" -e OPQUAL_CHAT_ID="$SYNTH_CHAT_ID" \
  "$PYTHON_IMAGE" python - <<'PY' > "$RUN_DIR/webhook-http-response.txt"
import json,os,time,urllib.request
uid=int(os.environ['OPQUAL_USER_ID']); cid=int(os.environ['OPQUAL_CHAT_ID'])
update={'update_id':990001,'message':{'message_id':990001,'date':int(time.time()),
        'chat':{'id':cid,'type':'private','first_name':'OpQualUser'},
        'from':{'id':uid,'is_bot':False,'first_name':'OpQualUser','username':'opqual_user'},'text':'/help'}}
req=urllib.request.Request(f"http://{os.environ['OPQUAL_WEBHOOK_HOST']}:{os.environ['OPQUAL_WEBHOOK_PORT']}/webhook",
    data=json.dumps(update).encode(),headers={'Content-Type':'application/json','X-Telegram-Bot-Api-Secret-Token':os.environ['OPQUAL_WEBHOOK_SECRET']},method='POST')
with urllib.request.urlopen(req,timeout=5) as r:
    print(r.status); print(r.read().decode())
PY

grep -q '^200$' "$RUN_DIR/webhook-http-response.txt"
for i in $(seq 1 60); do
  if tail -n +$((event_before+1)) "$EVENTS_DIR/telegram-events.jsonl" | grep -q 'Kaspa Pulse Help'; then break; fi
  [[ "$i" -lt 60 ]] || die "webhook update did not reach dispatcher/help handler"
  sleep 0.25
done
sudo -n docker logs "$WEBHOOK_CONTAINER" > "$LOGS_DIR/webhook-app.log" 2>&1
grep -q '\[TASK START\] telegram_webhook_server' "$LOGS_DIR/webhook-app.log" || die "telegram_webhook_server task marker missing"

date -u -Ins > "$RUN_DIR/webhook-term-timestamp.txt"
sudo -n docker kill --signal=TERM "$WEBHOOK_CONTAINER" >/dev/null
for i in $(seq 1 120); do
  [[ "$(sudo -n docker inspect "$WEBHOOK_CONTAINER" --format '{{.State.Running}}')" == false ]] && break
  [[ "$i" -lt 120 ]] || die "webhook app did not stop after TERM"
  sleep 0.25
done
exit_code="$(sudo -n docker inspect "$WEBHOOK_CONTAINER" --format '{{.State.ExitCode}}')"
[[ "$exit_code" == 0 ]] || die "webhook app exit code=$exit_code"
sudo -n docker logs "$WEBHOOK_CONTAINER" > "$LOGS_DIR/webhook-app.log" 2>&1
sudo -n docker rm "$WEBHOOK_CONTAINER" >/dev/null
# Restore the synthetic Telegram fixture to no-webhook state so the polling cycle remains isolated.
sudo -n docker run --rm -i --network "$DOCKER_NETWORK" \
  --mount "type=bind,src=$CERTS_DIR/ca.crt,dst=/ca.crt,readonly" \
  "$PYTHON_IMAGE" python - <<'PY' > "$RUN_DIR/webhook-delete-response.txt"
import ssl,urllib.request
ctx=ssl.create_default_context(cafile='/ca.crt')
req=urllib.request.Request('https://api.telegram.org/bot1234567890:TEST_TOKEN/deleteWebhook',data=b'',method='POST')
with urllib.request.urlopen(req,context=ctx,timeout=5) as r: print(r.status); print(r.read().decode())
PY

grep -q '^200$' "$RUN_DIR/webhook-delete-response.txt"
export OPQUAL_WEBHOOK_EXIT_CODE="$exit_code" OPQUAL_WEBHOOK_EVENT_BEFORE="$event_before"
python3 - <<'PY'
import json,os
out={'result':'PASS','container':'kp-opqual-webhook-app','published_host_ports':0,
     'webhook_http':'PASS','dispatcher_help_response':'PASS','telegram_webhook_server_task':'PASS',
     'exit_code':int(os.environ['OPQUAL_WEBHOOK_EXIT_CODE']),'synthetic_only':True,'production_contact':False}
with open(os.path.join(os.environ['RUN_DIR'],'webhook-cycle.json'),'w') as f:
    json.dump(out,f,indent=2,sort_keys=True); f.write('\n')
PY
record_op WEBHOOK_CYCLE VERIFIED "exact candidate webhook server accepted synthetic /webhook update; dispatcher replied; clean TERM exit; synthetic webhook state deleted"
info "WEBHOOK_CYCLE=PASS"