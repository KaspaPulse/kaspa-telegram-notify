#!/usr/bin/env bash
set -Eeuo pipefail

OPQUAL_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
REPO_ROOT="$(cd "$OPQUAL_DIR/../.." && pwd)"

TASK_ID="${OPQUAL_TASK_ID:-kaspa-telegram-opqual-v1}"
DOCKER_NETWORK="${OPQUAL_DOCKER_NETWORK:-kp-opqual-v1}"
POSTGRES_CONTAINER="${OPQUAL_POSTGRES_CONTAINER:-kp-opqual-postgres18}"
POSTGRES_VOLUME="${OPQUAL_POSTGRES_VOLUME:-kp-opqual-v1-pgdata}"
TELEGRAM_CONTAINER="${OPQUAL_TELEGRAM_CONTAINER:-kp-opqual-telegram}"
HTTP_CONTAINER="${OPQUAL_HTTP_CONTAINER:-kp-opqual-http}"
NODE_CONTAINER="${OPQUAL_NODE_CONTAINER:-kp-opqual-node}"
APP_CONTAINER="${OPQUAL_APP_CONTAINER:-kp-opqual-app}"
WEBHOOK_CONTAINER="${OPQUAL_WEBHOOK_CONTAINER:-kp-opqual-webhook-app}"
LOCK_CONTAINER="${OPQUAL_LOCK_CONTAINER:-kp-opqual-lock}"
DATABASE="${OPQUAL_DATABASE:-kaspa_opqual_v1}"
PG_ADMIN="${OPQUAL_PG_ADMIN:-opqual_admin}"
RUNTIME_ROLE="${OPQUAL_RUNTIME_ROLE:-kaspa_pulse_app}"
OBSERVER_ROLE="${OPQUAL_OBSERVER_ROLE:-opqual_observer}"
APPLICATION_NAME="${OPQUAL_APPLICATION_NAME:-kaspa-opqual-v1}"
WEBHOOK_APPLICATION_NAME="${OPQUAL_WEBHOOK_APPLICATION_NAME:-kaspa-opqual-v1-webhook}"
OBSERVER_APP_NAME="${OPQUAL_OBSERVER_APP_NAME:-kaspa-opqual-observer}"
LOCK_APP_NAME="${OPQUAL_LOCK_APP_NAME:-kaspa-opqual-lock-holder}"
HEALTH_PORT="${OPQUAL_HEALTH_PORT:-18480}"
WEBHOOK_PORT="${OPQUAL_WEBHOOK_PORT:-18443}"
WEBHOOK_HEALTH_PORT="${OPQUAL_WEBHOOK_HEALTH_PORT:-18481}"
WEBHOOK_DOMAIN="${OPQUAL_WEBHOOK_DOMAIN:-opqual-webhook.test}"
NODE_PORT="${OPQUAL_NODE_PORT:-17110}"
EXPECTED_HOST="${OPQUAL_EXPECTED_HOST:-kas}"
EXPECTED_SOURCE_HEAD="${EXPECTED_SOURCE_HEAD:-8b380d2f882ddf0e634c5d61e0d73d2c32f23220}"
EXPECTED_BINARY_SHA256="${EXPECTED_BINARY_SHA256:-eb7ee2be53365a745cf2c532e0b694d06cb38473110edb5850ff340a446bf6f1}"
EXPECTED_POSTGRES_IMAGE_ID="${OPQUAL_EXPECTED_POSTGRES_IMAGE_ID:-sha256:4ef4dbc939d61acea57712655ddb4b4ab27419c913f94cca0cd57cb3ea3c2280}"
POSTGRES_IMAGE="${OPQUAL_POSTGRES_IMAGE:-postgres:18}"
PYTHON_IMAGE="${OPQUAL_PYTHON_IMAGE:-python:3.13-alpine}"
BINARY_PATH="${OPQUAL_BINARY_PATH:-/home/kas/kaspa-telegram-dev/evidence/KASPA_TELEGRAM_OPERATIONAL_QUALIFICATION_20260914/kaspa-pulse-opqual-amd64}"
SCENARIO_MATRIX="${OPQUAL_SCENARIO_MATRIX:-$OPQUAL_DIR/fixtures/scenario-map.csv}"
EVIDENCE_ROOT="${OPQUAL_EVIDENCE_ROOT:-$REPO_ROOT/evidence/opqual}"
DRY_RUN="${OPQUAL_DRY_RUN:-0}"
RESUME_MODE="${OPQUAL_RESUME_MODE:-0}"
if [[ -n "${OPQUAL_RUN_ID:-}" ]]; then
  RUN_ID="$OPQUAL_RUN_ID"
