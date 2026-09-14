#!/usr/bin/env bash
# shellcheck disable=SC2024
set -Eeuo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/opqual/lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"
init_run
require_phase fixtures
record_op START_APP PLANNED "launch exact qualified binary in task-owned internal container and verify readiness/application_name"

if is_dry_run; then
  info "DRY-RUN: would start exact binary SHA $EXPECTED_BINARY_SHA256"
  exit 0
fi
verify_candidate_identity

if container_exists "$APP_CONTAINER"; then
  assert_owned_container "$APP_CONTAINER"
  if [[ "$(sudo -n docker inspect "$APP_CONTAINER" --format '{{.State.Running}}')" != true ]]; then
    die "existing owned app container is stopped; resume requires reconciliation, not blind restart"
  fi
else
  sudo -n docker run -d --name "$APP_CONTAINER" --network "$DOCKER_NETWORK" --network-alias "$APP_CONTAINER" \
    --label "task=$TASK_ID" --label purpose=exact-candidate \
    --read-only --security-opt no-new-privileges:true --tmpfs /tmp:rw,noexec,nosuid,size=16m \
    --mount "type=bind,src=$BINARY_PATH,dst=/task/kaspa-pulse,readonly" \
    --mount "type=bind,src=$CERTS_DIR,dst=/task/certs,readonly" \
    --mount "type=bind,src=$RUN_DIR,dst=/evidence" \
    -e APP_ENV=development -e RUST_LOG=info -e ENABLE_VERBOSE_LOGS=true \
    -e "DATABASE_URL=postgresql://$RUNTIME_ROLE@$POSTGRES_CONTAINER:5432/$DATABASE?sslmode=disable&application_name=$APPLICATION_NAME" \
    -e DB_MAX_CONNECTIONS=8 -e ALLOW_RUNTIME_SCHEMA_ENSURE=false \
    -e BOT_TOKEN=1234567890:TEST_TOKEN_NOT_REAL -e ADMIN_ID="$SYNTH_ADMIN_ID" \
    -e ADMIN_USER_ID="$SYNTH_ADMIN_ID" -e ADMIN_CHAT_ID="$SYNTH_ADMIN_ID" \
    -e USE_WEBHOOK=false -e "NODE_URL_01=ws://$NODE_CONTAINER:$NODE_PORT" \
    -e KASPA_MONITOR_MODE=polling_only -e KASPA_MONITOR_POLL_INTERVAL_SECS=1 \
    -e KASPA_MONITOR_RECONCILIATION_INTERVAL_SECS=2 -e KASPA_SUBSCRIPTION_MIN_SCAN_INTERVAL_SECS=1 \
    -e READINESS_REQUIRE_NODE=true -e READINESS_REQUIRE_SUBSCRIPTION=false \
    -e READINESS_MAX_SCAN_AGE_SECS=120 -e HEALTH_ENDPOINT_ENABLED=true \
    -e HEALTH_BIND=0.0.0.0 -e "HEALTH_PORT=$HEALTH_PORT" -e HEALTH_ALLOW_PUBLIC_BIND=true \
    -e ENABLE_TELEGRAM_DELIVERY_QUEUE=true -e TELEGRAM_DELIVERY_MAX_ATTEMPTS=5 \
    -e RATE_LIMIT_COMMANDS_PER_SECOND=1000 -e RATE_LIMIT_CALLBACKS_PER_SECOND=1000 \
    -e RATE_LIMIT_ADD_WALLET_PER_MINUTE=1000 -e MAX_WALLETS_PER_USER=50 \
    -e 'COINGECKO_API_URL=https://api.coingecko.com/api/v3/simple/price?ids=kaspa&vs_currencies=usd&include_market_cap=true' \
    -e 'COINGECKO_MARKET_CHART_RANGE_URL=https://api.coingecko.com/api/v3/coins/kaspa/market_chart/range' \
    -e KAS_PRICE_HISTORY_ENABLED=true -e KAS_PRICE_REFRESH_INTERVAL_SECS=2 \
    -e RPC_TIMEOUT_SECS=5 -e HTTP_TIMEOUT_SECS=5 -e HTTP_CONNECT_TIMEOUT_SECS=2 \
    -e SHUTDOWN_DRAIN_SECS=3 -e SSL_CERT_FILE=/task/certs/ca.crt \
    -e PANIC_EVENT_MARKER_PATH=/evidence/panic_event_pending.json \
    --entrypoint /bin/sh "$POSTGRES_IMAGE" -c \
    'exec /task/kaspa-pulse >>/evidence/app.stdout.log 2>>/evidence/app.stderr.log' \
    > "$RECEIPTS_DIR/app-container-id.txt"
