#!/usr/bin/env bash
set -Eeuo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/opqual/lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"

[[ "${1:-}" == "--dry-run" ]] && export DRY_RUN=1
init_run
record_op PREFLIGHT PLANNED "verify host, candidate identity, tool/image state, ownership conflicts, resources"

[[ "$(hostname -s)" == "$EXPECTED_HOST" ]] || die "host must be $EXPECTED_HOST"
[[ "$(uname -s)" == Linux ]] || die "Linux required"
[[ "$(uname -m)" == x86_64 ]] || die "x86_64 required"
command -v git >/dev/null || die "git missing"
command -v python3 >/dev/null || die "python3 missing"
command -v sudo >/dev/null || die "sudo missing"
sudo -n docker version >/dev/null 2>&1 || die "Docker daemon unavailable through sudo -n"
verify_no_host_secret_env
verify_candidate_identity
ACTUAL_PG_IMAGE_ID="$(sudo -n docker image inspect "$POSTGRES_IMAGE" --format '{{.Id}}' 2>/dev/null || true)"
[[ "$ACTUAL_PG_IMAGE_ID" == "$EXPECTED_POSTGRES_IMAGE_ID" ]] \
  || die "postgres:18 image mismatch/missing: $ACTUAL_PG_IMAGE_ID"
sudo -n docker image inspect "$PYTHON_IMAGE" >/dev/null 2>&1 || die "$PYTHON_IMAGE missing locally"

for pair in \
  "$POSTGRES_CONTAINER:container" "$TELEGRAM_CONTAINER:container" "$HTTP_CONTAINER:container" \
  "$NODE_CONTAINER:container" "$APP_CONTAINER:container" "$LOCK_CONTAINER:container"; do
  name="${pair%%:*}"
  if container_exists "$name"; then
    [[ "$(sudo -n docker inspect "$name" --format '{{ index .Config.Labels "task" }}')" == "$TASK_ID" ]] \
      || die "conflicting non-owned container exists: $name"
  fi
done
if network_exists "$DOCKER_NETWORK"; then
  [[ "$(sudo -n docker network inspect "$DOCKER_NETWORK" --format '{{ index .Labels "task" }}')" == "$TASK_ID" ]] \
    || die "conflicting non-owned network exists: $DOCKER_NETWORK"
fi
if volume_exists "$POSTGRES_VOLUME"; then
  [[ "$(sudo -n docker volume inspect "$POSTGRES_VOLUME" --format '{{ index .Labels "task" }}')" == "$TASK_ID" ]] \
    || die "conflicting non-owned volume exists: $POSTGRES_VOLUME"
fi
FREE_KB="$(df -Pk "$REPO_ROOT" | awk 'NR==2 {print $4}')"
[[ "$FREE_KB" -ge 1048576 ]] || die "less than 1 GiB free disk"
MEM_KB="$(awk '/MemAvailable:/ {print $2}' /proc/meminfo)"
[[ "$MEM_KB" -ge 524288 ]] || die "less than 512 MiB available memory"
mkdir -p "$EVIDENCE_ROOT"
test -w "$EVIDENCE_ROOT" || die "evidence root not writable"

# No host port is required. Record host occupancy for proof that the harness does not rely on it.
python3 - <<PY
import json,subprocess,os
ports=[443,5432,17110,18080,int(os.environ['HEALTH_PORT'])]
items=[]
for port in ports:
 p=subprocess.run(['ss','-H','-ltn',f'sport = :{port}'],text=True,capture_output=True)
 items.append({'port':port,'host_in_use':bool(p.stdout.strip()),'required_by_harness':False})
with open(os.path.join(os.environ['RUN_DIR'],'host-port-inventory.json'),'w') as f:
 json.dump(items,f,indent=2,sort_keys=True); f.write('\n')
PY
write_identity_json
capture_docker_inventory
python3 - <<'PY2'
import json,os,subprocess
task=os.environ['TASK_ID']; out=os.environ['RUN_DIR']
def lines(args):
 p=subprocess.run(args,text=True,capture_output=True,check=True); return p.stdout.splitlines()
containers=[]
for line in lines(['sudo','-n','docker','ps','-a','--format','{{.ID}}\t{{.Names}}\t{{.Labels}}']):
 parts=line.split('\t',2); labels=parts[2] if len(parts)>2 else ''
 if f'task={task}' not in labels: containers.append(parts[:2])
networks=[]
for line in lines(['sudo','-n','docker','network','ls','--format','{{.ID}}\t{{.Name}}\t{{.Labels}}']):
 parts=line.split('\t',2); labels=parts[2] if len(parts)>2 else ''
 if f'task={task}' not in labels: networks.append(parts[:2])
with open(os.path.join(out,'unrelated-resources-before.json'),'w') as f:
 json.dump({'containers':containers,'networks':networks},f,indent=2,sort_keys=True); f.write('\n')
PY2

if is_dry_run; then
  print_plan
  mark_phase preflight VERIFIED
  state_set dry_run true
  record_op PREFLIGHT VERIFIED "dry-run preflight only; no Docker/database/signal mutations"
  info "DRY_RUN_PREFLIGHT=PASS run_id=$RUN_ID"
  exit 0
fi

mark_phase preflight VERIFIED
record_op PREFLIGHT VERIFIED "candidate/source/binary/tool/isolation prerequisites satisfied"
info "PREFLIGHT=PASS run_id=$RUN_ID"