else
  RUN_ID="$(date -u +%Y%m%dT%H%M%SZ)-$$"
fi
RUN_DIR="$EVIDENCE_ROOT/$RUN_ID"
STATE_FILE="$RUN_DIR/state.json"
OPERATIONS_FILE="$RUN_DIR/operations.jsonl"
CONTROL_DIR="$RUN_DIR/control"
EVENTS_DIR="$RUN_DIR/events"
CERTS_DIR="$RUN_DIR/certs"
LOGS_DIR="$RUN_DIR/logs"
RECEIPTS_DIR="$RUN_DIR/receipts"

export OPQUAL_DIR REPO_ROOT TASK_ID DOCKER_NETWORK POSTGRES_CONTAINER POSTGRES_VOLUME
export TELEGRAM_CONTAINER HTTP_CONTAINER NODE_CONTAINER APP_CONTAINER WEBHOOK_CONTAINER LOCK_CONTAINER
export DATABASE PG_ADMIN RUNTIME_ROLE OBSERVER_ROLE APPLICATION_NAME WEBHOOK_APPLICATION_NAME OBSERVER_APP_NAME LOCK_APP_NAME
export HEALTH_PORT WEBHOOK_PORT WEBHOOK_HEALTH_PORT WEBHOOK_DOMAIN NODE_PORT EXPECTED_HOST EXPECTED_SOURCE_HEAD EXPECTED_BINARY_SHA256
export EXPECTED_POSTGRES_IMAGE_ID POSTGRES_IMAGE PYTHON_IMAGE BINARY_PATH SCENARIO_MATRIX
export EVIDENCE_ROOT RUN_ID RUN_DIR STATE_FILE OPERATIONS_FILE CONTROL_DIR EVENTS_DIR CERTS_DIR LOGS_DIR RECEIPTS_DIR
export DRY_RUN RESUME_MODE
die() { printf 'OPQUAL_FAIL: %s\n' "$*" >&2; exit 1; }
info() { printf '[opqual] %s\n' "$*"; }

mono_ns() {
  python3 - <<'PY'
import time
print(time.monotonic_ns())
PY
}

init_run() {
  umask 077
  mkdir -p "$RUN_DIR" "$CONTROL_DIR" "$EVENTS_DIR" "$CERTS_DIR" "$LOGS_DIR" "$RECEIPTS_DIR"
  if [[ ! -f "$STATE_FILE" ]]; then
    printf '{}\n' > "$STATE_FILE"
  fi
  touch "$OPERATIONS_FILE"
}

record_op() {
  local op="$1" phase="$2" detail="${3:-}"
  OPQUAL_REC_OP="$op" OPQUAL_REC_PHASE="$phase" OPQUAL_REC_DETAIL="$detail" python3 - <<'PY'
import datetime,json,os
p=os.environ['OPERATIONS_FILE']
rec={'op':os.environ['OPQUAL_REC_OP'],'task':os.environ['TASK_ID'],'phase':os.environ['OPQUAL_REC_PHASE'],
     'utc':datetime.datetime.now(datetime.timezone.utc).isoformat(),'host':os.uname().nodename,
     'run_id':os.environ['RUN_ID'],'detail':os.environ.get('OPQUAL_REC_DETAIL','')}
with open(p,'a',encoding='utf-8') as f:
    f.write(json.dumps(rec,sort_keys=True)+'\n'); f.flush(); os.fsync(f.fileno())
PY
}
state_set() {
  local key="$1" value="$2"
  OPQUAL_STATE_KEY="$key" OPQUAL_STATE_VALUE="$value" python3 - <<'PY'
import json,os,tempfile
p=os.environ['STATE_FILE']; key=os.environ['OPQUAL_STATE_KEY']; value=os.environ['OPQUAL_STATE_VALUE']
try:
    data=json.load(open(p,encoding='utf-8'))
except Exception:
    data={}
data[key]=value
fd,tmp=tempfile.mkstemp(prefix='.state.',dir=os.path.dirname(p),text=True)
with os.fdopen(fd,'w',encoding='utf-8') as f:
    json.dump(data,f,sort_keys=True,indent=2); f.write('\n'); f.flush(); os.fsync(f.fileno())
os.replace(tmp,p)
PY
}