fi
assert_no_published_ports "$APP_CONTAINER"
APP_HOST_PID="$(sudo -n docker inspect "$APP_CONTAINER" --format '{{.State.Pid}}')"
export APP_HOST_PID
APP_CONTAINER_ID="$(sudo -n docker inspect "$APP_CONTAINER" --format '{{.Id}}')"
[[ "$APP_HOST_PID" -gt 1 ]] || die "invalid application host PID"
APP_START_MONOTONIC="$(mono_ns)"
printf 'container_id=%s\nhost_pid=%s\nstart_monotonic_ns=%s\n' "$APP_CONTAINER_ID" "$APP_HOST_PID" "$APP_START_MONOTONIC" \
  > "$RUN_DIR/app-process-identity.txt"
state_set app_container_id "$APP_CONTAINER_ID"
state_set app_host_pid "$APP_HOST_PID"
state_set app_start_monotonic_ns "$APP_START_MONOTONIC"

READY=""
for i in $(seq 1 60); do
  READY="$(sudo -n docker run --rm -i --network "$DOCKER_NETWORK" "$PYTHON_IMAGE" python - <<PY 2>/dev/null || true
import urllib.request
try: print(urllib.request.urlopen('http://$APP_CONTAINER:$HEALTH_PORT/readyz',timeout=2).read().decode().strip())
except Exception: pass
PY
)"
  [[ "$READY" == ready ]] && break
  [[ "$i" -lt 60 ]] || { tail -n 100 "$RUN_DIR/app.stderr.log" >&2 || true; die "application readiness failed"; }
  sleep 1
done

sudo -n docker run --rm -i --network "$DOCKER_NETWORK" "$PYTHON_IMAGE" python - <<PY > "$RUN_DIR/readiness-response.txt"
import urllib.request
for p in ('healthz','readyz','metrics'):
 with urllib.request.urlopen('http://$APP_CONTAINER:$HEALTH_PORT/'+p,timeout=3) as r:
  body=r.read().decode(); print(p,r.status,body[:1000].strip())
PY
grep -q '^healthz 200 ok' "$RUN_DIR/readiness-response.txt" || die "healthz failed"
grep -q '^readyz 200 ready' "$RUN_DIR/readiness-response.txt" || die "readyz failed"
SESSION_COUNT="$(pg_admin -At -c "SELECT count(*) FROM pg_stat_activity WHERE application_name='$APPLICATION_NAME';")"
[[ "$SESSION_COUNT" -gt 0 ]] || die "no PostgreSQL session for application_name=$APPLICATION_NAME"
printf '%s\n' "$SESSION_COUNT" > "$RUN_DIR/app-db-session-count-start.txt"

python3 - <<'PY'
import json,os
obj={'application_name':os.environ['APPLICATION_NAME'],'container':os.environ['APP_CONTAINER'],
     'container_id':open(os.path.join(os.environ['RUN_DIR'],'app-process-identity.txt')).read().splitlines()[0].split('=',1)[1],
     'host_pid':int(os.environ.get('APP_HOST_PID','0') or 0),'health_port':int(os.environ['HEALTH_PORT']),
     'binary_sha256':os.environ['EXPECTED_BINARY_SHA256']}
with open(os.path.join(os.environ['RUN_DIR'],'app-start-verification.json'),'w') as f: json.dump(obj,f,indent=2,sort_keys=True); f.write('\n')
PY
mark_phase start_app VERIFIED
record_op START_APP VERIFIED "exact candidate ready; application_name visible in PostgreSQL"
info "START_APP=PASS pid=$APP_HOST_PID"
