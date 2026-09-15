#!/usr/bin/env bash
set -Eeuo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/opqual/lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"
init_run
record_op CLEANUP PLANNED "remove only task-labelled containers/network/volume; no force-kill success path"

if is_dry_run; then
  info "DRY-RUN: cleanup would target only task=$TASK_ID resources"
  exit 0
fi
capture_docker_inventory
cp "$RUN_DIR/docker-inventory.json" "$RUN_DIR/docker-inventory-before-cleanup.json"
CLEANUP_OK=1

stop_owned() {
  local name="$1" timeout_steps="${2:-80}" fallback_signal="${3:-}"
  container_exists "$name" || return 0
  assert_owned_container "$name"
  if [[ "$(sudo -n docker inspect "$name" --format '{{.State.Running}}')" == true ]]; then
    sudo -n docker kill --signal=TERM "$name" >/dev/null || true
    for _ in $(seq 1 "$timeout_steps"); do
      [[ "$(sudo -n docker inspect "$name" --format '{{.State.Running}}')" == false ]] && break
      sleep 0.25
    done
    if [[ "$(sudo -n docker inspect "$name" --format '{{.State.Running}}')" == true && -n "$fallback_signal" ]]; then
      printf 'cleanup_fallback=%s signal=%s after_TERM_timeout\n' "$name" "$fallback_signal" >> "$RUN_DIR/cleanup-fallbacks.txt"
      sudo -n docker kill --signal="$fallback_signal" "$name" >/dev/null || true
      for _ in $(seq 1 40); do
        [[ "$(sudo -n docker inspect "$name" --format '{{.State.Running}}')" == false ]] && break
        sleep 0.25
      done
    fi
    if [[ "$(sudo -n docker inspect "$name" --format '{{.State.Running}}')" == true ]]; then
      printf 'cleanup_blocked=%s still_running_after_graceful_signals\n' "$name" >> "$RUN_DIR/cleanup-errors.txt"
      CLEANUP_OK=0
      return 0
    fi
  fi
  sudo -n docker rm "$name" >/dev/null || { CLEANUP_OK=0; printf 'remove_failed=%s\n' "$name" >> "$RUN_DIR/cleanup-errors.txt"; }
}
# Applications first so their graceful shutdown can still use DB/providers.
stop_owned "$WEBHOOK_CONTAINER" 120
stop_owned "$APP_CONTAINER" 120
stop_owned "$LOCK_CONTAINER" 40
stop_owned "$TELEGRAM_CONTAINER" 40 INT
stop_owned "$HTTP_CONTAINER" 40 INT
stop_owned "$NODE_CONTAINER" 40 INT
stop_owned "$POSTGRES_CONTAINER" 120

if volume_exists "$POSTGRES_VOLUME"; then
  assert_owned_volume "$POSTGRES_VOLUME"
  sudo -n docker volume rm "$POSTGRES_VOLUME" >/dev/null || { CLEANUP_OK=0; echo "volume_remove_failed=$POSTGRES_VOLUME" >> "$RUN_DIR/cleanup-errors.txt"; }
fi
if network_exists "$DOCKER_NETWORK"; then
  assert_owned_network "$DOCKER_NETWORK"
  sudo -n docker network rm "$DOCKER_NETWORK" >/dev/null || { CLEANUP_OK=0; echo "network_remove_failed=$DOCKER_NETWORK" >> "$RUN_DIR/cleanup-errors.txt"; }
fi

rm -f "$CERTS_DIR/ca.key" "$CERTS_DIR/server.key" "$CERTS_DIR/server.csr" "$CERTS_DIR/ca.srl"

TASK_CONTAINERS="$(sudo -n docker ps -a --filter "label=task=$TASK_ID" -q | wc -l)"
TASK_NETWORKS="$(sudo -n docker network ls --filter "label=task=$TASK_ID" -q | wc -l)"
TASK_VOLUMES="$(sudo -n docker volume ls --filter "label=task=$TASK_ID" -q | wc -l)"
TASK_PROCESSES="$(task_process_count | tr -d ' ')"
[[ "$TASK_CONTAINERS" == 0 ]] || CLEANUP_OK=0
[[ "$TASK_NETWORKS" == 0 ]] || CLEANUP_OK=0
[[ "$TASK_VOLUMES" == 0 ]] || CLEANUP_OK=0
[[ "$TASK_PROCESSES" == 0 ]] || CLEANUP_OK=0
export TASK_CONTAINERS TASK_NETWORKS TASK_VOLUMES TASK_PROCESSES CLEANUP_OK
python3 - <<'PY'
import json,os,subprocess
run=os.environ['RUN_DIR']; task=os.environ['TASK_ID']
def lines(args): return subprocess.check_output(args,text=True).splitlines()
current_cont={line.split('\t',1)[0]:line.split('\t',1)[1] for line in lines(['sudo','-n','docker','ps','-a','--format','{{.ID}}\t{{.Names}}']) if '\t' in line}
current_net={line.split('\t',1)[0]:line.split('\t',1)[1] for line in lines(['sudo','-n','docker','network','ls','--format','{{.ID}}\t{{.Name}}']) if '\t' in line}
before_path=os.path.join(run,'unrelated-resources-before.json')
preserved=True; missing=[]
if os.path.exists(before_path):
    before=json.load(open(before_path))
    for rid,name in before.get('containers',[]):
        if current_cont.get(rid)!=name: preserved=False; missing.append(['container',rid,name])
    for rid,name in before.get('networks',[]):
        if current_net.get(rid)!=name: preserved=False; missing.append(['network',rid,name])
out={'task_containers':int(os.environ['TASK_CONTAINERS']),'task_networks':int(os.environ['TASK_NETWORKS']),
     'task_volumes':int(os.environ['TASK_VOLUMES']),'task_processes':int(os.environ['TASK_PROCESSES']),
     'unrelated_baseline_resources_preserved':preserved,'missing_or_changed_unrelated_baseline':missing,
     'forced_kill_used_for_application':False,'cleanup_ok':os.environ['CLEANUP_OK']=='1'}
with open(os.path.join(run,'cleanup-receipt.json'),'w') as f: json.dump(out,f,indent=2,sort_keys=True); f.write('\n')
PY

if [[ "$CLEANUP_OK" == 1 ]]; then
  state_set cleanup.completed true
  mark_phase cleanup VERIFIED
  record_op CLEANUP VERIFIED "task resources removed; unrelated baseline resources preserved"
  info "CLEANUP=PASS"
else
  mark_phase cleanup FAILED
  record_op CLEANUP FAILED "one or more task resources could not be removed without force-kill"
  die "cleanup incomplete; evidence preserved"
fi