state_get() {
  local key="$1"
  OPQUAL_STATE_KEY="$key" python3 - <<'PY'
import json,os
p=os.environ['STATE_FILE']; key=os.environ['OPQUAL_STATE_KEY']
try: data=json.load(open(p,encoding='utf-8'))
except Exception: data={}
print(data.get(key,''))
PY
}

state_is() { [[ "$(state_get "$1")" == "$2" ]]; }

sha256_file() { sha256sum "$1" | awk '{print $1}'; }

container_exists() { sudo -n docker container inspect "$1" >/dev/null 2>&1; }
network_exists() { sudo -n docker network inspect "$1" >/dev/null 2>&1; }
volume_exists() { sudo -n docker volume inspect "$1" >/dev/null 2>&1; }
assert_owned_container() {
  local name="$1"
  container_exists "$name" || die "container missing: $name"
  [[ "$(sudo -n docker inspect "$name" --format '{{ index .Config.Labels "task" }}')" == "$TASK_ID" ]] \
    || die "refusing non-owned container: $name"
}

assert_owned_network() {
  local name="$1"
  network_exists "$name" || die "network missing: $name"
  [[ "$(sudo -n docker network inspect "$name" --format '{{ index .Labels "task" }}')" == "$TASK_ID" ]] \
    || die "refusing non-owned network: $name"
}

assert_owned_volume() {
  local name="$1"
  volume_exists "$name" || die "volume missing: $name"
  [[ "$(sudo -n docker volume inspect "$name" --format '{{ index .Labels "task" }}')" == "$TASK_ID" ]] \
    || die "refusing non-owned volume: $name"
}

assert_no_published_ports() {
  local name="$1" bindings
  assert_owned_container "$name"
  bindings="$(sudo -n docker inspect "$name" --format '{{json .HostConfig.PortBindings}}')"
  [[ "$bindings" == "null" || "$bindings" == "{}" ]] || die "$name publishes host ports: $bindings"
}

assert_internal_network() {
  assert_owned_network "$DOCKER_NETWORK"
  [[ "$(sudo -n docker network inspect "$DOCKER_NETWORK" --format '{{.Internal}}')" == "true" ]] \
    || die "network is not internal: $DOCKER_NETWORK"
}
safe_rm_container() {
  local name="$1"
  container_exists "$name" || return 0
  assert_owned_container "$name"
  [[ "$(sudo -n docker inspect "$name" --format '{{.State.Running}}')" == false ]] \
    || die "refusing to remove running container without prior graceful stop: $name"
  sudo -n docker rm "$name" >/dev/null
}

safe_rm_network() {
  local name="$1"
  network_exists "$name" || return 0
  assert_owned_network "$name"
  sudo -n docker network rm "$name" >/dev/null
}

safe_rm_volume() {
  local name="$1"
  volume_exists "$name" || return 0
  assert_owned_volume "$name"
  sudo -n docker volume rm "$name" >/dev/null
}

verify_candidate_identity() {
  git -C "$REPO_ROOT" cat-file -e "$EXPECTED_SOURCE_HEAD^{commit}" \
    || die "expected source commit not present: $EXPECTED_SOURCE_HEAD"
  [[ -f "$BINARY_PATH" ]] || die "qualified binary missing: $BINARY_PATH"
  [[ "$(sha256_file "$BINARY_PATH")" == "$EXPECTED_BINARY_SHA256" ]] \
    || die "qualified binary SHA mismatch"
  git -C "$REPO_ROOT" diff --quiet "$EXPECTED_SOURCE_HEAD" -- \
    Cargo.toml Cargo.lock rust-toolchain.toml src migrations Dockerfile docker-compose.yml \
    || die "application source differs from expected candidate $EXPECTED_SOURCE_HEAD"
}

