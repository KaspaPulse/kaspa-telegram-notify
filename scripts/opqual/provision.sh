#!/usr/bin/env bash
# shellcheck disable=SC2024
set -Eeuo pipefail
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
# shellcheck source=scripts/opqual/lib/common.sh
source "$SCRIPT_DIR/lib/common.sh"
init_run
require_phase preflight
record_op PROVISION PLANNED "create/reconcile internal task network, labelled pgdata volume, PostgreSQL 18"

if is_dry_run; then
  info "DRY-RUN: would provision $DOCKER_NETWORK $POSTGRES_VOLUME $POSTGRES_CONTAINER"
  exit 0
fi

if ! network_exists "$DOCKER_NETWORK"; then
  sudo -n docker network create --internal --label "task=$TASK_ID" --label purpose=opqual "$DOCKER_NETWORK" \
    > "$RECEIPTS_DIR/network-create.txt"
else
  assert_owned_network "$DOCKER_NETWORK"
fi
assert_internal_network

if ! volume_exists "$POSTGRES_VOLUME"; then
  sudo -n docker volume create --label "task=$TASK_ID" --label purpose=postgres-data "$POSTGRES_VOLUME" \
    > "$RECEIPTS_DIR/postgres-volume-create.txt"
else
  assert_owned_volume "$POSTGRES_VOLUME"
fi
if ! container_exists "$POSTGRES_CONTAINER"; then
  sudo -n docker run -d --name "$POSTGRES_CONTAINER" \
    --network "$DOCKER_NETWORK" --network-alias "$POSTGRES_CONTAINER" \
    --label "task=$TASK_ID" --label purpose=postgresql \
    -e POSTGRES_DB="$DATABASE" -e POSTGRES_USER="$PG_ADMIN" \
    -e POSTGRES_HOST_AUTH_METHOD=trust \
    --mount "type=volume,src=$POSTGRES_VOLUME,dst=/var/lib/postgresql" \
    "$POSTGRES_IMAGE" > "$RECEIPTS_DIR/postgres-container-id.txt"
else
  assert_owned_container "$POSTGRES_CONTAINER"
  if [[ "$(sudo -n docker inspect "$POSTGRES_CONTAINER" --format '{{.State.Running}}')" != true ]]; then
    sudo -n docker start "$POSTGRES_CONTAINER" > "$RECEIPTS_DIR/postgres-restart.txt"
  fi
fi
assert_no_published_ports "$POSTGRES_CONTAINER"

for i in $(seq 1 60); do
  if sudo -n docker exec "$POSTGRES_CONTAINER" pg_isready -U "$PG_ADMIN" -d "$DATABASE" >/dev/null 2>&1; then break; fi
  [[ "$i" -lt 60 ]] || die "PostgreSQL did not become ready"
  sleep 1
done

sudo -n docker inspect "$POSTGRES_CONTAINER" --format \
  'name={{.Name}} image={{.Image}} running={{.State.Running}} ports={{json .HostConfig.PortBindings}} labels={{json .Config.Labels}} networks={{json .NetworkSettings.Networks}}' \
  > "$RECEIPTS_DIR/postgres-identity.txt"
python3 - <<'PY'
import json,os,subprocess
def inspect(args): return json.loads(subprocess.check_output(args,text=True))
net=inspect(['sudo','-n','docker','network','inspect',os.environ['DOCKER_NETWORK']])[0]
pg=inspect(['sudo','-n','docker','inspect',os.environ['POSTGRES_CONTAINER']])[0]
out={'network':{'name':net['Name'],'id':net['Id'],'internal':net['Internal'],'labels':net.get('Labels',{})},
     'postgres':{'name':pg['Name'].lstrip('/'),'id':pg['Id'],'image':pg['Image'],'running':pg['State']['Running'],
                 'port_bindings':pg['HostConfig'].get('PortBindings'),'labels':pg['Config'].get('Labels',{})}}
with open(os.path.join(os.environ['RUN_DIR'],'network-map.json'),'w') as f:
 json.dump(out,f,indent=2,sort_keys=True); f.write('\n')
PY
capture_docker_inventory
mark_phase provision VERIFIED
record_op PROVISION VERIFIED "internal-only PostgreSQL 18 environment ready; no host ports"
info "PROVISION=PASS"