verify_no_host_secret_env() {
  local key
  for key in BOT_TOKEN DATABASE_URL NODE_URL_01 ADMIN_ID ADMIN_USER_ID ADMIN_CHAT_ID WEBHOOK_SECRET_TOKEN; do
    if [[ -n "${!key:-}" ]]; then
      die "host environment contains $key; qualification refuses inherited runtime credentials"
    fi
  done
}
write_identity_json() {
  python3 - <<'PY'
import json,os,platform,subprocess
root=os.environ['REPO_ROOT']; out=os.environ['RUN_DIR']
def sh(*a): return subprocess.check_output(a,text=True).strip()
source={
 'expected_source_head':os.environ['EXPECTED_SOURCE_HEAD'],
 'harness_head':sh('git','-C',root,'rev-parse','HEAD'),
 'branch':sh('git','-C',root,'branch','--show-current'),
 'application_diff_from_expected':bool(subprocess.run(['git','-C',root,'diff','--quiet',os.environ['EXPECTED_SOURCE_HEAD'],'--','Cargo.toml','Cargo.lock','rust-toolchain.toml','src','migrations','Dockerfile','docker-compose.yml']).returncode),
}
binary={'path':os.environ['BINARY_PATH'],'expected_sha256':os.environ['EXPECTED_BINARY_SHA256'],'actual_sha256':sh('sha256sum',os.environ['BINARY_PATH']).split()[0]}
env={'task_id':os.environ['TASK_ID'],'run_id':os.environ['RUN_ID'],'host':platform.node(),'machine':platform.machine(),'system':platform.system(),'host_ports_required':False,'production_allowed':False}
for name,obj in [('source-identity.json',source),('binary-identity.json',binary),('environment.json',env)]:
    with open(os.path.join(out,name),'w',encoding='utf-8') as f: json.dump(obj,f,indent=2,sort_keys=True); f.write('\n')
PY
}

capture_docker_inventory() {
  python3 - <<'PY'
import json,os,subprocess
out=os.environ['RUN_DIR']; task=os.environ['TASK_ID']; net=os.environ['DOCKER_NETWORK']
def cmd(args):
 p=subprocess.run(args,text=True,capture_output=True); return p.stdout.strip().splitlines() if p.returncode==0 else []
containers=cmd(['sudo','-n','docker','ps','-a','--filter',f'label=task={task}','--format','{{json .}}'])
networks=cmd(['sudo','-n','docker','network','ls','--filter',f'label=task={task}','--format','{{json .}}'])
volumes=cmd(['sudo','-n','docker','volume','ls','--filter',f'label=task={task}','--format','{{json .}}'])
obj={'containers':[json.loads(x) for x in containers if x], 'networks':[json.loads(x) for x in networks if x], 'volumes':[json.loads(x) for x in volumes if x]}
with open(os.path.join(out,'docker-inventory.json'),'w') as f: json.dump(obj,f,indent=2,sort_keys=True); f.write('\n')
PY
}
task_process_count() {
  pgrep -af "${APP_CONTAINER}|${TASK_ID}" 2>/dev/null | grep -vcE 'pgrep|scripts/opqual' || true
}

pg_admin() {
  assert_owned_container "$POSTGRES_CONTAINER"
  sudo -n docker exec -i -e PGAPPNAME="$OBSERVER_APP_NAME" "$POSTGRES_CONTAINER" \
    psql -X -v ON_ERROR_STOP=1 -U "$PG_ADMIN" -d "$DATABASE" "$@"
}

pg_runtime() {
  assert_owned_container "$POSTGRES_CONTAINER"
  sudo -n docker exec -i -e PGAPPNAME="$APPLICATION_NAME" "$POSTGRES_CONTAINER" \
    psql -X -v ON_ERROR_STOP=1 -U "$RUNTIME_ROLE" -d "$DATABASE" "$@"
}

require_phase() {
  local phase="$1"
  state_is "phase.$phase" VERIFIED || die "required phase not verified: $phase"
}

mark_phase() {
  state_set "phase.$1" "$2"
}

is_dry_run() { [[ "$DRY_RUN" == 1 ]]; }

print_plan() {
  cat <<'EOF'
OPQUAL PLAN:
  preflight -> provision -> migrate -> fixtures -> start-app -> exec01
  -> restart -> run-scenarios -> collect-evidence -> cleanup
No host ports are published. All runtime resources require task ownership labels.
EOF
}

SYNTH_ADMIN_ID="${OPQUAL_SYNTH_ADMIN_ID:-969100001}"
SYNTH_USER_ID="${OPQUAL_SYNTH_USER_ID:-969100002}"
SYNTH_CHAT_ID="${OPQUAL_SYNTH_CHAT_ID:-969100002}"
SYNTH_WALLET="${OPQUAL_SYNTH_WALLET:-kaspa:qqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqqkx9awp4e}"
export SYNTH_ADMIN_ID SYNTH_USER_ID SYNTH_CHAT_ID SYNTH_WALLET
